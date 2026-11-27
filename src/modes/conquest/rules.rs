//! Conquest on the server: taking flags, bleeding tickets, and where each
//! team comes in.

use bevy::math::Vec3;

use super::{CAPTURE_CROWD, CAPTURE_TIME, CONQUEST_TICKETS, TICKET_BLEED};
use crate::{
    map::Map,
    modes::{BotBrief, Kill, ModeSnap, Rules},
    protocol::{FlagSnap, PlayerId, Winner},
    server::Server,
};

pub const RULES: Rules = Rules {
    start,
    tick,
    killed,
    time_up: Server::leading_team,
    spawns,
    snap,
    bot_brief,
    fight_spots,
    ..Rules::NONE
};

/// A conquest flag, as the server keeps it.
#[derive(Clone, Copy, Default)]
pub struct FlagState {
    pub owner: Option<u8>,
    /// -1 (all team 0's) to 1 (all team 1's).
    pub control: f32,
}

impl FlagState {
    /// Fully held by `team`, with nothing to win back.
    pub fn secure(&self, team: u8) -> bool {
        self.owner == Some(team) && self.control * if team == 0 { -1.0 } else { 1.0 } >= 0.999
    }
}

/// What the server keeps of a conquest match: who holds each of the map's
/// flags, and the tickets each team has bled that are not whole yet.
pub struct Conquest {
    pub flags: Vec<FlagState>,
    bleed: [f32; 2],
}

/// The flags as they stand, or none outside a conquest match.
pub fn flags(server: &Server) -> &[FlagState] {
    server.state.downcast_ref::<Conquest>().map_or(&[], |conquest| &conquest.flags)
}

fn start(server: &mut Server) {
    server.team_scores = [CONQUEST_TICKETS; 2];
    server.state = Box::new(Conquest {
        flags: vec![FlagState::default(); server.map.flags.len()],
        bleed: [0.0; 2],
    });
}

/// Whoever outnumbers the other team on a flag pushes it their way: first
/// back to neutral, then on until it is theirs. A flag left alone drifts
/// back to whoever held it. The team holding fewer flags bleeds tickets.
fn tick(server: &mut Server, dt: f32) -> Option<Winner> {
    let conquest = server.state.downcast_mut::<Conquest>()?;
    let mut held = [0_i32; 2];
    let mut changes = Vec::new();
    for (index, point) in server.map.flags.iter().enumerate() {
        let mut present = [0_i32; 2];
        for player in server.players.values().filter(|p| p.alive && point.holds(p.pos)) {
            present[player.team as usize & 1] += 1;
        }
        let flag = &mut conquest.flags[index];
        let rate = dt / CAPTURE_TIME;
        if present == [0, 0] {
            let rest = match flag.owner {
                Some(0) => -1.0,
                Some(_) => 1.0,
                None => 0.0,
            };
            flag.control += (rest - flag.control).clamp(-rate * 0.5, rate * 0.5);
        } else {
            let push = (present[1] - present[0]).clamp(-CAPTURE_CROWD, CAPTURE_CROWD) as f32;
            flag.control = (flag.control + push * rate).clamp(-1.0, 1.0);
        }
        let owner = match flag.owner {
            Some(0) if flag.control >= 0.0 => None,
            Some(1) if flag.control <= 0.0 => None,
            None if flag.control <= -1.0 => Some(0),
            None if flag.control >= 1.0 => Some(1),
            owner => owner,
        };
        if owner != flag.owner {
            flag.owner = owner;
            changes.push(index);
        }
        if let Some(team) = flag.owner {
            held[team as usize & 1] += 1;
        }
    }
    if !changes.is_empty() {
        // Bots going for a flag that changed hands think again.
        for bot in server.players.values_mut().filter_map(|p| p.bot.as_mut()) {
            if bot.objective.is_some_and(|i| changes.contains(&i)) {
                bot.rethink_in = 0.0;
            }
        }
    }
    for team in 0..2 {
        let behind = held[team ^ 1] - held[team];
        if behind <= 0 {
            continue;
        }
        conquest.bleed[team] += behind as f32 * TICKET_BLEED * dt;
        let whole = conquest.bleed[team].floor();
        conquest.bleed[team] -= whole;
        server.team_scores[team] = server.team_scores[team].saturating_sub(whole as u32);
        if server.team_scores[team] == 0 {
            return Some(Winner::Team(team as u8 ^ 1));
        }
    }
    None
}

/// Every death costs the team a ticket.
fn killed(server: &mut Server, kill: &Kill) -> Option<Winner> {
    let tickets = &mut server.team_scores[kill.victim_team as usize];
    *tickets = tickets.saturating_sub(1);
    (*tickets == 0).then_some(Winner::Team(kill.victim_team ^ 1))
}

/// At the team's base or one of the flags it holds.
fn spawns(server: &mut Server, id: PlayerId, enemies: &[Vec3]) -> Option<Vec<(Vec3, f32)>> {
    let (team, choice) = server.players.get(&id).map_or((0, None), |p| (p.team & 1, p.spawn_at));
    let pool = match spawn_group(server, team, enemies, choice) {
        Some(index) => &server.map.flags[index].spawns,
        None => &server.map.bases[team as usize],
    };
    Some(pool.iter().map(|spawn| (spawn.pos, spawn.yaw)).collect())
}

/// Which of the flags `team` holds to spawn at, or `None` for its base. The
/// one picked (`choice`, see `ClientMsg::SpawnAt`) if it can be; otherwise
/// mostly the one nearest a flag still to take (the front), sometimes
/// another, never one with an enemy on it.
fn spawn_group(server: &mut Server, team: u8, enemies: &[Vec3], choice: Option<u8>) -> Option<usize> {
    let (roll, any) = (server.random() % 4, server.random() as usize);
    let points = &server.map.flags;
    let held = flags(server);
    let ours: Vec<usize> = (0..points.len())
        .filter(|&i| held.get(i).is_some_and(|f| f.owner == Some(team)))
        .filter(|&i| !enemies.iter().any(|enemy| points[i].holds(*enemy)))
        .collect();
    match choice {
        Some(0) => return None,
        Some(point) => return ours.contains(&(point as usize - 1)).then_some(point as usize - 1),
        None => {}
    }
    if ours.is_empty() {
        return None;
    }
    if roll == 0 {
        return None;
    }
    let to_take: Vec<Vec3> = (0..points.len())
        .filter(|&i| held.get(i).is_some_and(|f| f.owner != Some(team)))
        .map(|i| points[i].pos)
        .collect();
    if roll == 1 || to_take.is_empty() {
        return Some(ours[any % ours.len()]);
    }
    let front = |i: &usize| to_take.iter().map(|p| p.distance(points[*i].pos)).fold(f32::INFINITY, f32::min);
    ours.iter().copied().min_by(|a, b| front(a).total_cmp(&front(b)))
}

fn snap(server: &Server) -> ModeSnap {
    ModeSnap::Conquest(
        flags(server)
            .iter()
            .map(|flag| FlagSnap {
                owner: flag.owner,
                control: (flag.control * 100.0).round() as i8,
            })
            .collect(),
    )
}

fn bot_brief(server: &Server) -> BotBrief {
    BotBrief {
        flags: flags(server).to_vec(),
        ..BotBrief::default()
    }
}

/// The fighting is around the flags.
fn fight_spots(map: &Map) -> Vec<Vec3> {
    if map.flags.is_empty() {
        return (Rules::NONE.fight_spots)(map);
    }
    map.flags.iter().flat_map(|flag| flag.spawns.iter().map(|s| s.pos)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        map,
        protocol::*,
        server::{Phase, testing::*},
        weapons::KNIFE,
    };

    fn flags_mut(server: &mut Server) -> &mut [FlagState] {
        &mut server.state.downcast_mut::<Conquest>().expect("a conquest match").flags
    }

    /// A tick of conquest alone, ending the match if that wins it.
    fn pass(server: &mut Server, dt: f32) {
        let won = tick(server, dt);
        server.conclude(won);
    }

    #[test]
    fn a_flag_is_taken_by_standing_on_it_and_lost_the_same_way() {
        let mut server = conquest_with(0, &[0, 1]);
        server.players.get_mut(&1001).unwrap().pos = Vec3::new(0.0, 100.0, 0.0);
        stand_on_flag(&mut server, 1000, 0);
        let tick = 1.0 / 60.0;
        for _ in 0..(CAPTURE_TIME * 0.9 / tick) as usize {
            pass(&mut server, tick);
        }
        assert_eq!(flags(&server)[0].owner, None, "taken too soon");
        for _ in 0..(CAPTURE_TIME * 0.2 / tick) as usize {
            pass(&mut server, tick);
        }
        assert_eq!(flags(&server)[0].owner, Some(0));
        // Both on it: contested, nothing moves.
        stand_on_flag(&mut server, 1001, 0);
        let before = flags(&server)[0].control;
        for _ in 0..120 {
            pass(&mut server, tick);
        }
        assert_eq!(flags(&server)[0].control, before);
        // The enemy alone neutralizes it.
        server.players.get_mut(&1000).unwrap().pos = Vec3::new(0.0, 100.0, 0.0);
        for _ in 0..(CAPTURE_TIME * 1.05 / tick) as usize {
            pass(&mut server, tick);
        }
        assert_eq!(flags(&server)[0].owner, None);
    }

    #[test]
    fn deaths_and_holding_fewer_flags_cost_tickets() {
        let mut server = conquest_with(0, &[0, 1]);
        assert_eq!(server.team_scores, [CONQUEST_TICKETS; 2]);
        server.damage(1000, 1001, 500, 0);
        assert_eq!(server.team_scores, [CONQUEST_TICKETS - 1, CONQUEST_TICKETS]);
        // Team 1 holds two flags to none: team 0 bleeds half a ticket a second.
        for flag in &mut flags_mut(&mut server)[..2] {
            *flag = FlagState { owner: Some(1), control: 1.0 };
        }
        for player in server.players.values_mut() {
            player.pos = Vec3::new(0.0, 100.0, 0.0);
        }
        for _ in 0..60 * 4 + 6 {
            pass(&mut server, 1.0 / 60.0);
        }
        assert_eq!(server.team_scores, [CONQUEST_TICKETS - 3, CONQUEST_TICKETS]);
        // Out of tickets: the match is over.
        server.team_scores[0] = 1;
        server.spawn_player(1000);
        server.damage(1000, 1001, 500, 0);
        assert!(server.phase == Phase::Lobby);
    }

    #[test]
    fn in_conquest_the_dead_wait_the_whole_delay() {
        let mut server = conquest_with(0, &[0]);
        server.damage(1000, 1000, 1000, KNIFE);
        for _ in 0..60 * 2 {
            server.update_match(1.0 / 60.0);
        }
        server.handle_message(1000, ClientMsg::Respawn);
        for _ in 0..6 {
            server.update_match(1.0 / 60.0);
        }
        assert!(!server.players[&1000].alive, "asking to respawn skipped the wait");
        for _ in 0..(RESPAWN_DELAY * 60.0) as usize {
            server.update_match(1.0 / 60.0);
        }
        assert!(server.players[&1000].alive, "not respawned after the delay");
    }

    #[test]
    fn conquest_spawns_are_at_the_base_or_a_flag_held() {
        let mut server = conquest_with(2, &[0]);
        let near = |pos: Vec3, spawns: &[map::Spawn]| spawns.iter().any(|s| s.pos.distance(pos) < 0.01);
        for _ in 0..10 {
            server.spawn_player(1000);
            assert!(near(server.players[&1000].pos, &server.map.bases[0]));
        }
        // A flag picked but not held: the base.
        server.handle_message(1000, ClientMsg::SpawnAt { point: Some(3) });
        server.spawn_player(1000);
        assert!(near(server.players[&1000].pos, &server.map.bases[0]));
        flags_mut(&mut server)[2] = FlagState { owner: Some(0), control: -1.0 };
        let mut at_flag = 0;
        for _ in 0..40 {
            server.spawn_player(1000);
            let pos = server.players[&1000].pos;
            assert!(near(pos, &server.map.bases[0]) || near(pos, &server.map.flags[2].spawns));
            at_flag += near(pos, &server.map.flags[2].spawns) as u32;
        }
        assert!(at_flag > 20, "only {at_flag} of 40 at the flag");
        // Picked and held: always there.
        server.handle_message(1000, ClientMsg::SpawnAt { point: Some(3) });
        for _ in 0..10 {
            server.spawn_player(1000);
            assert!(near(server.players[&1000].pos, &server.map.flags[2].spawns));
        }
    }
}
