//! The match: its start, every tick of it, and its end.

use super::*;

impl Server {
    /// True once the match is under way: playing, and the countdown is over.
    pub(crate) fn live(&self) -> bool {
        self.phase == Phase::Playing && self.countdown <= 0.0
    }

    /// The rules of the mode being played.
    pub(crate) fn rules(&self) -> &'static Rules {
        &modes::info(self.settings.mode).rules
    }

    /// Ends the match if a rule just said somebody won it. The rules never
    /// end it themselves: they say who won, and this is told.
    pub(crate) fn conclude(&mut self, won: Option<Winner>) {
        if let Some(winner) = won
            && self.phase == Phase::Playing
        {
            self.end_match(winner);
        }
    }

    pub(crate) fn update_match(&mut self, dt: f32) {
        // Everybody is in place, frozen, while the countdown runs.
        if self.countdown > 0.0 {
            self.countdown = (self.countdown - dt).max(0.0);
            self.send_snapshot(dt);
            return;
        }
        self.time_left -= dt;
        self.elapsed += dt;

        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in &ids {
            let player = self.players.get_mut(id).unwrap();
            player.fire_credit = (player.fire_credit + dt).min(MAX_FIRE_CREDIT);
            let stats = grenade(player.grenade);
            player.grenades = (player.grenades + dt / stats.recharge).min(stats.max);
            // Hurt from too long ago no longer helps anybody's kill.
            player.hurt_by.retain_mut(|(_, ago)| {
                *ago += dt;
                *ago <= ASSIST_WINDOW
            });
            // Health comes back after a while out of trouble.
            player.since_damage += dt;
            if player.alive && player.health < player.kit.health && player.since_damage >= REGEN_DELAY {
                player.regen += REGEN_RATE * dt;
                let whole = player.regen.floor();
                player.regen -= whole;
                player.health = (player.health + whole as i32).min(player.kit.health);
            }
            if !player.alive && modes::info(self.settings.mode).respawns {
                player.respawn_in -= dt;
                if player.respawn_in <= 0.0 {
                    self.spawn_player(*id);
                }
            }
        }
        self.update_footsteps(dt);
        self.update_bots(dt);
        self.noises.retain_mut(|noise| {
            noise.age += dt;
            noise.age < NOISE_LIFETIME
        });
        self.update_bullets(dt);
        if self.phase != Phase::Playing {
            // A bot's bullet won the match.
            return;
        }
        self.update_armor_pickups(dt);
        self.update_cooking(dt);
        self.update_grenades(dt);
        if self.phase == Phase::Playing {
            let won = (self.rules().tick)(self, dt);
            self.conclude(won);
        }
        if self.phase != Phase::Playing {
            // The last explosion (or second on the hill) won the match.
            return;
        }

        self.send_snapshot(dt);

        if self.time_left <= 0.0 {
            let winner = (self.rules().time_up)(self);
            self.end_match(winner);
        }
    }

    /// The team with the better score, a tie has no winner: who wins a team
    /// mode when time is up.
    pub(crate) fn leading_team(&self) -> Winner {
        match self.team_scores[0].cmp(&self.team_scores[1]) {
            std::cmp::Ordering::Greater => Winner::Team(0),
            std::cmp::Ordering::Less => Winner::Team(1),
            std::cmp::Ordering::Equal => Winner::Nobody,
        }
    }

    /// Whoever has the most kills, a tie has no winner.
    pub(crate) fn most_kills(&self) -> Winner {
        let best = self.players.values().map(|p| p.kills).max().unwrap_or(0);
        let mut leaders = self.players.iter().filter(|(_, p)| p.kills == best);
        match (leaders.next(), leaders.next()) {
            (Some((id, _)), None) => Winner::Player(*id),
            _ => Winner::Nobody,
        }
    }

    /// Sends everybody a snapshot, `SNAPSHOT_RATE` times a second.
    pub(crate) fn send_snapshot(&mut self, dt: f32) {
        self.snapshot_timer += dt;
        if self.snapshot_timer >= 1.0 / SNAPSHOT_RATE {
            self.snapshot_timer = 0.0;
            let snapshot = Snapshot {
                time_left: self.time_left.max(0.0),
                team_scores: self.team_scores,
                mode: (self.rules().snap)(self),
                armor_taken: self
                    .armor_respawn
                    .iter()
                    .enumerate()
                    .filter(|(_, left)| **left > 0.0)
                    .fold(0, |bits, (spot, _)| bits | 1 << spot),
                players: self
                    .players
                    .iter()
                    .map(|(id, p)| PlayerSnap {
                        id: *id,
                        pos: p.pos.to_array(),
                        yaw: p.yaw,
                        pitch: p.pitch,
                        flags: p.flags
                            | if p.away { FLAG_AWAY } else { 0 }
                            | if p.kit.guns == Guns::None { FLAG_UNARMED } else { 0 },
                        weapon: p.weapon,
                        sight: p.sight,
                        health: p.health,
                        armor: p.armor,
                        alive: p.alive,
                    })
                    .collect(),
            };
            self.renet.broadcast_message(CH_UNRELIABLE, encode(&snapshot));
        }
    }

    /// Counts down the taken armor pickups, and hands out the ones that are
    /// there to whoever walks into them with room for more.
    pub(crate) fn update_armor_pickups(&mut self, dt: f32) {
        for (spot, left) in self.armor_respawn.iter_mut().enumerate() {
            if *left > 0.0 {
                *left = (*left - dt).max(0.0);
                continue;
            }
            let at = self.map.armor_spots[spot];
            let taker = self.players.iter_mut().find(|(_, p)| {
                let feet = p.pos - Vec3::Y * map::player_half(p.flags & FLAG_CROUCH != 0).y;
                p.alive && p.armor < MAX_ARMOR && map::touches_pickup(feet, at)
            });
            let Some((id, player)) = taker else {
                continue;
            };
            player.armor = (player.armor + ARMOR_PICKUP).min(MAX_ARMOR);
            *left = ARMOR_RESPAWN;
            let msg = ServerMsg::ArmorPickup {
                id: *id,
                spot: spot as u8,
                armor: player.armor,
            };
            self.renet.broadcast_message(CH_RELIABLE, encode(&msg));
        }
    }

    pub(crate) fn start_match(&mut self) {
        self.reload_map_file();
        self.log(format_args!("match started"));
        self.phase = Phase::Playing;
        self.countdown = COUNTDOWN;
        // A time trial has no time limit: it goes on until the leader ends it.
        self.time_left = if modes::info(self.settings.mode).timed { self.settings.match_minutes * 60.0 } else { f32::INFINITY };
        self.elapsed = 0.0;
        self.grenades.clear();
        self.smokes.clear();
        self.bullets.clear();
        self.noises.clear();
        self.team_scores = [0, 0];
        self.armor_respawn = vec![0.0; self.map.armor_spots.len()];
        self.bot_range = typical_range(&self.map, self.settings.mode);
        for player in self.players.values_mut() {
            player.kills = 0;
            player.deaths = 0;
            player.assists = 0;
            player.damage = 0;
            player.hurt_by.clear();
            player.alive = false;
            player.away = false;
            player.role = 0;
        }
        self.state = Box::new(());
        (self.rules().start)(self);
        if is_team_mode(self.settings.mode) {
            self.balance_bots();
        }
        let roster = self.roster_msg();
        self.broadcast(&roster);
        self.broadcast(&ServerMsg::MatchStarted {
            score_limit: score_limit(self.settings.mode, self.settings.frag_limit),
            time_left: self.time_left,
            countdown: self.countdown,
            elapsed: self.elapsed,
        });
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            self.spawn_player(id);
        }
    }

    /// A new round of the match, for a mode that has them: everybody is put
    /// back in (the rules say where, and with what), and held for the
    /// countdown; the clock then runs down from `time`.
    pub(crate) fn start_round(&mut self, time: f32) {
        self.countdown = COUNTDOWN;
        self.time_left = time;
        self.grenades.clear();
        self.smokes.clear();
        self.bullets.clear();
        self.noises.clear();
        // The roster first: who is what this round.
        let roster = self.roster_msg();
        self.broadcast(&roster);
        self.roster_dirty = false;
        self.broadcast(&ServerMsg::RoundStarted {
            countdown: self.countdown,
            time_left: time,
        });
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            self.spawn_player(id);
        }
    }

    /// Takes a living player out of play until they are next spawned,
    /// without their having died: no kill, no death, no body.
    pub(crate) fn take_out(&mut self, id: PlayerId) {
        let Some(player) = self.players.get_mut(&id).filter(|player| player.alive) else {
            return;
        };
        player.alive = false;
        player.away = true;
        player.cooking = None;
        self.broadcast(&ServerMsg::Out { id });
    }

    pub(crate) fn end_match(&mut self, winner: Winner) {
        self.log(format_args!("match ended"));
        self.phase = Phase::Lobby;
        self.grenades.clear();
        self.smokes.clear();
        self.bullets.clear();
        for player in self.players.values_mut() {
            player.alive = false;
            player.cooking = None;
        }
        let roster = self.roster_msg();
        self.broadcast(&roster);
        self.broadcast(&ServerMsg::MatchEnded {
            winner,
            team_scores: self.team_scores,
        });
        self.roster_dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::testing::*;

    #[test]
    fn the_dead_respawn_after_the_delay_or_sooner_when_they_ask() {
        let (mut server, id) = match_with_one_person();
        let tick = |server: &mut Server, seconds: f32| {
            for _ in 0..(seconds * 60.0) as usize {
                server.update_match(1.0 / 60.0);
            }
        };
        // By the delay alone.
        server.damage(id, id, 1000, KNIFE);
        assert!(!server.players[&id].alive);
        tick(&mut server, RESPAWN_DELAY - 0.5);
        assert!(!server.players[&id].alive, "respawned before the delay");
        tick(&mut server, 1.0);
        assert!(server.players[&id].alive, "not respawned after the delay");

        // Asking straight away is ignored; asking a moment later is not.
        server.damage(id, id, 1000, KNIFE);
        server.handle_message(id, ClientMsg::Respawn);
        tick(&mut server, 0.1);
        assert!(!server.players[&id].alive, "a click as they died respawned them");
        tick(&mut server, MIN_RESPAWN_DELAY);
        server.handle_message(id, ClientMsg::Respawn);
        tick(&mut server, 0.1);
        assert!(server.players[&id].alive, "asking to respawn did nothing");
    }

    #[test]
    fn nothing_happens_until_the_countdown_is_over() {
        let (mut server, id) = match_with_one_person();
        server.countdown = COUNTDOWN;
        let time_left = server.time_left;
        server.handle_cook(id);
        assert!(server.players[&id].cooking.is_none(), "a pin was pulled during the countdown");
        for _ in 0..((COUNTDOWN - 0.5) * 60.0) as usize {
            server.update_match(1.0 / 60.0);
        }
        assert_eq!(server.time_left, time_left, "the clock ran during the countdown");
        for _ in 0..60 {
            server.update_match(1.0 / 60.0);
        }
        assert!(server.countdown <= 0.0 && server.time_left < time_left);
        server.handle_cook(id);
        assert!(server.players[&id].cooking.is_some());
    }
}
