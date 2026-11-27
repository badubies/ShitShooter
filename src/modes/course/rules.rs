//! The racing modes on the server: taking the runs players' games time, and
//! ending a race once everybody is through.

use super::{RACE_END_DELAY, RACE_FINISH_GRACE};
use crate::{
    modes::Rules,
    protocol::{PlayerId, RunEvent, ServerMsg, Winner},
    server::Server,
};

pub const RACE: Rules = Rules {
    run: |server, id, event| run(server, id, event, false),
    time_up: best_time,
    ..Rules::NONE
};

pub const TRIAL: Rules = Rules {
    run: |server, id, event| run(server, id, event, true),
    time_up: best_time,
    ..Rules::NONE
};

/// The best time wins.
fn best_time(server: &Server) -> Winner {
    let best = server.players.iter().filter_map(|(id, p)| Some((*id, p.run_time?))).min_by(|a, b| a.1.total_cmp(&b.1));
    best.map_or(Winner::Nobody, |(id, _)| Winner::Player(id))
}

/// A player's run, as their game timed it. Taken if it follows on from what
/// they told us before (checkpoints in order, the finish after the last) and
/// is no longer than the match has been on; then passed on to everybody. A
/// race's first finish leaves the rest `RACE_FINISH_GRACE` to follow, and
/// once everybody is through it ends.
fn run(server: &mut Server, id: PlayerId, event: RunEvent, trial: bool) {
    let Some(course) = &server.map.course else { return };
    let gates = course.gates.len();
    let elapsed = server.elapsed;
    let plausible = |time: f32| time.is_finite() && time > 0.0 && time <= elapsed + 0.5;
    let Some(player) = server.players.get_mut(&id).filter(|p| p.alive) else { return };
    let on = !trial || player.run_on;
    let finished = !trial && player.run_time.is_some();
    match event {
        RunEvent::Started if trial => {
            player.run_on = true;
            player.run_passed = 0;
        }
        RunEvent::Abandoned if trial => {
            player.run_on = false;
            player.run_passed = 0;
        }
        RunEvent::Gate { gate, time } if on && !finished && gate as usize == player.run_passed && plausible(time) => {
            player.run_passed += 1;
        }
        RunEvent::Finished { time } if on && !finished && player.run_passed == gates && plausible(time) => {
            if trial {
                player.run_on = false;
                player.run_passed = 0;
            }
            player.run_time = Some(player.run_time.map_or(time, |best| best.min(time)));
        }
        _ => return,
    }
    server.roster_dirty = true;
    server.broadcast(&ServerMsg::Run { id, event });
    if trial || !matches!(event, RunEvent::Finished { .. }) {
        return;
    }
    let everybody = server.players.values().filter(|p| p.bot.is_none()).all(|p| p.run_time.is_some());
    let wait = if everybody { RACE_END_DELAY } else { RACE_FINISH_GRACE };
    server.time_left = server.time_left.min(wait);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        map,
        protocol::*,
        server::{Phase, ServerSettings},
    };

    /// A match on the course, in `mode`, with `people` in it and the
    /// countdown over.
    fn course_match(mode: u8, people: usize) -> (Server, Vec<PlayerId>) {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode,
            map: map::GAUNTLET,
            bots: 3,
            ..ServerSettings::default()
        })
        .unwrap();
        let ids = (0..people as PlayerId)
            .map(|i| {
                server.join_counter += 1;
                let person = server.new_player(format!("Racer {i}"), None);
                server.players.insert(1000 + i, person);
                1000 + i
            })
            .collect();
        server.start_match();
        server.countdown = 0.0;
        (server, ids)
    }

    fn gates(server: &Server) -> u8 {
        server.map.course.as_ref().unwrap().gates.len() as u8
    }

    /// Through every checkpoint, the first at `from` seconds, one a second.
    fn through_the_gates(server: &mut Server, id: PlayerId, from: f32) {
        for gate in 0..gates(server) {
            server.handle_message(id, ClientMsg::Run(RunEvent::Gate { gate, time: from + gate as f32 }));
        }
    }

    fn told(server: &mut Server, id: PlayerId, event: RunEvent) {
        server.handle_message(id, ClientMsg::Run(event));
    }

    #[test]
    fn nobody_races_bots_or_carries_a_gun() {
        let (mut server, ids) = course_match(MODE_RACE, 2);
        assert_eq!(server.players.len(), 2, "the bots asked for came in");
        server.handle_message(ids[0], ClientMsg::AddBot);
        assert_eq!(server.players.len(), 2);
        let eye = server.players[&ids[0]].pos;
        server.handle_message(ids[0], ClientMsg::Fire { weapon: 0, origin: eye.to_array(), dirs: vec![[0.0, 0.0, -1.0]] });
        server.handle_message(ids[0], ClientMsg::Hit { victim: ids[1], weapon: 0, headshot: true });
        assert_eq!(server.players[&ids[1]].health, MAX_HEALTH);
    }

    #[test]
    fn picking_a_racing_mode_sends_the_bots_home_and_goes_to_the_course() {
        let mut server = Server::bind(ServerSettings { port: 0, bots: 3, ..ServerSettings::default() }).unwrap();
        server.join_counter += 1;
        let person = server.new_player("Leader".into(), None);
        server.players.insert(1000, person);
        server.handle_message(1000, ClientMsg::SetMode(MODE_TRIAL));
        assert_eq!(server.settings.map, map::GAUNTLET);
        assert!(server.map.course.is_some());
        assert_eq!(server.players.len(), 1);
        // And back: to a map with a fight on it.
        server.handle_message(1000, ClientMsg::SetMode(MODE_FFA));
        assert_ne!(server.settings.map, map::GAUNTLET);
        server.handle_message(1000, ClientMsg::SetMap(map::GAUNTLET));
        assert_ne!(server.settings.map, map::GAUNTLET);
    }

    #[test]
    fn a_race_takes_checkpoints_in_order_and_the_first_through_wins() {
        let (mut server, ids) = course_match(MODE_RACE, 3);
        server.elapsed = 40.0;
        // Out of order, or the finish before the checkpoints: not taken.
        told(&mut server, ids[0], RunEvent::Gate { gate: 1, time: 5.0 });
        told(&mut server, ids[0], RunEvent::Finished { time: 9.0 });
        assert_eq!((server.players[&ids[0]].run_passed, server.players[&ids[0]].run_time), (0, None));
        through_the_gates(&mut server, ids[0], 5.0);
        assert_eq!(server.players[&ids[0]].run_passed, gates(&server) as usize);
        // Longer than the race has been on: not taken.
        told(&mut server, ids[0], RunEvent::Finished { time: 99.0 });
        assert_eq!(server.players[&ids[0]].run_time, None);
        told(&mut server, ids[0], RunEvent::Finished { time: 30.0 });
        assert_eq!(server.players[&ids[0]].run_time, Some(30.0));
        assert!(server.time_left <= RACE_FINISH_GRACE);
        // Only one finish each.
        told(&mut server, ids[0], RunEvent::Finished { time: 20.0 });
        assert_eq!(server.players[&ids[0]].run_time, Some(30.0));
        // A slower one; the race goes on for the last.
        through_the_gates(&mut server, ids[1], 6.0);
        told(&mut server, ids[1], RunEvent::Finished { time: 33.0 });
        assert!(server.time_left > RACE_END_DELAY);
        assert_eq!(best_time(&server), Winner::Player(ids[0]));
        // Everybody through: it ends soon after.
        through_the_gates(&mut server, ids[2], 7.0);
        told(&mut server, ids[2], RunEvent::Finished { time: 35.0 });
        assert!(server.time_left <= RACE_END_DELAY);
        server.update_match(RACE_END_DELAY + 0.1);
        assert!(server.phase == Phase::Lobby);
    }

    #[test]
    fn a_time_trial_keeps_everybodys_best_run() {
        let (mut server, ids) = course_match(MODE_TRIAL, 2);
        server.elapsed = 300.0;
        // Not on a run: nothing counts.
        told(&mut server, ids[0], RunEvent::Gate { gate: 0, time: 3.0 });
        assert_eq!(server.players[&ids[0]].run_passed, 0);
        for (time, best) in [(40.0, 40.0), (45.0, 40.0), (38.0, 38.0)] {
            told(&mut server, ids[0], RunEvent::Started);
            through_the_gates(&mut server, ids[0], 5.0);
            told(&mut server, ids[0], RunEvent::Finished { time });
            let player = &server.players[&ids[0]];
            assert_eq!((player.run_time, player.run_on, player.run_passed), (Some(best), false, 0));
        }
        // A run given up counts for nothing.
        told(&mut server, ids[1], RunEvent::Started);
        told(&mut server, ids[1], RunEvent::Gate { gate: 0, time: 3.0 });
        told(&mut server, ids[1], RunEvent::Abandoned);
        told(&mut server, ids[1], RunEvent::Gate { gate: 1, time: 4.0 });
        assert_eq!((server.players[&ids[1]].run_passed, server.players[&ids[1]].run_time), (0, None));
        // A finish does not run the clock down: there is no clock.
        assert_eq!(server.time_left, f32::INFINITY);
        // The leader ends it, and the best time wins.
        assert_eq!(best_time(&server), Winner::Player(ids[0]));
        let leader = server.leader().unwrap();
        server.handle_message(leader, ClientMsg::EndMatch);
        assert!(server.phase == Phase::Lobby);
    }
}
