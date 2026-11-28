//! The sniper mode on the server: whose turn it is on the platform, who has
//! been shot or made it across, and when a turn is over.

use std::collections::HashMap;

use bevy::math::Vec3;

use super::{INTERMISSION, LAPS, ROLE_RUNNER, ROLE_SNIPER, RUNNER_HEALTH, TURN_TIME};
use crate::{
    custom::{FINISH_ZONE, SNIPER_ROLE},
    modes::{Guns, Kill, Kit, ModeSnap, ModeStats, Rules},
    movement::{MOVES_RUNNER, MOVES_SNIPER},
    protocol::{PlayerId, Winner},
    server::Server,
    weapons::SNIPER,
};

pub const RULES: Rules = Rules {
    start,
    tick,
    killed,
    joined,
    time_up: winner,
    spawns,
    kit,
    snap,
    stats,
    ..Rules::NONE
};

/// What the server keeps of a match: the turns, and how the one being
/// played is going.
pub struct Turns {
    /// Who snipes, turn by turn: everybody there at the start, `LAPS` times
    /// over. Somebody who has left is passed over.
    order: Vec<PlayerId>,
    /// How many turns the first lap is.
    lap: usize,
    /// The turn being played: an index into `order`.
    turn: usize,
    /// Between two turns: seconds until the next one starts.
    between: Option<f32>,
    /// This turn: runners shot, and runners through to the platform.
    shot: u8,
    through: u8,
    /// How many times each player has made it across, all match.
    reached: HashMap<PlayerId, u8>,
}

impl Turns {
    fn sniper(&self) -> Option<PlayerId> {
        self.order.get(self.turn).copied()
    }
}

fn turns(server: &Server) -> Option<&Turns> {
    server.state.downcast_ref::<Turns>()
}

/// Everybody there is, in a new order, `LAPS` times over: nobody twice in a row.
fn start(server: &mut Server) {
    let mut ids: Vec<PlayerId> = server.players.keys().copied().collect();
    ids.sort();
    let mut order: Vec<PlayerId> = Vec::new();
    for _ in 0..LAPS {
        for i in (1..ids.len()).rev() {
            let j = server.random() as usize % (i + 1);
            ids.swap(i, j);
        }
        if ids.len() > 1 && order.last() == ids.first() {
            ids.swap(0, 1);
        }
        order.extend(&ids);
    }
    server.state = Box::new(Turns {
        lap: ids.len(),
        order,
        turn: 0,
        between: None,
        shot: 0,
        through: 0,
        reached: HashMap::new(),
    });
    assign_roles(server);
    server.time_left = TURN_TIME;
}

/// Whoever's turn it is, is the sniper; everybody else runs.
fn assign_roles(server: &mut Server) {
    let sniper = turns(server).and_then(Turns::sniper);
    for (id, player) in server.players.iter_mut() {
        player.role = if Some(*id) == sniper { ROLE_SNIPER } else { ROLE_RUNNER };
    }
}

/// The sniper has the rifle and nothing else, and walks; a runner has
/// nothing at all, and goes on foot.
fn kit(server: &Server, id: PlayerId) -> Kit {
    if server.players.get(&id).is_some_and(|player| player.role == ROLE_SNIPER) {
        Kit {
            guns: Guns::Only(vec![SNIPER]),
            knife: false,
            grenades: false,
            armor: 0,
            moves: MOVES_SNIPER,
            ..Kit::standard()
        }
    } else {
        Kit {
            health: RUNNER_HEALTH,
            armor: 0,
            moves: MOVES_RUNNER,
            ..Kit::unarmed()
        }
    }
}

/// The sniper comes in on the platform; the runners at the map's spawns.
fn spawns(server: &mut Server, id: PlayerId, _others: &[Vec3]) -> Option<Vec<(Vec3, f32)>> {
    if server.players.get(&id)?.role != ROLE_SNIPER {
        return None;
    }
    Some(server.map.posts(SNIPER_ROLE).map(|spawn| (spawn.pos, spawn.yaw)).collect())
}

/// A runner shot is out for the turn, and counted.
fn killed(server: &mut Server, kill: &Kill) -> Option<Winner> {
    if kill.scorer.is_some()
        && let Some(turns) = server.state.downcast_mut::<Turns>()
    {
        turns.shot = turns.shot.saturating_add(1);
    }
    None
}

/// Whoever comes late runs from the next turn on, and is the sniper once,
/// at the very end, if the first lap is still being played.
fn joined(server: &mut Server, id: PlayerId) {
    if let Some(player) = server.players.get_mut(&id) {
        player.role = ROLE_RUNNER;
    }
    if let Some(turns) = server.state.downcast_mut::<Turns>()
        && turns.turn < turns.lap
    {
        turns.order.push(id);
    }
}

fn tick(server: &mut Server, dt: f32) -> Option<Winner> {
    let turns = server.state.downcast_mut::<Turns>()?;
    if let Some(left) = &mut turns.between {
        *left -= dt;
        return if *left > 0.0 { None } else { next_turn(server) };
    }
    // Runners in the finish are through: counted, and out of the sniper's sights.
    let through: Vec<PlayerId> = match server.map.zone(FINISH_ZONE) {
        Some(finish) => server
            .players
            .iter()
            .filter(|(_, player)| player.alive && player.role == ROLE_RUNNER && finish.contains(player.pos))
            .map(|(id, _)| *id)
            .collect(),
        None => Vec::new(),
    };
    for id in &through {
        *turns.reached.entry(*id).or_default() += 1;
        turns.through = turns.through.saturating_add(1);
    }
    let sniper = turns.sniper();
    for id in &through {
        server.take_out(*id);
    }
    if !through.is_empty() {
        server.roster_dirty = true;
    }
    // The turn is over when nobody is left running, when the time is up, or
    // when the sniper has gone. (Alone, with nobody to run, it runs its time.)
    let runners = server.players.values().filter(|player| player.role == ROLE_RUNNER).count();
    let running = server.players.values().filter(|player| player.role == ROLE_RUNNER && player.alive).count();
    let sniper_here = sniper.is_some_and(|id| server.players.contains_key(&id));
    if server.time_left <= 0.0 || !sniper_here || (runners > 0 && running == 0) {
        end_turn(server);
    }
    None
}

/// The turn is over: whoever is still running is taken out, and its result
/// shows for a moment before the next one.
fn end_turn(server: &mut Server) {
    let running: Vec<PlayerId> = server
        .players
        .iter()
        .filter(|(_, player)| player.alive && player.role == ROLE_RUNNER)
        .map(|(id, _)| *id)
        .collect();
    for id in running {
        server.take_out(id);
    }
    if let Some(turns) = server.state.downcast_mut::<Turns>() {
        turns.between = Some(INTERMISSION);
    }
    server.time_left = INTERMISSION;
}

/// On to whoever is next and still here; after the last turn, the match is over.
fn next_turn(server: &mut Server) -> Option<Winner> {
    let turns = server.state.downcast_mut::<Turns>()?;
    turns.turn += 1;
    while turns.sniper().is_some_and(|id| !server.players.contains_key(&id)) {
        turns.turn += 1;
    }
    if turns.sniper().is_none() {
        return Some(winner(server));
    }
    turns.between = None;
    turns.shot = 0;
    turns.through = 0;
    assign_roles(server);
    server.start_round(TURN_TIME);
    None
}

/// The most runners shot wins; between those level, whoever made it across
/// more often; still level, nobody.
fn winner(server: &Server) -> Winner {
    let mut scores: Vec<(u32, u8, PlayerId)> =
        server.players.iter().map(|(id, player)| (player.kills, reached(server, *id), *id)).collect();
    scores.sort_by(|a, b| (b.0, b.1).cmp(&(a.0, a.1)));
    match scores.as_slice() {
        [only] => Winner::Player(only.2),
        [best, next, ..] if (best.0, best.1) != (next.0, next.1) => Winner::Player(best.2),
        _ => Winner::Nobody,
    }
}

/// How many times `id` has made it across.
pub fn reached(server: &Server, id: PlayerId) -> u8 {
    turns(server).and_then(|turns| turns.reached.get(&id).copied()).unwrap_or(0)
}

fn snap(server: &Server) -> ModeSnap {
    let Some(turns) = turns(server) else { return ModeSnap::None };
    ModeSnap::Sniper {
        sniper: turns.sniper().unwrap_or(0),
        turn: (turns.turn + 1).min(u8::MAX as usize) as u8,
        turns: turns.order.len().min(u8::MAX as usize) as u8,
        shot: turns.shot,
        through: turns.through,
        between: turns.between.is_some(),
    }
}

fn stats(server: &Server, id: PlayerId) -> ModeStats {
    ModeStats::Sniper { reached: reached(server, id) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        course::Zone,
        map::{Block, Map, PLAYER_HALF, Spawn},
        protocol::*,
        server::{Phase, ServerSettings},
        weapons::{RIFLE, WeaponId},
    };

    /// A long floor: runners come in at one end, the sniper stands at the
    /// other, and the finish is just in front of the sniper.
    fn field() -> Map {
        let mut map = Map::empty(0);
        map.blocks.push(Block {
            min: Vec3::new(-20.0, -1.0, -120.0),
            max: Vec3::new(20.0, 0.0, 20.0),
            color: [0.5; 3],
            visible: true,
        });
        let stand = |x: f32, z: f32| Spawn {
            pos: Vec3::new(x, PLAYER_HALF.y + 0.05, z),
            yaw: 0.0,
        };
        map.spawns = (0..8).map(|i| stand(i as f32 * 3.0 - 10.0, 10.0)).collect();
        map.posts.push((SNIPER_ROLE.to_string(), stand(0.0, -110.0)));
        map.zones.push((
            FINISH_ZONE.to_string(),
            Zone {
                center: Vec3::new(0.0, 2.0, -100.0),
                half: Vec3::new(10.0, 3.0, 4.0),
                yaw: 0.0,
            },
        ));
        map.finish()
    }

    /// A match of the sniper mode with `people` in it, the countdown over.
    fn sniper_match(people: usize) -> (Server, Vec<PlayerId>) {
        let mut server = Server::bind(ServerSettings { port: 0, ..ServerSettings::default() }).unwrap();
        server.settings.mode = MODE_SNIPER;
        server.map = field();
        let ids: Vec<PlayerId> = (0..people as PlayerId)
            .map(|i| {
                server.join_counter += 1;
                let person = server.new_player(format!("P{i}"), None);
                server.players.insert(1000 + i, person);
                1000 + i
            })
            .collect();
        server.start_match();
        server.countdown = 0.0;
        (server, ids)
    }

    fn sniper_of(server: &Server) -> PlayerId {
        let mut snipers = server.players.iter().filter(|(_, player)| player.role == ROLE_SNIPER).map(|(id, _)| *id);
        let sniper = snipers.next().expect("a sniper");
        assert!(snipers.next().is_none(), "more than one sniper");
        sniper
    }

    fn runners(server: &Server) -> Vec<PlayerId> {
        let mut ids: Vec<PlayerId> = server.players.iter().filter(|(_, player)| player.role == ROLE_RUNNER).map(|(id, _)| *id).collect();
        ids.sort();
        ids
    }

    fn pass(server: &mut Server, seconds: f32) {
        for _ in 0..(seconds * 60.0).ceil() as usize {
            if server.phase != Phase::Playing {
                return;
            }
            server.update_match(1.0 / 60.0);
        }
    }

    fn between(server: &Server) -> bool {
        matches!(snap(server), ModeSnap::Sniper { between: true, .. })
    }

    /// Lets the turn's time run out, then the wait and the countdown after it.
    fn sit_out_the_turn(server: &mut Server) {
        pass(server, TURN_TIME + 0.1);
        pass(server, INTERMISSION + 0.1);
        server.countdown = 0.0;
    }

    fn into_the_finish(server: &mut Server, id: PlayerId) {
        server.players.get_mut(&id).unwrap().pos = Vec3::new(0.0, 0.95, -100.0);
    }

    #[test]
    fn everybody_is_the_sniper_twice_and_never_twice_running() {
        for people in [1, 2, 3, 5] {
            let (mut server, ids) = sniper_match(people);
            let mut turns_of: HashMap<PlayerId, usize> = HashMap::new();
            let mut last = None;
            let mut played = 0;
            while server.phase == Phase::Playing {
                let sniper = sniper_of(&server);
                assert_eq!(runners(&server).len(), people - 1);
                if people > 1 {
                    assert_ne!(last, Some(sniper), "{people} people: the same sniper twice running");
                }
                last = Some(sniper);
                *turns_of.entry(sniper).or_default() += 1;
                played += 1;
                assert!(played <= people * LAPS, "it never ends");
                sit_out_the_turn(&mut server);
            }
            assert_eq!(played, people * LAPS);
            assert!(ids.iter().all(|id| turns_of[id] == LAPS), "{people} people: {turns_of:?}");
        }
    }

    #[test]
    fn the_sniper_has_the_rifle_alone_and_runners_have_nothing() {
        let (mut server, _) = sniper_match(3);
        let sniper = sniper_of(&server);
        let runner = runners(&server)[0];
        let victim = runners(&server)[1];
        assert_eq!(server.players[&sniper].kit.guns, Guns::Only(vec![SNIPER]));
        assert_eq!(server.players[&runner].kit.guns, Guns::None);
        // The sniper is on the platform, the runners far from it.
        assert!(server.players[&sniper].pos.z < -100.0 && server.players[&runner].pos.z > 0.0);
        let shoot = |server: &mut Server, shooter: PlayerId, weapon: WeaponId| {
            let origin = server.players[&shooter].pos.to_array();
            let dirs = vec![[0.0, 0.0, 1.0]; crate::weapons::weapon(weapon).pellets];
            server.handle_message(shooter, ClientMsg::Fire { weapon, origin, dirs });
            server.handle_message(shooter, ClientMsg::Hit { victim, weapon, headshot: true });
        };
        // A runner has no gun to fire, and no knife.
        shoot(&mut server, runner, RIFLE);
        shoot(&mut server, runner, SNIPER);
        shoot(&mut server, runner, crate::weapons::KNIFE);
        assert!(server.players[&victim].alive && server.players[&victim].health == RUNNER_HEALTH);
        // The sniper has no other gun than the rifle.
        shoot(&mut server, sniper, RIFLE);
        assert!(server.players[&victim].alive && server.players[&victim].health == RUNNER_HEALTH);
        shoot(&mut server, sniper, SNIPER);
        assert!(!server.players[&victim].alive, "the rifle does take a runner out");
        // Nor a grenade.
        server.handle_message(sniper, ClientMsg::Cook);
        assert!(server.players[&sniper].cooking.is_none());
    }

    /// One hit to the body is lived through, and never got over: the next
    /// one is the end of the run, however long after.
    #[test]
    fn a_runner_lives_through_one_body_shot_and_stays_hurt() {
        let (mut server, _) = sniper_match(3);
        let sniper = sniper_of(&server);
        let runner = runners(&server)[0];
        let body = crate::weapons::weapon(SNIPER).damage;
        server.damage(runner, sniper, body, SNIPER);
        assert!(server.players[&runner].alive);
        pass(&mut server, crate::protocol::REGEN_DELAY + 10.0);
        assert_eq!(server.players[&runner].health, RUNNER_HEALTH - body);
        server.damage(runner, sniper, body, SNIPER);
        assert!(!server.players[&runner].alive);
    }

    #[test]
    fn a_runner_shot_is_out_until_the_next_turn() {
        let (mut server, _) = sniper_match(3);
        let sniper = sniper_of(&server);
        let shot = runners(&server)[0];
        server.damage(shot, sniper, 500, SNIPER);
        assert!(!server.players[&shot].alive);
        assert_eq!(server.players[&sniper].kills, 1);
        // Neither the wait nor asking brings them back.
        pass(&mut server, RESPAWN_DELAY + 1.0);
        server.handle_message(shot, ClientMsg::Respawn);
        pass(&mut server, 1.0);
        assert!(!server.players[&shot].alive, "back in before the turn was over");
        assert!(!between(&server), "the turn ended with a runner still running");
        // The next turn puts everybody back in.
        sit_out_the_turn(&mut server);
        assert!(server.players.values().all(|player| player.alive));
    }

    #[test]
    fn getting_to_the_finish_counts_once_a_turn() {
        let (mut server, _) = sniper_match(3);
        let [first, second] = runners(&server)[..] else { panic!("two runners") };
        into_the_finish(&mut server, first);
        pass(&mut server, 0.5);
        assert_eq!((reached(&server, first), reached(&server, second)), (1, 0));
        // Through, and out of the way: not dead, not counted again.
        let player = &server.players[&first];
        assert!(!player.alive && player.away && player.deaths == 0);
        assert!(matches!(snap(&server), ModeSnap::Sniper { through: 1, shot: 0, between: false, .. }));
        assert_eq!(stats(&server, first), ModeStats::Sniper { reached: 1 });
        // The other one too: nobody is left running, so the turn is over.
        into_the_finish(&mut server, second);
        pass(&mut server, 0.1);
        assert!(between(&server));
        assert_eq!((reached(&server, first), reached(&server, second)), (1, 1));
    }

    #[test]
    fn a_turn_ends_when_everybody_is_out_or_the_time_is_up_or_the_sniper_goes() {
        // Everybody shot.
        let (mut server, _) = sniper_match(3);
        let sniper = sniper_of(&server);
        for runner in runners(&server) {
            server.damage(runner, sniper, 500, SNIPER);
        }
        pass(&mut server, 0.1);
        assert!(between(&server));
        assert!(matches!(snap(&server), ModeSnap::Sniper { shot: 2, .. }));
        // The time up: whoever is still running is taken out.
        let (mut server, _) = sniper_match(3);
        pass(&mut server, TURN_TIME - 1.0);
        assert!(!between(&server));
        pass(&mut server, 1.5);
        assert!(between(&server));
        assert!(runners(&server).iter().all(|id| !server.players[id].alive));
        // The sniper gone: on to the next.
        let (mut server, _) = sniper_match(3);
        let sniper = sniper_of(&server);
        server.players.remove(&sniper);
        pass(&mut server, 0.1);
        assert!(between(&server));
        pass(&mut server, INTERMISSION + 0.1);
        assert_ne!(sniper_of(&server), sniper);
    }

    #[test]
    fn the_most_runners_shot_wins_and_getting_across_breaks_a_tie() {
        let (mut server, ids) = sniper_match(3);
        let set = |server: &mut Server, scores: [(u32, u8); 3]| {
            for (id, (kills, across)) in ids.iter().zip(scores) {
                server.players.get_mut(id).unwrap().kills = kills;
                server.state.downcast_mut::<Turns>().unwrap().reached.insert(*id, across);
            }
        };
        set(&mut server, [(2, 0), (3, 0), (1, 5)]);
        assert_eq!(winner(&server), Winner::Player(ids[1]));
        set(&mut server, [(3, 1), (3, 2), (1, 5)]);
        assert_eq!(winner(&server), Winner::Player(ids[1]));
        set(&mut server, [(3, 2), (3, 2), (1, 5)]);
        assert_eq!(winner(&server), Winner::Nobody);
        // And that is who the match ends for, after the last turn.
        set(&mut server, [(4, 0), (3, 2), (1, 5)]);
        let mut ended = None;
        for _ in 0..3 * LAPS {
            sit_out_the_turn(&mut server);
            if server.phase != Phase::Playing {
                ended = Some(winner(&server));
                break;
            }
        }
        assert_eq!(ended, Some(Winner::Player(ids[0])));
    }

    #[test]
    fn somebody_who_comes_late_runs_next_turn_and_snipes_at_the_end() {
        let (mut server, _) = sniper_match(2);
        server.join_counter += 1;
        let late = server.new_player("Late".into(), None);
        server.players.insert(2000, late);
        joined(&mut server, 2000);
        assert!(!server.players[&2000].alive && server.players[&2000].role == ROLE_RUNNER);
        let mut snipers = vec![sniper_of(&server)];
        sit_out_the_turn(&mut server);
        assert!(server.players[&2000].alive, "not put in when the next turn started");
        while server.phase == Phase::Playing {
            snipers.push(sniper_of(&server));
            sit_out_the_turn(&mut server);
        }
        assert_eq!(snipers.len(), 2 * LAPS + 1);
        assert_eq!(snipers.last(), Some(&2000));
    }
}
