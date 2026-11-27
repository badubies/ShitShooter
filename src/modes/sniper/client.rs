//! The sniper mode on the client: what it shows at the top of the screen and
//! on the scoreboard, and the words that come up as a turn goes: what you
//! are this turn, that you made it across, and how the turn ended.

use bevy::prelude::*;

use super::{INTERMISSION, ROLE_RUNNER, ROLE_SNIPER};
use crate::{
    AppState,
    combat::Notices,
    modes::{
        MODE_SNIPER, ModeSnap, ModeStats,
        hud::{Column, KILLS, ModeHud, TopBar},
    },
    net::{NetMsg, Session},
    protocol::{PlayerInfo, ServerMsg},
    ui::{GOOD, RED},
};

pub struct SniperPlugin;

impl Plugin for SniperPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, announce.run_if(in_state(AppState::Playing)));
    }
}

/// How often `player` has made it across.
fn reached(player: &PlayerInfo) -> u8 {
    match player.stats {
        ModeStats::Sniper { reached } => reached,
        _ => 0,
    }
}

/// What we last knew, to tell when it changes.
#[derive(Default)]
struct Seen {
    reached: u8,
    between: bool,
}

/// The words in the middle of the screen: our part as each turn starts,
/// making it across, and how a turn ended.
fn announce(mut messages: MessageReader<NetMsg>, session: Res<Session>, mut notices: ResMut<Notices>, mut seen: Local<Seen>) {
    if session.mode != MODE_SNIPER {
        messages.clear();
        *seen = Seen::default();
        return;
    }
    let me = session.roster.iter().find(|player| player.id == session.my_id);
    for NetMsg(msg) in messages.read() {
        match msg {
            ServerMsg::MatchStarted { .. } => *seen = Seen::default(),
            // Put in for a turn: the roster that came just before says as what.
            ServerMsg::Spawn { id, .. } if *id == session.my_id => {
                let part = match me.map(|player| player.role) {
                    Some(ROLE_SNIPER) => "YOU ARE THE SNIPER",
                    Some(ROLE_RUNNER) => "GET TO THE PLATFORM",
                    _ => continue,
                };
                notices.banner = Some((part.to_string(), 4.0));
            }
            _ => {}
        }
    }
    let across = me.map_or(0, reached);
    if across > seen.reached {
        notices.banner = Some(("YOU MADE IT ACROSS".to_string(), 4.0));
    }
    seen.reached = across;
    let ModeSnap::Sniper { sniper, shot, through, between, .. } = session.snap else {
        return;
    };
    if between && !seen.between {
        let name = if sniper == session.my_id { "YOU".to_string() } else { session.name_of(sniper).to_uppercase() };
        notices.banner = Some((format!("{name} SHOT {shot}   ·   {through} MADE IT ACROSS"), INTERMISSION));
    }
    seen.between = between;
}

// ---------------------------------------------------------------- what the interface shows

pub const HUD: ModeHud = ModeHud {
    top_bar,
    columns: &[KILLS, ACROSS],
    rank: |a, b| b.kills.cmp(&a.kills).then(reached(b).cmp(&reached(a))),
    wins: |_| "MOST RUNNERS SHOT".to_string(),
    countdown: "TURN STARTS IN",
};

const ACROSS: Column = Column::new("ACROSS", |player| reached(player).to_string());

/// This turn's runners shot and runners across, and whose turn it is.
fn top_bar(session: &Session) -> TopBar {
    let ModeSnap::Sniper { sniper, turn, turns, shot, through, between } = session.snap else {
        return TopBar {
            labels: ["SHOT".to_string(), "ACROSS".to_string()],
            scores: ["0".to_string(), "0".to_string()],
            colors: [RED, GOOD],
            info: String::new(),
        };
    };
    let runners = session.roster.iter().filter(|player| player.role == ROLE_RUNNER).count();
    let running = runners.saturating_sub(shot as usize + through as usize);
    let who = if sniper == session.my_id {
        "you are the sniper".to_string()
    } else {
        format!("{} is the sniper", session.name_of(sniper))
    };
    let info = if between {
        format!("turn {turn} of {turns} is over   |   the next one starts in a moment")
    } else {
        format!("{who}   |   turn {turn} of {turns}   |   {running} still running")
    };
    TopBar {
        labels: ["SHOT".to_string(), "ACROSS".to_string()],
        scores: [shot.to_string(), through.to_string()],
        colors: [RED, GOOD],
        info,
    }
}
