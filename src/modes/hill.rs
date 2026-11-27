//! King of the hill: a team scores a point for every second it alone holds
//! the zone in the middle of the map. Kills only clear the way.

use bevy::math::Vec3;

use crate::{
    map::{self, Map},
    modes::{BotBrief, ModeSnap, Rules},
    protocol::Winner,
    server::Server,
};

/// The hill is everything within this distance of the map's hill point,
/// measured along the ground (so any height counts).
pub const HILL_RADIUS: f32 = 7.0;
pub const HILL_SCORE_LIMIT: u32 = 100;

/// Values of `Snapshot::hill`.
pub const HILL_EMPTY: u8 = 0;
/// Held by team 0 or team 1: `HILL_HELD + team`.
pub const HILL_HELD: u8 = 1;
pub const HILL_CONTESTED: u8 = 3;

/// Bots stay within this far of the hill point.
const BOTS_HOLD_WITHIN: f32 = 5.5;

pub const RULES: Rules = Rules {
    start,
    tick,
    time_up: Server::leading_team,
    snap,
    bot_brief,
    fight_spots,
    ..Rules::NONE
};

/// What the server keeps of the hill during a match.
pub struct Hill {
    /// Who holds it right now (`HILL_*`).
    pub held: u8,
    /// Time held that has not become a whole point yet.
    clock: f32,
}

fn start(server: &mut Server) {
    server.state = Box::new(Hill { held: HILL_EMPTY, clock: 0.0 });
}

/// A team alone on the hill scores a point every second.
fn tick(server: &mut Server, dt: f32) -> Option<Winner> {
    let hill = server.state.downcast_mut::<Hill>()?;
    let mut present = [false; 2];
    for player in server.players.values() {
        let from_hill = (player.pos - server.map.hill).with_y(0.0).length();
        if player.alive && from_hill < HILL_RADIUS {
            present[player.team as usize & 1] = true;
        }
    }
    let holder = match present {
        [true, true] => None,
        [true, false] => Some(0),
        [false, true] => Some(1),
        [false, false] => None,
    };
    hill.held = match (holder, present[0]) {
        (Some(team), _) => HILL_HELD + team as u8,
        (None, true) => HILL_CONTESTED,
        (None, false) => HILL_EMPTY,
    };
    let team = holder?;
    hill.clock += dt;
    while hill.clock >= 1.0 {
        hill.clock -= 1.0;
        server.team_scores[team] += 1;
    }
    (server.team_scores[team] >= HILL_SCORE_LIMIT).then_some(Winner::Team(team as u8))
}

fn snap(server: &Server) -> ModeSnap {
    server.state.downcast_ref::<Hill>().map_or(ModeSnap::None, |hill| ModeSnap::Hill(hill.held))
}

fn bot_brief(server: &Server) -> BotBrief {
    BotBrief {
        hold: Some((server.map.hill, BOTS_HOLD_WITHIN)),
        ..BotBrief::default()
    }
}

/// The fighting is around the hill.
fn fight_spots(map: &Map) -> Vec<Vec3> {
    (0..8)
        .map(|k| {
            let around = k as f32 * std::f32::consts::TAU / 8.0;
            map.hill + Vec3::new(around.cos() * 4.0, map::PLAYER_HALF.y, around.sin() * 4.0)
        })
        .filter(|center| map.is_free(*center, map::PLAYER_HALF))
        .collect()
}
