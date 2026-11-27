//! What a mode tells the interface: what goes at the top of the screen, what
//! the scoreboard lists and in which order, and how the match is won. The
//! interface (`ui/`) draws these without knowing which mode it is.

use std::cmp::Ordering;

use bevy::prelude::Color;

use crate::{
    net::Session,
    protocol::PlayerInfo,
    ui::{ACCENT, BLUE, RED, TEXT},
};

pub struct ModeHud {
    /// What goes either side of the clock at the top of the screen, and under it.
    pub top_bar: fn(&Session) -> TopBar,
    /// The scoreboard's columns, beside the names.
    pub columns: &'static [Column],
    /// Who goes higher on the scoreboard.
    pub rank: fn(&PlayerInfo, &PlayerInfo) -> Ordering,
    /// How the match is won, for the scoreboard's caption: "FIRST TO 20".
    pub wins: fn(&Session) -> String,
    /// What it says over the starting countdown.
    pub countdown: &'static str,
}

impl ModeHud {
    /// A mode counted in kills, each for themselves.
    pub const KILLS: ModeHud = ModeHud {
        top_bar: kills_bar,
        columns: &[KILLS, DEATHS, DAMAGE],
        rank: by_kills,
        wins: first_to,
        countdown: "MATCH STARTS IN",
    };

    /// A mode of two teams, each with a score.
    pub const TEAMS: ModeHud = ModeHud {
        top_bar: |session| team_bar(session, first_to(session).to_lowercase()),
        columns: &[KILLS, ASSISTS, DEATHS, DAMAGE],
        ..ModeHud::KILLS
    };
}

/// What goes either side of the clock, and under it.
pub struct TopBar {
    pub labels: [String; 2],
    pub scores: [String; 2],
    pub colors: [Color; 2],
    pub info: String,
}

/// A column of the scoreboard.
pub struct Column {
    pub title: &'static str,
    pub value: fn(&PlayerInfo) -> String,
    /// Twice as wide as the others: a time.
    pub wide: bool,
    /// In the brightest text: the number the mode is counted in.
    pub bright: bool,
}

impl Column {
    pub const fn new(title: &'static str, value: fn(&PlayerInfo) -> String) -> Column {
        Column {
            title,
            value,
            wide: false,
            bright: false,
        }
    }

    pub const fn bright(self) -> Column {
        Column { bright: true, ..self }
    }

    pub const fn wide(self) -> Column {
        Column { wide: true, ..self }
    }
}

pub const KILLS: Column = Column::new("KILLS", |player| player.kills.to_string()).bright();
pub const ASSISTS: Column = Column::new("ASSISTS", |player| player.assists.to_string());
pub const DEATHS: Column = Column::new("DEATHS", |player| player.deaths.to_string());
pub const DAMAGE: Column = Column::new("DAMAGE", |player| player.damage.to_string());

/// The most kills, then the most assists, then the fewest deaths.
pub fn by_kills(a: &PlayerInfo, b: &PlayerInfo) -> Ordering {
    b.kills.cmp(&a.kills).then(b.assists.cmp(&a.assists)).then(a.deaths.cmp(&b.deaths))
}

pub fn first_to(session: &Session) -> String {
    format!("FIRST TO {}", session.score_limit)
}

/// Our kills against the best there are.
fn kills_bar(session: &Session) -> TopBar {
    let my_kills = session.roster.iter().find(|p| p.id == session.my_id).map_or(0, |p| p.kills);
    let leader = session.roster.iter().max_by_key(|p| p.kills);
    let lead = match leader {
        Some(p) if p.kills > 0 => format!("{} leads", p.name),
        _ => "no kills yet".to_string(),
    };
    TopBar {
        labels: ["YOU".to_string(), "BEST".to_string()],
        scores: [my_kills.to_string(), leader.map_or(0, |p| p.kills).to_string()],
        colors: [ACCENT, TEXT],
        info: format!("first to {}   |   {lead}", session.score_limit),
    }
}

/// Our team's score against the enemy's, with `info` under them.
pub fn team_bar(session: &Session, info: String) -> TopBar {
    let mine = session.team_of(session.my_id) as usize;
    TopBar {
        labels: ["YOUR TEAM".to_string(), "ENEMY".to_string()],
        scores: [session.team_scores[mine].to_string(), session.team_scores[mine ^ 1].to_string()],
        colors: [BLUE, RED],
        info,
    }
}
