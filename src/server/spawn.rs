//! Where players come in.

use super::*;

pub(crate) fn crowds(body: Vec3, spawn: Vec3) -> bool {
    let apart = body - spawn;
    apart.with_y(0.0).length() < SPAWN_CLEARANCE && apart.y.abs() < 2.0 * map::PLAYER_HALF.y
}

/// How far apart (along the ground) two players' centres must be for one
/// to spawn by the other: their boxes' width and a step to spare.
pub(crate) const SPAWN_CLEARANCE: f32 = 2.0 * map::PLAYER_HALF.x + 0.7;

impl Server {
    /// Spawns at one of the points farthest away from every living player (in
    /// the team modes: from every living enemy). The mode may say which
    /// points are this player's to come in at.
    pub(crate) fn spawn_player(&mut self, id: PlayerId) {
        let team_mode = is_team_mode(self.settings.mode);
        let my_team = self.players.get(&id).map_or(0, |p| p.team);
        let others: Vec<Vec3> = self
            .players
            .iter()
            .filter(|(other, p)| **other != id && p.alive && !(team_mode && p.team == my_team))
            .map(|(_, p)| p.pos)
            .collect();
        let pool: Vec<(Vec3, f32)> = (self.rules().spawns)(self, id, &others)
            .filter(|pool| !pool.is_empty())
            .unwrap_or_else(|| self.map.spawns.iter().map(|spawn| (spawn.pos, spawn.yaw)).collect());
        // Everybody standing, teammates too: nobody comes in inside them.
        let bodies: Vec<Vec3> = self
            .players
            .iter()
            .filter(|(other, p)| **other != id && p.alive)
            .map(|(_, p)| p.pos)
            .collect();
        let taken = |pos: Vec3| bodies.iter().any(|body| crowds(*body, pos));
        let away = |pos: Vec3| others.iter().map(|other| other.distance(pos)).fold(f32::INFINITY, f32::min);
        let mut ranked: Vec<(f32, Vec3, f32)> = pool
            .iter()
            .filter(|(pos, _)| !taken(*pos))
            .map(|(pos, yaw)| (away(*pos), *pos, *yaw))
            .collect();
        if ranked.is_empty() {
            // Every point taken (more players than points, or a crowd standing
            // on them): somewhere clear near one of them instead.
            ranked = pool
                .iter()
                .flat_map(|(pos, yaw)| {
                    let around = map::spawn_ring(&self.map, Vec2::new(pos.x, pos.z), 1.5, 6.0, 24);
                    around.into_iter().map(move |near| (near.pos, *yaw))
                })
                .filter(|(pos, _)| !taken(*pos))
                .map(|(pos, yaw)| (away(pos), pos, yaw))
                .collect();
        }
        if ranked.is_empty() {
            // Nowhere clear at all: no better than overlapping somebody.
            ranked = pool.iter().map(|(pos, yaw)| (0.0, *pos, *yaw)).collect();
        }
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let pick = (self.rng >> 17) as usize % ranked.len().min(3);
        let (_, pos, yaw) = ranked[pick];
        self.random();
        // A bot picks guns for this map, and not all the same as its team's.
        let teammates: Vec<WeaponId> = self
            .players
            .iter()
            .filter(|(other, p)| **other != id && (!team_mode || p.team == my_team))
            .filter_map(|(_, p)| p.bot.as_ref().map(|bot| bot.primary))
            .collect();
        let mut rng = self.rng;
        let (primary, secondary) = bot_loadout(self.bot_range, &teammates, &mut rng);
        self.rng = rng;

        let kit = (self.rules().kit)(self, id);
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        player.alive = true;
        player.health = kit.health;
        player.armor = kit.armor;
        player.kit = kit.clone();
        player.pos = pos;
        player.yaw = yaw;
        player.pitch = 0.0;
        player.flags = 0;
        // Bots carry one too, though they never throw it.
        player.grenade = player.loadout_grenade;
        player.grenades = grenade(player.grenade).max;
        player.cooking = None;
        player.fire_credit = MAX_FIRE_CREDIT;
        player.since_damage = 0.0;
        player.hurt_by.clear();
        player.spawn_seq += 1;
        let spawn_seq = player.spawn_seq;
        let carried = player.grenade;
        if let Some(bot) = &mut player.bot {
            // A fresh bot with its guns loaded, ready once the first is drawn.
            *bot = Bot::new();
            bot.gun = primary;
            bot.ammo = weapon(primary).magazine;
            bot.spare = secondary;
            bot.spare_ammo = weapon(secondary).magazine;
            bot.primary = primary;
            bot.cooldown = DRAW_TIME;
            player.weapon = primary;
        }
        self.broadcast(&ServerMsg::Spawn {
            id,
            pos: pos.to_array(),
            yaw,
            spawn_seq,
            grenade: carried,
            kit,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No two living players stand in each other.
    fn assert_nobody_overlaps(server: &Server, what: &str) {
        let alive: Vec<(PlayerId, Vec3)> = server.players.iter().filter(|(_, p)| p.alive).map(|(id, p)| (*id, p.pos)).collect();
        for (i, (a, pa)) in alive.iter().enumerate() {
            for (b, pb) in &alive[i + 1..] {
                assert!(!crowds(*pa, *pb), "{what}: {a} at {pa} and {b} at {pb} spawned in each other");
            }
        }
    }

    #[test]
    fn a_full_server_spawns_nobody_inside_anybody_else() {
        for map in 0..map::MAP_NAMES.len() as u8 {
            for mode in (0..modes::count()).filter(|mode| map::plays(map, *mode)) {
                let mut server = Server::bind(ServerSettings { port: 0, mode, map, ..ServerSettings::default() }).unwrap();
                // Nobody races bots: a course fills up with people.
                while server.players.len() < MAX_PLAYERS {
                    if is_course_mode(mode) {
                        server.join_counter += 1;
                        let person = server.new_player(format!("Racer {}", server.players.len()), None);
                        server.players.insert(1000 + server.players.len() as PlayerId, person);
                    } else {
                        server.add_bot();
                    }
                }
                for round in 0..20 {
                    server.start_match();
                    let what = format!("map {map} mode {mode} round {round}");
                    assert_eq!(server.players.values().filter(|p| p.alive).count(), MAX_PLAYERS, "{what}");
                    assert_nobody_overlaps(&server, &what);
                    // And one at a time, coming back while everybody else stands.
                    let ids: Vec<PlayerId> = server.players.keys().copied().collect();
                    for id in ids {
                        server.players.get_mut(&id).unwrap().alive = false;
                        server.spawn_player(id);
                        assert_nobody_overlaps(&server, &what);
                    }
                }
            }
        }
    }

    #[test]
    fn with_too_few_spawns_players_still_come_in_apart() {
        let mut server = Server::bind(ServerSettings { port: 0, map: 0, ..ServerSettings::default() }).unwrap();
        server.map.spawns.truncate(2);
        while server.players.len() < 8 {
            server.add_bot();
        }
        server.start_match();
        assert_nobody_overlaps(&server, "two spawns, eight players");
    }
}
