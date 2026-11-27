//! Grenades: cooking and throwing them, their flight, and what they do going off.

use super::*;

pub(crate) struct Grenade {
    pub(crate) id: u32,
    pub(crate) kind: GrenadeKind,
    pub(crate) owner: PlayerId,
    pub(crate) pos: Vec3,
    pub(crate) vel: Vec3,
    pub(crate) fuse: f32,
}

/// A smoke cloud: bots cannot see through it.
pub(crate) struct Smoke {
    pub(crate) center: Vec3,
    pub(crate) left: f32,
}

impl Server {
    /// A frag's pin is pulled: the fuse burns from now, thrown or not.
    pub(crate) fn handle_cook(&mut self, id: PlayerId) {
        if !self.armed() {
            return;
        }
        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        // A little slack on the recharge for network jitter.
        if !player.alive || player.grenade != FRAG || player.cooking.is_some() || player.grenades < 0.9 {
            return;
        }
        player.grenades = (player.grenades - 1.0).max(0.0);
        player.cooking = Some(grenade(FRAG).fuse);
    }

    /// A frag must have been cooked first; a flash or smoke is taken from
    /// what the thrower carries. Only the kind in their loadout can be thrown.
    pub(crate) fn handle_grenade(&mut self, thrower_id: PlayerId, kind: GrenadeKind, origin: [f32; 3], vel: [f32; 3]) {
        let (pos, vel) = (Vec3::from(origin), Vec3::from(vel));
        if !self.armed() || !pos.is_finite() || !vel.is_finite() {
            return;
        }
        let Some(thrower) = self.players.get_mut(&thrower_id) else {
            return;
        };
        if !thrower.alive || kind != thrower.grenade || pos.distance(thrower.pos) > 6.0 {
            return;
        }
        let fuse = if kind == FRAG {
            let Some(fuse) = thrower.cooking.take() else {
                return;
            };
            fuse
        } else {
            if thrower.grenades < 0.9 {
                return;
            }
            thrower.grenades = (thrower.grenades - 1.0).max(0.0);
            grenade(kind).fuse
        };
        self.launch_grenade(thrower_id, kind, pos, vel.clamp_length_max(70.0), fuse);
    }

    pub(crate) fn launch_grenade(&mut self, owner: PlayerId, kind: GrenadeKind, pos: Vec3, vel: Vec3, fuse: f32) {
        self.next_grenade_id += 1;
        let id = self.next_grenade_id;
        self.grenades.push(Grenade {
            id,
            kind,
            owner,
            pos,
            vel,
            fuse,
        });
        self.broadcast(&ServerMsg::Grenade {
            id,
            kind,
            pos: pos.to_array(),
            vel: vel.to_array(),
            fuse,
        });
    }

    /// Burns the fuses of grenades still in hand. One held too long goes off
    /// right there.
    pub(crate) fn update_cooking(&mut self, dt: f32) {
        let mut in_hand = Vec::new();
        for (id, player) in &mut self.players {
            if let Some(fuse) = &mut player.cooking {
                *fuse -= dt;
                if *fuse <= 0.0 {
                    in_hand.push((*id, player.pos + Vec3::Y * 0.3));
                    player.cooking = None;
                }
            }
        }
        for (owner, pos) in in_hand {
            // A kill can end the match, and then the rest are gone.
            if self.phase != Phase::Playing {
                return;
            }
            self.next_grenade_id += 1;
            let id = self.next_grenade_id;
            self.explode(Grenade {
                id,
                kind: FRAG,
                owner,
                pos,
                vel: Vec3::ZERO,
                fuse: 0.0,
            });
        }
    }

    pub(crate) fn update_grenades(&mut self, dt: f32) {
        self.smokes.retain_mut(|smoke| {
            smoke.left -= dt;
            smoke.left > 0.0
        });
        self.grenade_clock += dt;
        while self.grenade_clock >= GRENADE_STEP {
            self.grenade_clock -= GRENADE_STEP;
            for grenade in &mut self.grenades {
                let contact = self.map.step_grenade(&mut grenade.pos, &mut grenade.vel, GRENADE_STEP);
                grenade.fuse -= GRENADE_STEP;
                let stats = crate::protocol::grenade(grenade.kind);
                if !stats.impact {
                    continue;
                }
                // Impact grenades go off on whatever they touch first: a block,
                // or anybody's body (the thrower's once it has left the hand).
                let airborne = stats.fuse - grenade.fuse;
                let hits_body = self.players.iter().any(|(id, player)| {
                    let half = map::hitbox_half(player.flags & FLAG_CROUCH != 0) + Vec3::splat(map::GRENADE_HALF);
                    player.alive
                        && (*id != grenade.owner || airborne > 0.3)
                        && ((grenade.pos - player.pos).abs().cmple(half)).all()
                });
                if contact > 0.0 || hits_body {
                    grenade.fuse = 0.0;
                }
            }
            let (exploding, flying) = std::mem::take(&mut self.grenades)
                .into_iter()
                .partition(|grenade| grenade.fuse <= 0.0);
            self.grenades = flying;
            for grenade in exploding {
                // A kill can end the match, and then the rest are gone.
                if self.phase == Phase::Playing {
                    self.explode(grenade);
                }
            }
        }
    }

    pub(crate) fn explode(&mut self, grenade: Grenade) {
        self.broadcast(&ServerMsg::Explosion {
            id: grenade.id,
            kind: grenade.kind,
            pos: grenade.pos.to_array(),
        });
        let noise = if grenade.kind == SMOKE { NoiseKind::Pop } else { NoiseKind::Bang };
        self.make_noise(grenade.owner, grenade.pos, noise);
        match grenade.kind {
            FLASH => self.flash(grenade.pos),
            SMOKE => self.smokes.push(Smoke {
                center: map::smoke_center(grenade.pos),
                left: SMOKE_TIME,
            }),
            _ => self.blast(grenade),
        }
    }

    /// A frag: hurts everyone in range who is not behind cover, the thrower included.
    pub(crate) fn blast(&mut self, grenade: Grenade) {
        let center = grenade.pos + Vec3::Y * 0.1;
        let mut hurt = Vec::new();
        for (id, player) in &self.players {
            if !player.alive {
                continue;
            }
            let half = map::player_half(player.flags & FLAG_CROUCH != 0);
            // Distance to the nearest part of their body.
            let distance = center.distance(center.clamp(player.pos - half, player.pos + half));
            let amount = (GRENADE_DAMAGE * (1.0 - distance / GRENADE_RADIUS)).round() as i32;
            let head = player.pos + Vec3::Y * (half.y - 0.15);
            let exposed = self.map.line_of_sight(center, player.pos) || self.map.line_of_sight(center, head);
            if amount > 0 && exposed {
                hurt.push((*id, amount));
            }
        }
        for (id, amount) in hurt {
            // A kill can end the match, and then nobody else gets hurt.
            if self.phase == Phase::Playing {
                self.damage(id, grenade.owner, amount, weapons::GRENADE);
            }
        }
    }

    /// A flashbang blinds the bots that see it. People work out their own
    /// blindness on their clients.
    pub(crate) fn flash(&mut self, pos: Vec3) {
        for player in self.players.values_mut() {
            if !player.alive {
                continue;
            }
            let eye = bot_eye(player);
            let look = map::look_dir(player.yaw, player.pitch);
            if let Some(bot) = &mut player.bot {
                bot.blind = bot.blind.max(self.map.flash_blindness(pos, eye, look));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::testing::*;

    #[test]
    fn a_frag_held_too_long_goes_off_in_hand() {
        let (mut server, id) = match_with_one_person();
        server.handle_cook(id);
        assert!(server.players[&id].cooking.is_some());
        for _ in 0..60 * 3 {
            server.update_match(1.0 / 60.0);
        }
        let person = &server.players[&id];
        assert!(person.cooking.is_none());
        assert_eq!(person.deaths, 1, "the frag in hand did not kill its holder");
    }

    #[test]
    fn a_thrown_frag_keeps_what_is_left_of_the_fuse() {
        let (mut server, id) = match_with_one_person();
        // Nothing to throw before the pin is pulled.
        let pos = server.players[&id].pos.to_array();
        server.handle_grenade(id, FRAG, pos, [0.0; 3]);
        assert!(server.grenades.is_empty());
        server.handle_cook(id);
        for _ in 0..60 {
            server.update_match(1.0 / 60.0);
        }
        server.handle_grenade(id, FRAG, pos, [0.0, 0.0, 5.0]);
        let thrown = &server.grenades[0];
        assert_eq!(thrown.kind, FRAG);
        assert!((thrown.fuse - (grenade(FRAG).fuse - 1.0)).abs() < 0.05, "fuse {}", thrown.fuse);
    }

    #[test]
    fn smoke_goes_off_when_it_lands_and_only_one_is_carried() {
        let (mut server, id) = match_with_one_person_carrying(SMOKE);
        let pos = server.players[&id].pos;
        // Thrown at the floor.
        server.handle_grenade(id, SMOKE, pos.to_array(), [0.0, -10.0, 0.0]);
        assert_eq!(server.grenades.len(), 1);
        for _ in 0..30 {
            server.update_match(1.0 / 60.0);
        }
        assert!(server.grenades.is_empty(), "the smoke did not go off on landing");
        assert_eq!(server.smokes.len(), 1);
        server.handle_grenade(id, SMOKE, pos.to_array(), [0.0, -10.0, 0.0]);
        assert!(server.grenades.is_empty(), "a second smoke was thrown");
    }

    #[test]
    fn a_flash_goes_off_on_hitting_somebody() {
        let (mut server, id) = match_with_one_person_carrying(FLASH);
        server.join_counter += 1;
        let other = server.new_player("Target".into(), None);
        server.players.insert(1001, other);
        server.spawn_player(1001);
        // Two people face to face in the air, well clear of the floor.
        let from = server.map.spawns[0].pos + Vec3::Y * 8.0;
        server.players.get_mut(&id).unwrap().pos = from;
        server.players.get_mut(&1001).unwrap().pos = from + Vec3::X * 3.0;
        server.handle_grenade(id, FLASH, (from + Vec3::X * 0.6).to_array(), [25.0, 0.0, 0.0]);
        for _ in 0..12 {
            server.players.get_mut(&1001).unwrap().pos = from + Vec3::X * 3.0;
            server.update_match(1.0 / 60.0);
        }
        assert!(server.grenades.is_empty(), "the flash flew through the person it hit");
    }

    #[test]
    fn only_the_grenade_in_the_loadout_can_be_thrown() {
        let (mut server, id) = match_with_one_person_carrying(SMOKE);
        let pos = server.players[&id].pos.to_array();
        server.handle_grenade(id, FLASH, pos, [0.0, -10.0, 0.0]);
        server.handle_cook(id);
        assert!(server.grenades.is_empty() && server.players[&id].cooking.is_none());
        server.handle_grenade(id, SMOKE, pos, [0.0, -10.0, 0.0]);
        assert_eq!(server.grenades.len(), 1);
        // A new pick is carried from the next life on.
        server.handle_message(id, ClientMsg::Loadout { grenade: FLASH });
        assert_eq!(server.players[&id].grenade, SMOKE);
        server.spawn_player(id);
        assert_eq!(server.players[&id].grenade, FLASH);
        assert_eq!(server.players[&id].grenades, grenade(FLASH).max);
    }

    #[test]
    fn every_grenade_fits_on_the_hud() {
        for stats in &GRENADES {
            assert!(stats.max as usize <= MOST_GRENADES, "{}", stats.name);
        }
    }

    #[test]
    fn dying_with_a_cooked_frag_drops_it() {
        let (mut server, id) = match_with_one_person();
        server.handle_cook(id);
        server.damage(id, id, 500, KNIFE);
        assert!(!server.players[&id].alive);
        assert_eq!(server.grenades.len(), 1);
        assert_eq!(server.grenades[0].kind, FRAG);
    }
}
