//! Free for all: everybody for themselves, and the first to the frag limit wins.

use crate::{
    modes::{Kill, Rules, score_limit},
    protocol::Winner,
    server::Server,
};

pub const RULES: Rules = Rules {
    killed,
    time_up: Server::most_kills,
    ..Rules::NONE
};

fn killed(server: &mut Server, kill: &Kill) -> Option<Winner> {
    let (kills, _) = kill.scorer?;
    let limit = score_limit(server.settings.mode, server.settings.frag_limit);
    (kills >= limit).then_some(Winner::Player(kill.killer))
}
