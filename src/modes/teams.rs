//! Team deathmatch: every kill is a point for the team.

use crate::{
    modes::{Kill, Rules, score_limit},
    protocol::Winner,
    server::Server,
};

pub const RULES: Rules = Rules {
    killed,
    time_up: Server::leading_team,
    ..Rules::NONE
};

fn killed(server: &mut Server, kill: &Kill) -> Option<Winner> {
    let (_, team) = kill.scorer?;
    let limit = score_limit(server.settings.mode, server.settings.frag_limit);
    server.team_scores[team] += 1;
    (server.team_scores[team] >= limit).then_some(Winner::Team(team as u8))
}
