//! Matches set up for the tests.

use super::*;

/// A conquest match on `map` with people on the given teams.
pub(crate) fn conquest_with(map: u8, teams: &[u8]) -> Server {
    let mut server = Server::bind(ServerSettings {
        port: 0,
        mode: MODE_CONQUEST,
        map,
        ..ServerSettings::default()
    })
    .unwrap();
    for (k, team) in teams.iter().enumerate() {
        server.join_counter += 1;
        let mut person = server.new_player(format!("P{k}"), None);
        person.team = *team;
        server.players.insert(1000 + k as PlayerId, person);
    }
    server.start_match();
    server.countdown = 0.0;
    server
}

pub(crate) fn stand_on_flag(server: &mut Server, id: PlayerId, flag: usize) {
    let pos = server.map.flags[flag].spawns[0].pos;
    server.players.get_mut(&id).unwrap().pos = pos;
}

/// A server with one person in a running match (the countdown over),
/// carrying `grenade`, and that person's id.
pub(crate) fn match_with_one_person_carrying(grenade: GrenadeKind) -> (Server, PlayerId) {
    let mut server = Server::bind(ServerSettings {
        port: 0,
        ..ServerSettings::default()
    })
    .unwrap();
    server.join_counter += 1;
    let mut person = server.new_player("Cook".into(), None);
    person.loadout_grenade = grenade;
    server.players.insert(1000, person);
    server.start_match();
    server.countdown = 0.0;
    (server, 1000)
}

pub(crate) fn match_with_one_person() -> (Server, PlayerId) {
    match_with_one_person_carrying(FRAG)
}
