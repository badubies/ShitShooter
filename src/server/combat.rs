//! Shots, hits and damage: what the server checks of a shot, and what a hit does.

use super::*;

pub(crate) const MAX_FIRE_CREDIT: f32 = 1.5;
/// Hurting somebody earns an assist if they die within this many seconds.
pub(crate) const ASSIST_WINDOW: f32 = 10.0;
pub(crate) const MAX_BULLETS_OUT: u32 = 40;

/// `(health, armor)` after taking `amount` damage. Armor takes the hit first;
/// whatever it cannot hold goes through to health.
pub(crate) fn take_damage(health: i32, armor: i32, amount: i32) -> (i32, i32) {
    let absorbed = amount.clamp(0, armor.max(0));
    ((health - (amount - absorbed)).max(0), armor - absorbed)
}

impl Server {
    /// A shot or knife swing. Checked against the weapon's fire rate, counted
    /// towards the hits this player may claim, and shown to everyone else.
    pub(crate) fn handle_fire(&mut self, shooter_id: PlayerId, weapon_id: WeaponId, origin: [f32; 3], dirs: Vec<[f32; 3]>) {
        let stats = weapon(weapon_id);
        let origin_v = Vec3::from(origin);
        if !self.live()
            || weapon_id as usize >= WEAPONS.len()
            || dirs.len() != stats.pellets
            || !origin_v.is_finite()
            || dirs.iter().any(|dir| !Vec3::from(*dir).is_finite())
        {
            return;
        }
        let Some(shooter) = self.players.get_mut(&shooter_id) else {
            return;
        };
        let cost = stats.interval * 0.8;
        if !shooter.alive || !shooter.kit.allows(weapon_id) || shooter.fire_credit < cost || origin_v.distance(shooter.pos) > 6.0 {
            return;
        }
        shooter.fire_credit -= cost;
        if weapon_id == KNIFE {
            shooter.stab_ready = true;
        } else {
            shooter.bullets_out = (shooter.bullets_out + dirs.len() as u32).min(MAX_BULLETS_OUT);
            self.make_noise(shooter_id, origin_v, NoiseKind::Shot);
        }
        self.renet.broadcast_message_except(
            shooter_id,
            CH_RELIABLE,
            encode(&ServerMsg::Fire {
                shooter: shooter_id,
                weapon: weapon_id,
                origin,
                dirs,
            }),
        );
    }

    /// The shooter's client says one of its bullets, or its knife, reached
    /// `victim_id`. Accepted if the shooter really had a bullet in the air (or
    /// a knife swing going and the victim within reach).
    pub(crate) fn handle_hit(&mut self, shooter_id: PlayerId, victim_id: PlayerId, weapon_id: WeaponId, headshot: bool) {
        if !self.live() || shooter_id == victim_id || weapon_id as usize >= WEAPONS.len() {
            return;
        }
        let Some(victim_pos) = self.players.get(&victim_id).filter(|v| v.alive).map(|v| v.pos) else {
            return;
        };
        // Only with what their kit has: nobody hits with a gun they were not given.
        let Some(shooter) = self.players.get_mut(&shooter_id).filter(|shooter| shooter.kit.allows(weapon_id)) else {
            return;
        };
        let allowed = if weapon_id == KNIFE {
            // Generous reach: both players have moved since the swing.
            std::mem::take(&mut shooter.stab_ready)
                && shooter.alive
                && shooter.pos.distance(victim_pos) < KNIFE_RANGE + 3.0
        } else if shooter.bullets_out > 0 {
            // A bullet may land after its shooter has died.
            shooter.bullets_out -= 1;
            true
        } else {
            false
        };
        if allowed {
            let headshot = headshot && weapon_id != KNIFE;
            self.damage(victim_id, shooter_id, hit_damage(weapon_id, headshot), weapon_id);
        }
    }

    pub(crate) fn damage(&mut self, victim_id: PlayerId, attacker_id: PlayerId, amount: i32, weapon_id: WeaponId) {
        let mode = self.settings.mode;
        let attacker_team = self.players.get(&attacker_id).map(|attacker| attacker.team);
        let attacker_side = self.players.get(&attacker_id).map(|attacker| (attacker.team, attacker.role));
        let sides = modes::info(mode).sides;
        // The dead take no more hurt: the rest of a shotgun's pellets, or a
        // grenade going off over a body, must not kill them again.
        let Some(victim) = self.players.get_mut(&victim_id).filter(|victim| victim.alive) else {
            return;
        };
        let victim_team = victim.team & 1;
        let body = victim.pos;
        // No friendly fire. Your own grenade still hurts you.
        if attacker_id != victim_id && attacker_side.is_some_and(|attacker| sides.allies(attacker, (victim.team, victim.role))) {
            return;
        }
        victim.since_damage = 0.0;
        victim.regen = 0.0;
        if attacker_id != victim_id && amount > 0 {
            match victim.hurt_by.iter_mut().find(|(id, _)| *id == attacker_id) {
                Some((_, ago)) => *ago = 0.0,
                None => victim.hurt_by.push((attacker_id, 0.0)),
            }
        }
        if let Some(bot) = &mut victim.bot
            && attacker_id != victim_id
        {
            bot.provoked_by = Some(attacker_id);
            bot.provoked_for = BOT_GRUDGE;
        }
        let had_armor = victim.armor > 0;
        let before = victim.health + victim.armor;
        (victim.health, victim.armor) = take_damage(victim.health, victim.armor, amount);
        let (health, armor) = (victim.health, victim.armor);
        let dealt = (before - health - armor).max(0) as u32;
        let armor_broken = had_armor && armor == 0;
        let mut dropped = None;
        let mut helpers = Vec::new();
        if health == 0 {
            helpers = std::mem::take(&mut victim.hurt_by);
            victim.alive = false;
            victim.deaths += 1;
            victim.respawn_in = RESPAWN_DELAY;
            dropped = victim.cooking.take().map(|fuse| (fuse, victim.pos));
        }
        self.broadcast(&ServerMsg::Hit {
            victim: victim_id,
            attacker: attacker_id,
            health,
            armor,
            armor_broken,
        });
        if attacker_id != victim_id
            && dealt > 0
            && let Some(attacker) = self.players.get_mut(&attacker_id)
        {
            attacker.damage += dealt;
            self.damage_dirty = true;
        }
        if health > 0 {
            return;
        }
        // A frag with its pin pulled falls from the dead hand, still live.
        if let Some((fuse, pos)) = dropped {
            self.launch_grenade(victim_id, FRAG, pos, Vec3::Y * 2.0, fuse);
        }

        // Everybody else who hurt them helped, if it was their own side that
        // got the kill. Free for all has no sides, so no assists.
        if is_team_mode(mode) {
            for (helper, _) in helpers.into_iter().filter(|(helper, _)| *helper != attacker_id) {
                if let Some(helper) = self.players.get_mut(&helper)
                    && Some(helper.team) == attacker_team
                {
                    helper.assists += 1;
                }
            }
        }
        // Blowing yourself up does not score.
        let mut scorer = None;
        if attacker_id != victim_id
            && let Some(attacker) = self.players.get_mut(&attacker_id)
        {
            attacker.kills += 1;
            scorer = Some((attacker.kills, attacker.team as usize & 1));
            let close = attacker.pos.distance(body) < BOT_GLOAT_RANGE;
            if let Some(bot) = &mut attacker.bot {
                // Whoever it was cross with is dead: no grudge left to get in the way.
                if bot.provoked_by == Some(victim_id) {
                    bot.provoked_by = None;
                }
                if close {
                    bot.gloat = Some(body);
                    bot.gloat_for = BOT_GLOAT_TIME;
                    bot.bobbing = 0.0;
                }
            }
        }
        self.broadcast(&ServerMsg::Kill {
            killer: attacker_id,
            victim: victim_id,
            weapon: weapon_id,
        });
        self.roster_dirty = true;
        // What a kill is worth, and whether it wins the match, is the mode's to say.
        let kill = Kill {
            killer: attacker_id,
            victim_team,
            scorer,
        };
        let won = (self.rules().killed)(self, &kill);
        self.conclude(won);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everybody_who_hurt_the_victim_but_the_killer_gets_an_assist() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode: MODE_TEAMS,
            ..ServerSettings::default()
        })
        .unwrap();
        // 1000 on their own against the other three.
        for k in 0..4 {
            server.join_counter += 1;
            let mut person = server.new_player(format!("P{k}"), None);
            person.team = (k == 0) as u8;
            server.players.insert(1000 + k, person);
        }
        server.start_match();
        server.countdown = 0.0;
        let assists = |server: &Server| (1000..1004).map(|id| server.players[&id].assists).collect::<Vec<_>>();
        // 1001 and 1002 hurt 1000, 1001 twice; 1003 finishes them.
        server.damage(1000, 1001, 10, 0);
        server.damage(1000, 1001, 10, 0);
        server.damage(1000, 1002, 10, 0);
        server.damage(1000, 1003, 500, 0);
        assert_eq!(server.players[&1003].kills, 1);
        assert_eq!(assists(&server), [0, 1, 1, 0]);
        let wait = |server: &mut Server, seconds: f32| {
            for _ in 0..(seconds * 60.0) as usize {
                server.tick(Duration::from_secs_f32(1.0 / 60.0));
            }
        };
        // Hurt 9 seconds before the kill, and healed back to full since: an
        // assist all the same.
        server.spawn_player(1000);
        server.damage(1000, 1001, START_ARMOR + 20, 0);
        wait(&mut server, 9.0);
        assert_eq!(server.players[&1000].health, MAX_HEALTH);
        server.damage(1000, 1003, 500, 0);
        assert_eq!(assists(&server), [0, 2, 1, 0]);
        // Hurt 11 seconds before: too long ago. Hurting them again restarts
        // the clock.
        server.spawn_player(1000);
        server.damage(1000, 1001, 10, 0);
        server.damage(1000, 1002, 10, 0);
        wait(&mut server, 6.0);
        server.damage(1000, 1002, 10, 0);
        wait(&mut server, 5.0);
        server.damage(1000, 1003, 500, 0);
        assert_eq!(assists(&server), [0, 2, 2, 0]);
        // Hurting yourself helps nobody, and is not an assist for you.
        server.spawn_player(1000);
        server.damage(1000, 1000, 10, 0);
        server.damage(1000, 1002, 500, 0);
        assert_eq!(assists(&server), [0, 2, 2, 0]);
        // Hurting somebody who then blows themselves up is no assist: the
        // kill was not your side's.
        server.spawn_player(1000);
        server.damage(1000, 1001, 10, 0);
        server.damage(1000, 1000, 500, 0);
        assert_eq!(assists(&server), [0, 2, 2, 0]);
    }

    #[test]
    fn damage_counts_what_came_off_other_players() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode: MODE_TEAMS,
            ..ServerSettings::default()
        })
        .unwrap();
        // 1000 on their own against 1001 and 1002.
        for k in 0..3 {
            server.join_counter += 1;
            let mut person = server.new_player(format!("P{k}"), None);
            person.team = (k == 0) as u8;
            server.players.insert(1000 + k, person);
        }
        server.start_match();
        server.countdown = 0.0;
        let damage = |server: &Server| (1000..1003).map(|id| server.players[&id].damage).collect::<Vec<_>>();
        // Armor and health both count.
        server.damage(1000, 1001, 10, 0);
        server.damage(1000, 1001, START_ARMOR, 0);
        assert_eq!(damage(&server), [0, 10 + START_ARMOR as u32, 0]);
        // Hurting yourself, and shooting at your own side, do not.
        server.damage(1000, 1000, 10, 0);
        server.damage(1002, 1001, 10, 0);
        assert_eq!(damage(&server), [0, 10 + START_ARMOR as u32, 0]);
        // Overkill does not: only what they had left, which is their health
        // less the 20 taken off it above.
        server.damage(1000, 1002, 500, 0);
        assert_eq!(damage(&server), [0, 10 + START_ARMOR as u32, (MAX_HEALTH - 20) as u32]);
        // A new match starts everybody from nothing.
        server.start_match();
        assert_eq!(damage(&server), [0, 0, 0]);
    }

    #[test]
    fn free_for_all_has_no_assists() {
        let mut server = Server::bind(ServerSettings { port: 0, ..ServerSettings::default() }).unwrap();
        for k in 0..3 {
            server.join_counter += 1;
            let person = server.new_player(format!("P{k}"), None);
            server.players.insert(1000 + k, person);
        }
        server.start_match();
        server.countdown = 0.0;
        server.damage(1000, 1001, 10, 0);
        server.damage(1000, 1002, 500, 0);
        assert_eq!(server.players[&1002].kills, 1);
        assert_eq!(server.players[&1001].assists, 0);
    }

    /// A bot's shotgun blast that kills with its first pellets scores one
    /// kill, not one for every pellet after.
    #[test]
    fn a_shotgun_blast_kills_once() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            bots: 1,
            ..ServerSettings::default()
        })
        .unwrap();
        server.join_counter += 1;
        let human = server.new_player("Target".into(), None);
        server.players.insert(1000, human);
        server.start_match();
        server.countdown = 0.0;
        let bot = server.players.iter().find(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).unwrap();
        let target = server.players[&1000].pos;
        let team = server.players[&bot].team;
        // Nine pellets reaching them on the same tick.
        server.bullets.extend((0..9).map(|_| BotBullet {
            shooter: bot,
            team,
            weapon: weapons::SHOTGUN,
            pos: target + Vec3::Z * 1.0,
            vel: Vec3::NEG_Z * 130.0,
            age: 0.0,
        }));
        server.players.get_mut(&1000).unwrap().health = 10;
        server.players.get_mut(&1000).unwrap().armor = 0;
        server.update_bullets(1.0 / 60.0);
        assert_eq!(server.players[&1000].deaths, 1);
        assert_eq!(server.players[&bot].kills, 1);
    }

    #[test]
    fn armor_soaks_damage_before_health() {
        // All of it stopped by armor.
        assert_eq!(take_damage(100, 25, 20), (100, 5));
        // Armor breaks and the rest goes through.
        assert_eq!(take_damage(100, 5, 20), (85, 0));
        assert_eq!(take_damage(100, 25, 25), (100, 0));
        // No armor: straight to health, never below zero.
        assert_eq!(take_damage(40, 0, 20), (20, 0));
        assert_eq!(take_damage(30, 10, 90), (0, 0));
    }
}
