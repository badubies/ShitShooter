//! The match: its start, every tick of it, and its end.

use super::*;

impl Server {
    /// True once the match is under way: playing, and the countdown is over.
    pub(crate) fn live(&self) -> bool {
        self.phase == Phase::Playing && self.countdown <= 0.0
    }

    /// The match is live and there is fighting in it: not on a course.
    pub(crate) fn armed(&self) -> bool {
        self.live() && modes::info(self.settings.mode).armed
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
            if player.alive && player.health < MAX_HEALTH && player.since_damage >= REGEN_DELAY {
                player.regen += REGEN_RATE * dt;
                let whole = player.regen.floor();
                player.regen -= whole;
                player.health = (player.health + whole as i32).min(MAX_HEALTH);
            }
            if !player.alive {
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
        if self.settings.mode == MODE_HILL && self.phase == Phase::Playing {
            self.update_hill(dt);
        }
        if self.settings.mode == MODE_CONQUEST && self.phase == Phase::Playing {
            self.update_conquest(dt);
        }
        if self.phase != Phase::Playing {
            // The last explosion (or second on the hill) won the match.
            return;
        }

        self.send_snapshot(dt);

        if self.time_left <= 0.0 {
            let winner = self.time_up_winner();
            self.end_match(winner);
        }
    }

    /// Who wins when time is up: the best score, a tie has no winner. On a
    /// course, the best time.
    pub(crate) fn time_up_winner(&self) -> Winner {
        if is_course_mode(self.settings.mode) {
            let best = self.players.iter().filter_map(|(id, p)| Some((*id, p.run_time?))).min_by(|a, b| a.1.total_cmp(&b.1));
            return best.map_or(Winner::Nobody, |(id, _)| Winner::Player(id));
        }
        if is_team_mode(self.settings.mode) {
            return match self.team_scores[0].cmp(&self.team_scores[1]) {
                std::cmp::Ordering::Greater => Winner::Team(0),
                std::cmp::Ordering::Less => Winner::Team(1),
                std::cmp::Ordering::Equal => Winner::Nobody,
            };
        }
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
                flags: self
                    .flags
                    .iter()
                    .map(|flag| FlagSnap {
                        owner: flag.owner,
                        control: (flag.control * 100.0).round() as i8,
                    })
                    .collect(),
                hill: self.hill,
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
                        flags: p.flags,
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

    /// King of the hill: a team alone on the hill scores a point every second.
    pub(crate) fn update_hill(&mut self, dt: f32) {
        let mut present = [false; 2];
        for player in self.players.values() {
            let from_hill = (player.pos - self.map.hill).with_y(0.0).length();
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
        self.hill = match (holder, present[0]) {
            (Some(team), _) => HILL_HELD + team as u8,
            (None, true) => HILL_CONTESTED,
            (None, false) => HILL_EMPTY,
        };
        let Some(team) = holder else {
            return;
        };
        self.hill_clock += dt;
        while self.hill_clock >= 1.0 {
            self.hill_clock -= 1.0;
            self.team_scores[team] += 1;
        }
        if self.team_scores[team] >= HILL_SCORE_LIMIT {
            self.end_match(Winner::Team(team as u8));
        }
    }

    /// Conquest: whoever outnumbers the other team on a flag pushes it their
    /// way: first back to neutral, then on until it is theirs. A flag left
    /// alone drifts back to whoever held it. The team holding fewer flags
    /// bleeds tickets.
    pub(crate) fn update_conquest(&mut self, dt: f32) {
        let mut held = [0_i32; 2];
        let mut changes = Vec::new();
        for (index, point) in self.map.flags.iter().enumerate() {
            let mut present = [0_i32; 2];
            for player in self.players.values().filter(|p| p.alive && point.holds(p.pos)) {
                present[player.team as usize & 1] += 1;
            }
            let flag = &mut self.flags[index];
            let rate = dt / CAPTURE_TIME;
            if present == [0, 0] {
                let rest = match flag.owner {
                    Some(0) => -1.0,
                    Some(_) => 1.0,
                    None => 0.0,
                };
                flag.control += (rest - flag.control).clamp(-rate * 0.5, rate * 0.5);
            } else {
                let push = (present[1] - present[0]).clamp(-CAPTURE_CROWD, CAPTURE_CROWD) as f32;
                flag.control = (flag.control + push * rate).clamp(-1.0, 1.0);
            }
            let owner = match flag.owner {
                Some(0) if flag.control >= 0.0 => None,
                Some(1) if flag.control <= 0.0 => None,
                None if flag.control <= -1.0 => Some(0),
                None if flag.control >= 1.0 => Some(1),
                owner => owner,
            };
            if owner != flag.owner {
                flag.owner = owner;
                changes.push(index);
            }
            if let Some(team) = flag.owner {
                held[team as usize & 1] += 1;
            }
        }
        if !changes.is_empty() {
            // Bots going for a flag that changed hands think again.
            for bot in self.players.values_mut().filter_map(|p| p.bot.as_mut()) {
                if bot.objective.is_some_and(|i| changes.contains(&i)) {
                    bot.rethink_in = 0.0;
                }
            }
        }
        for team in 0..2 {
            let behind = held[team ^ 1] - held[team];
            if behind <= 0 {
                continue;
            }
            self.bleed[team] += behind as f32 * TICKET_BLEED * dt;
            let whole = self.bleed[team].floor();
            self.bleed[team] -= whole;
            self.team_scores[team] = self.team_scores[team].saturating_sub(whole as u32);
            if self.team_scores[team] == 0 {
                self.end_match(Winner::Team(team as u8 ^ 1));
                return;
            }
        }
    }

    /// The racing modes: a player's run, as their game timed it. Taken if
    /// it follows on from what they told us before (checkpoints in order,
    /// the finish after the last) and is no longer than the match has been
    /// on; then passed on to everybody. A race's first finish leaves the
    /// rest `RACE_FINISH_GRACE` to follow, and once everybody is through it
    /// ends.
    pub(crate) fn handle_run(&mut self, id: PlayerId, event: RunEvent) {
        let mode = self.settings.mode;
        if !self.live() || !is_course_mode(mode) {
            return;
        }
        let Some(course) = &self.map.course else { return };
        let gates = course.gates.len();
        let elapsed = self.elapsed;
        let plausible = |time: f32| time.is_finite() && time > 0.0 && time <= elapsed + 0.5;
        let Some(player) = self.players.get_mut(&id).filter(|p| p.alive) else { return };
        let trial = mode == MODE_TRIAL;
        let on = !trial || player.run_on;
        let finished = !trial && player.run_time.is_some();
        match event {
            RunEvent::Started if trial => {
                player.run_on = true;
                player.run_passed = 0;
            }
            RunEvent::Abandoned if trial => {
                player.run_on = false;
                player.run_passed = 0;
            }
            RunEvent::Gate { gate, time } if on && !finished && gate as usize == player.run_passed && plausible(time) => {
                player.run_passed += 1;
            }
            RunEvent::Finished { time } if on && !finished && player.run_passed == gates && plausible(time) => {
                if trial {
                    player.run_on = false;
                    player.run_passed = 0;
                }
                player.run_time = Some(player.run_time.map_or(time, |best| best.min(time)));
            }
            _ => return,
        }
        self.roster_dirty = true;
        self.broadcast(&ServerMsg::Run { id, event });
        if mode != MODE_RACE || !matches!(event, RunEvent::Finished { .. }) {
            return;
        }
        let everybody = self.players.values().filter(|p| p.bot.is_none()).all(|p| p.run_time.is_some());
        let wait = if everybody { RACE_END_DELAY } else { RACE_FINISH_GRACE };
        self.time_left = self.time_left.min(wait);
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
        self.hill = HILL_EMPTY;
        self.hill_clock = 0.0;
        self.armor_respawn = vec![0.0; self.map.armor_spots.len()];
        self.flags = Vec::new();
        self.bleed = [0.0; 2];
        self.bot_range = typical_range(&self.map, self.settings.mode);
        if self.settings.mode == MODE_CONQUEST {
            self.team_scores = [CONQUEST_TICKETS; 2];
            self.flags = vec![FlagState::default(); self.map.flags.len()];
        }
        for player in self.players.values_mut() {
            player.kills = 0;
            player.deaths = 0;
            player.assists = 0;
            player.damage = 0;
            player.hurt_by.clear();
            player.alive = false;
            player.run_passed = 0;
            player.run_on = false;
            player.run_time = None;
        }
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
    fn a_flag_is_taken_by_standing_on_it_and_lost_the_same_way() {
        let mut server = conquest_with(0, &[0, 1]);
        server.players.get_mut(&1001).unwrap().pos = Vec3::new(0.0, 100.0, 0.0);
        stand_on_flag(&mut server, 1000, 0);
        let tick = 1.0 / 60.0;
        for _ in 0..(CAPTURE_TIME * 0.9 / tick) as usize {
            server.update_conquest(tick);
        }
        assert_eq!(server.flags[0].owner, None, "taken too soon");
        for _ in 0..(CAPTURE_TIME * 0.2 / tick) as usize {
            server.update_conquest(tick);
        }
        assert_eq!(server.flags[0].owner, Some(0));
        // Both on it: contested, nothing moves.
        stand_on_flag(&mut server, 1001, 0);
        let before = server.flags[0].control;
        for _ in 0..120 {
            server.update_conquest(tick);
        }
        assert_eq!(server.flags[0].control, before);
        // The enemy alone neutralizes it.
        server.players.get_mut(&1000).unwrap().pos = Vec3::new(0.0, 100.0, 0.0);
        for _ in 0..(CAPTURE_TIME * 1.05 / tick) as usize {
            server.update_conquest(tick);
        }
        assert_eq!(server.flags[0].owner, None);
    }

    #[test]
    fn deaths_and_holding_fewer_flags_cost_tickets() {
        let mut server = conquest_with(0, &[0, 1]);
        assert_eq!(server.team_scores, [CONQUEST_TICKETS; 2]);
        server.damage(1000, 1001, 500, 0);
        assert_eq!(server.team_scores, [CONQUEST_TICKETS - 1, CONQUEST_TICKETS]);
        // Team 1 holds two flags to none: team 0 bleeds half a ticket a second.
        for flag in &mut server.flags[..2] {
            *flag = FlagState { owner: Some(1), control: 1.0 };
        }
        for player in server.players.values_mut() {
            player.pos = Vec3::new(0.0, 100.0, 0.0);
        }
        for _ in 0..60 * 4 + 6 {
            server.update_conquest(1.0 / 60.0);
        }
        assert_eq!(server.team_scores, [CONQUEST_TICKETS - 3, CONQUEST_TICKETS]);
        // Out of tickets: the match is over.
        server.team_scores[0] = 1;
        server.spawn_player(1000);
        server.damage(1000, 1001, 500, 0);
        assert!(server.phase == Phase::Lobby);
    }

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
    fn in_conquest_the_dead_wait_the_whole_delay() {
        let mut server = conquest_with(0, &[0]);
        server.damage(1000, 1000, 1000, KNIFE);
        for _ in 0..60 * 2 {
            server.update_match(1.0 / 60.0);
        }
        server.handle_message(1000, ClientMsg::Respawn);
        for _ in 0..6 {
            server.update_match(1.0 / 60.0);
        }
        assert!(!server.players[&1000].alive, "asking to respawn skipped the wait");
        for _ in 0..(RESPAWN_DELAY * 60.0) as usize {
            server.update_match(1.0 / 60.0);
        }
        assert!(server.players[&1000].alive, "not respawned after the delay");
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

    /// A match on the course, in `mode`, with `people` in it and the
    /// countdown over.
    fn course_match(mode: u8, people: usize) -> (Server, Vec<PlayerId>) {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode,
            map: map::GAUNTLET,
            bots: 3,
            ..ServerSettings::default()
        })
        .unwrap();
        let ids = (0..people as PlayerId)
            .map(|i| {
                server.join_counter += 1;
                let person = server.new_player(format!("Racer {i}"), None);
                server.players.insert(1000 + i, person);
                1000 + i
            })
            .collect();
        server.start_match();
        server.countdown = 0.0;
        (server, ids)
    }

    fn gates(server: &Server) -> u8 {
        server.map.course.as_ref().unwrap().gates.len() as u8
    }

    /// Through every checkpoint, the first at `from` seconds, one a second.
    fn through_the_gates(server: &mut Server, id: PlayerId, from: f32) {
        for gate in 0..gates(server) {
            server.handle_message(id, ClientMsg::Run(RunEvent::Gate { gate, time: from + gate as f32 }));
        }
    }

    fn run(server: &mut Server, id: PlayerId, event: RunEvent) {
        server.handle_message(id, ClientMsg::Run(event));
    }

    #[test]
    fn nobody_races_bots_or_carries_a_gun() {
        let (mut server, ids) = course_match(MODE_RACE, 2);
        assert_eq!(server.players.len(), 2, "the bots asked for came in");
        server.handle_message(ids[0], ClientMsg::AddBot);
        assert_eq!(server.players.len(), 2);
        let eye = server.players[&ids[0]].pos;
        server.handle_message(ids[0], ClientMsg::Fire { weapon: 0, origin: eye.to_array(), dirs: vec![[0.0, 0.0, -1.0]] });
        server.handle_message(ids[0], ClientMsg::Hit { victim: ids[1], weapon: 0, headshot: true });
        assert_eq!(server.players[&ids[1]].health, MAX_HEALTH);
    }

    #[test]
    fn picking_a_racing_mode_sends_the_bots_home_and_goes_to_the_course() {
        let mut server = Server::bind(ServerSettings { port: 0, bots: 3, ..ServerSettings::default() }).unwrap();
        server.join_counter += 1;
        let person = server.new_player("Leader".into(), None);
        server.players.insert(1000, person);
        server.handle_message(1000, ClientMsg::SetMode(MODE_TRIAL));
        assert_eq!(server.settings.map, map::GAUNTLET);
        assert!(server.map.course.is_some());
        assert_eq!(server.players.len(), 1);
        // And back: to a map with a fight on it.
        server.handle_message(1000, ClientMsg::SetMode(MODE_FFA));
        assert_ne!(server.settings.map, map::GAUNTLET);
        server.handle_message(1000, ClientMsg::SetMap(map::GAUNTLET));
        assert_ne!(server.settings.map, map::GAUNTLET);
    }

    #[test]
    fn a_race_takes_checkpoints_in_order_and_the_first_through_wins() {
        let (mut server, ids) = course_match(MODE_RACE, 3);
        server.elapsed = 40.0;
        // Out of order, or the finish before the checkpoints: not taken.
        run(&mut server, ids[0], RunEvent::Gate { gate: 1, time: 5.0 });
        run(&mut server, ids[0], RunEvent::Finished { time: 9.0 });
        assert_eq!((server.players[&ids[0]].run_passed, server.players[&ids[0]].run_time), (0, None));
        through_the_gates(&mut server, ids[0], 5.0);
        assert_eq!(server.players[&ids[0]].run_passed, gates(&server) as usize);
        // Longer than the race has been on: not taken.
        run(&mut server, ids[0], RunEvent::Finished { time: 99.0 });
        assert_eq!(server.players[&ids[0]].run_time, None);
        run(&mut server, ids[0], RunEvent::Finished { time: 30.0 });
        assert_eq!(server.players[&ids[0]].run_time, Some(30.0));
        assert!(server.time_left <= RACE_FINISH_GRACE);
        // Only one finish each.
        run(&mut server, ids[0], RunEvent::Finished { time: 20.0 });
        assert_eq!(server.players[&ids[0]].run_time, Some(30.0));
        // A slower one; the race goes on for the last.
        through_the_gates(&mut server, ids[1], 6.0);
        run(&mut server, ids[1], RunEvent::Finished { time: 33.0 });
        assert!(server.time_left > RACE_END_DELAY);
        assert_eq!(server.time_up_winner(), Winner::Player(ids[0]));
        // Everybody through: it ends soon after.
        through_the_gates(&mut server, ids[2], 7.0);
        run(&mut server, ids[2], RunEvent::Finished { time: 35.0 });
        assert!(server.time_left <= RACE_END_DELAY);
        server.update_match(RACE_END_DELAY + 0.1);
        assert!(server.phase == Phase::Lobby);
    }

    #[test]
    fn a_time_trial_keeps_everybodys_best_run() {
        let (mut server, ids) = course_match(MODE_TRIAL, 2);
        server.elapsed = 300.0;
        // Not on a run: nothing counts.
        run(&mut server, ids[0], RunEvent::Gate { gate: 0, time: 3.0 });
        assert_eq!(server.players[&ids[0]].run_passed, 0);
        for (time, best) in [(40.0, 40.0), (45.0, 40.0), (38.0, 38.0)] {
            run(&mut server, ids[0], RunEvent::Started);
            through_the_gates(&mut server, ids[0], 5.0);
            run(&mut server, ids[0], RunEvent::Finished { time });
            let player = &server.players[&ids[0]];
            assert_eq!((player.run_time, player.run_on, player.run_passed), (Some(best), false, 0));
        }
        // A run given up counts for nothing.
        run(&mut server, ids[1], RunEvent::Started);
        run(&mut server, ids[1], RunEvent::Gate { gate: 0, time: 3.0 });
        run(&mut server, ids[1], RunEvent::Abandoned);
        run(&mut server, ids[1], RunEvent::Gate { gate: 1, time: 4.0 });
        assert_eq!((server.players[&ids[1]].run_passed, server.players[&ids[1]].run_time), (0, None));
        // A finish does not run the clock down: there is no clock.
        assert_eq!(server.time_left, f32::INFINITY);
        // The leader ends it, and the best time wins.
        assert_eq!(server.time_up_winner(), Winner::Player(ids[0]));
        let leader = server.leader().unwrap();
        server.handle_message(leader, ClientMsg::EndMatch);
        assert!(server.phase == Phase::Lobby);
    }
}
