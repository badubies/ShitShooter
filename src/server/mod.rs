//! The game server. It is plain Rust with no ECS: it runs on its own thread
//! inside the host's game process, or on the main thread with `--server`.

use std::{
    any::Any,
    collections::HashMap,
    io,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use bevy::math::{EulerRot, Quat, Vec2, Vec3};
use renet::{ConnectionConfig, RenetServer, ServerEvent};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};

use crate::{
    map::{self, Map},
    modes::{self, BotBrief, Kill, Needs, Rules, conquest::rules::FlagState},
    nav::NavGrid,
    canvas::Canvas,
    protocol::*,
    weapons::{self, DRAW_TIME, GUN_COUNT, IRON_SIGHTS, KNIFE, KNIFE_RANGE, SightId, WEAPONS, WeaponId, hit_damage, weapon},
};

mod bots;
mod combat;
mod game;
mod grenades;
mod spawn;
#[cfg(test)]
pub(crate) mod testing;

use bots::*;
use combat::*;
use grenades::*;

pub(crate) const TICK: Duration = Duration::from_micros(1_000_000 / 60);
/// Damage changes with every hit, so the roster goes out for it at most this
/// often (seconds) instead of every tick of a fight.
pub(crate) const DAMAGE_ROSTER_INTERVAL: f32 = 0.5;

#[derive(Clone)]
pub struct ServerSettings {
    pub port: u16,
    pub password: String,
    pub frag_limit: u32,
    pub match_minutes: f32,
    pub bots: usize,
    /// Index into `map::MAP_NAMES`.
    pub map: u8,
    /// One of the `MODE_*` constants.
    pub mode: u8,
    /// See `AIR_ACCEL_CHOICES`.
    pub air_accel: u16,
    /// The bots never shoot (see `ClientMsg::SetBotsHoldFire`).
    pub bots_hold_fire: bool,
    /// How well the bots fight: an index into `BOT_DIFFICULTY_NAMES`.
    pub bot_difficulty: u8,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            password: String::new(),
            frag_limit: DEFAULT_FRAG_LIMIT,
            match_minutes: DEFAULT_MATCH_MINUTES,
            bots: 0,
            map: 0,
            mode: MODE_FFA,
            air_accel: DEFAULT_AIR_ACCEL,
            bots_hold_fire: false,
            bot_difficulty: BOT_NORMAL,
        }
    }
}

/// How a lobby is doing, for the lobby list of a lobby server (`hub.rs`).
/// The lobby's server keeps it up to date every tick.
#[derive(Clone, Default, Debug)]
pub struct LobbyStatus {
    /// People in it (bots not counted).
    pub humans: u8,
    pub bots: u8,
    pub map: u8,
    pub mode: u8,
    /// A match is being played.
    pub playing: bool,
    /// The leader's name; empty while nobody is in.
    pub leader: String,
}

pub(crate) struct Player {
    pub(crate) name: String,
    pub(crate) color: u8,
    pub(crate) team: u8,
    pub(crate) kills: u32,
    pub(crate) deaths: u32,
    /// Kills somebody else finished after this player hurt the victim.
    pub(crate) assists: u32,
    /// Health and armor taken off other players this match. Overkill and
    /// hurting yourself do not count.
    pub(crate) damage: u32,
    /// Who has hurt them this life, and how many seconds ago they last did.
    /// When they die, each but the killer who hurt them within
    /// `ASSIST_WINDOW` gets an assist, healed since or not.
    pub(crate) hurt_by: Vec<(PlayerId, f32)>,
    pub(crate) join_order: u64,
    pub(crate) alive: bool,
    pub(crate) health: i32,
    pub(crate) armor: i32,
    pub(crate) pos: Vec3,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// `FLAG_*` bits reported by the client.
    pub(crate) flags: u8,
    /// The weapon in hand, and the sight on it, as reported by the client.
    pub(crate) weapon: WeaponId,
    pub(crate) sight: SightId,
    pub(crate) respawn_in: f32,
    pub(crate) spawn_seq: u32,
    /// Seconds of firing this player has saved up. Every shot costs most of
    /// the weapon's interval, so nobody can fire faster than their gun allows,
    /// yet two shots arriving in one tick (network jitter) are both accepted.
    pub(crate) fire_credit: f32,
    /// Bullets fired that have not hit anything yet. A hit is only accepted
    /// while this is above zero.
    pub(crate) bullets_out: u32,
    /// A knife swing was announced and has not been used for a hit yet.
    pub(crate) stab_ready: bool,
    pub(crate) since_damage: f32,
    /// Fraction of a health point regenerated so far.
    pub(crate) regen: f32,
    /// The grenade carried this life, and how many of it, counted the same
    /// way the client counts them.
    pub(crate) grenade: GrenadeKind,
    pub(crate) grenades: f32,
    /// The grenade picked in their loadout: carried from the next spawn on.
    pub(crate) loadout_grenade: GrenadeKind,
    /// Seconds left on the fuse of the frag with its pin pulled, if any.
    pub(crate) cooking: Option<f32>,
    /// Their paint job, encoded, once they have sent one.
    pub(crate) paint: Option<Vec<u8>>,
    /// Conquest: where they asked to come back in (see `ClientMsg::SpawnAt`).
    pub(crate) spawn_at: Option<u8>,
    /// For their footsteps: where they were at the last sample, seconds
    /// since, and metres run since the last step.
    pub(crate) step_from: Vec3,
    pub(crate) step_clock: f32,
    pub(crate) stride: f32,
    /// What they are in a mode where players are not all alike (see `PlayerInfo::role`).
    pub(crate) role: u8,
    /// What they were given this life: the rules decide it at each spawn.
    pub(crate) kit: Kit,
    /// Out of play without having died (see `Server::take_out`).
    pub(crate) away: bool,
    pub(crate) bot: Option<Bot>,
}

#[derive(PartialEq)]
pub(crate) enum Phase {
    Lobby,
    Playing,
}

pub struct Server {
    pub(crate) renet: RenetServer,
    pub(crate) transport: NetcodeServerTransport,
    pub(crate) settings: ServerSettings,
    pub(crate) map: Map,
    pub(crate) players: HashMap<PlayerId, Player>,
    pub(crate) phase: Phase,
    /// Seconds of the starting countdown left; 0 once the match is under way.
    pub(crate) countdown: f32,
    pub(crate) time_left: f32,
    /// Seconds played since the countdown ended: a race's clock.
    pub(crate) elapsed: f32,
    pub(crate) join_counter: u64,
    pub(crate) next_bot_id: PlayerId,
    pub(crate) snapshot_timer: f32,
    pub(crate) roster_dirty: bool,
    /// Somebody's damage went up since the roster last went out.
    pub(crate) damage_dirty: bool,
    /// Seconds until a damage change may send the roster again.
    pub(crate) damage_roster_in: f32,
    pub(crate) rng: u64,
    pub(crate) grenades: Vec<Grenade>,
    pub(crate) smokes: Vec<Smoke>,
    pub(crate) bullets: Vec<BotBullet>,
    /// Gunshots and footsteps of the last `NOISE_LIFETIME` seconds.
    pub(crate) noises: Vec<Noise>,
    pub(crate) next_noise_id: u64,
    pub(crate) next_grenade_id: u32,
    /// Time not yet consumed by whole grenade steps.
    pub(crate) grenade_clock: f32,
    /// Team modes: kills (team deathmatch), seconds on the hill, or tickets
    /// left (conquest).
    pub(crate) team_scores: [u32; 2],
    /// What the mode being played keeps of the match (who holds the hill,
    /// the flags): its `Rules::start` puts it here, and its other rules read
    /// it back as the type they know it to be. `()` for a mode that keeps
    /// nothing.
    pub(crate) state: Box<dyn Any + Send>,
    /// Seconds until each of the map's armor pickups is back; 0 = it is there.
    pub(crate) armor_respawn: Vec<f32>,
    /// Where bots can walk, on this map.
    pub(crate) nav: NavGrid,
    /// How far apart fights on this map in this mode tend to be (metres):
    /// what bots pick their guns for.
    pub(crate) bot_range: f32,
    /// Where to report how the lobby is doing, when run by a lobby server.
    pub(crate) status: Option<Arc<Mutex<LobbyStatus>>>,
}

impl Server {
    /// Binds the UDP socket. Fails if the port is already in use.
    pub fn bind(settings: ServerSettings) -> io::Result<Server> {
        let key = protocol_id(&settings.password);
        Server::bind_with_key(settings, key)
    }

    /// Like `bind`, for a lobby whose password is known only as the
    /// `protocol_id` it makes (`settings.password` is then ignored).
    pub fn bind_with_key(mut settings: ServerSettings, protocol_key: u64) -> io::Result<Server> {
        // A map that is not played in the mode asked for (`--map` checks
        // this): its first mode instead.
        if !map::plays(settings.map, settings.mode) {
            settings.mode = (0..modes::count()).find(|mode| map::plays(settings.map, *mode)).unwrap_or(MODE_FFA);
        }
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, settings.port))?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let config = ServerConfig {
            current_time: now,
            max_clients: MAX_PLAYERS,
            protocol_id: protocol_key,
            public_addresses: vec![SocketAddr::from((Ipv4Addr::LOCALHOST, settings.port))],
            authentication: ServerAuthentication::Unsecure,
        };
        let transport = NetcodeServerTransport::new(config, socket)?;
        let map = Map::for_game(settings.map, settings.mode);
        let mut server = Server {
            renet: RenetServer::new(ConnectionConfig::default()),
            transport,
            nav: nav_for(&map, settings.mode),
            bot_range: typical_range(&map, settings.mode),
            map,
            state: Box::new(()),
            grenades: Vec::new(),
            smokes: Vec::new(),
            bullets: Vec::new(),
            noises: Vec::new(),
            next_noise_id: 1,
            next_grenade_id: 0,
            grenade_clock: 0.0,
            team_scores: [0, 0],
            armor_respawn: Vec::new(),
            players: HashMap::new(),
            phase: Phase::Lobby,
            countdown: 0.0,
            time_left: 0.0,
            elapsed: 0.0,
            join_counter: 0,
            next_bot_id: 1,
            snapshot_timer: 0.0,
            roster_dirty: false,
            damage_dirty: false,
            damage_roster_in: 0.0,
            rng: now.as_nanos() as u64 | 1,
            settings,
            status: None,
        };
        for _ in 0..server.settings.bots {
            server.add_bot();
        }
        Ok(server)
    }

    /// Keeps `status` up to date from now on.
    pub fn report_to(&mut self, status: Arc<Mutex<LobbyStatus>>) {
        self.status = Some(status);
        self.report_status();
    }

    pub(crate) fn report_status(&self) {
        let Some(status) = &self.status else { return };
        let Ok(mut status) = status.lock() else { return };
        let humans = self.players.values().filter(|p| p.bot.is_none()).count();
        status.humans = humans as u8;
        status.bots = (self.players.len() - humans) as u8;
        status.map = self.settings.map;
        status.mode = self.settings.mode;
        status.playing = self.phase == Phase::Playing;
        let leader = self.leader().and_then(|id| self.players.get(&id)).map_or("", |p| p.name.as_str());
        if status.leader != leader {
            status.leader = leader.to_string();
        }
    }

    /// A line in the log, marked with the port, as a lobby server runs several.
    pub(crate) fn log(&self, line: std::fmt::Arguments) {
        println!("[server :{}] {line}", self.settings.port);
    }

    /// Runs the server on a background thread until the returned handle is dropped.
    pub fn spawn_thread(self) -> ServerHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("game-server".into())
            .spawn(move || self.run(&thread_stop))
            .expect("failed to spawn the server thread");
        ServerHandle {
            stop,
            thread: Some(thread),
        }
    }

    /// Runs the server loop until `stop` is set.
    pub fn run(mut self, stop: &AtomicBool) {
        let mut last = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            let dt = now - last;
            last = now;
            self.tick(dt);
            thread::sleep(TICK.saturating_sub(now.elapsed()));
        }
        self.transport.disconnect_all(&mut self.renet);
    }

    pub(crate) fn tick(&mut self, delta: Duration) {
        let dt = delta.as_secs_f32().min(0.25);
        self.renet.update(delta);
        if let Err(err) = self.transport.update(delta, &mut self.renet) {
            self.log(format_args!("transport error: {err}"));
        }

        while let Some(event) = self.renet.get_event() {
            match event {
                ServerEvent::ClientConnected { client_id } => {
                    self.log(format_args!("client {client_id:016x} connected"));
                }
                ServerEvent::ClientDisconnected { client_id, reason } => {
                    if let Some(player) = self.players.remove(&client_id) {
                        self.log(format_args!("{} left ({reason})", player.name));
                        self.roster_dirty = true;
                    }
                    // Nobody left to end a time trial, which has no time limit.
                    if !modes::info(self.settings.mode).timed && self.phase == Phase::Playing && self.players.is_empty() {
                        self.end_match(Winner::Nobody);
                    }
                }
            }
        }

        for client_id in self.renet.clients_id() {
            while let Some(bytes) = self.renet.receive_message(client_id, CH_RELIABLE) {
                match decode::<ClientMsg>(&bytes) {
                    Some(msg) => self.handle_message(client_id, msg),
                    None => self.log(format_args!("bad message from {client_id:016x}")),
                }
            }
            let mut latest = None;
            while let Some(bytes) = self.renet.receive_message(client_id, CH_UNRELIABLE) {
                latest = decode::<ClientState>(&bytes).or(latest);
            }
            if let (Some(state), Some(player)) = (latest, self.players.get_mut(&client_id))
                && player.alive
                && state.spawn_seq == player.spawn_seq
                && state.pos.iter().all(|v| v.is_finite())
            {
                player.pos = Vec3::from(state.pos);
                player.yaw = state.yaw;
                player.pitch = state.pitch;
                player.flags = state.flags & CLIENT_FLAGS;
                player.weapon = state.weapon;
                player.sight = state.sight;
            }
        }

        // With nobody left to play, fall back to the lobby (drops leftover bots' match).
        if self.phase == Phase::Playing && self.players.values().all(|p| p.bot.is_some()) {
            self.end_match(Winner::Nobody);
        }

        if self.phase == Phase::Playing {
            self.update_match(dt);
        }

        self.damage_roster_in -= dt;
        if self.damage_dirty && self.damage_roster_in <= 0.0 {
            self.roster_dirty = true;
        }
        if self.roster_dirty {
            self.roster_dirty = false;
            self.damage_dirty = false;
            self.damage_roster_in = DAMAGE_ROSTER_INTERVAL;
            let roster = self.roster_msg();
            self.broadcast(&roster);
        }
        self.transport.send_packets(&mut self.renet);
        self.report_status();
    }

    pub(crate) fn handle_message(&mut self, id: PlayerId, msg: ClientMsg) {
        let is_leader = self.leader() == Some(id);
        match msg {
            ClientMsg::Hello { name } => {
                if self.players.contains_key(&id) {
                    return;
                }
                let name = sanitize_name(&name);
                self.log(format_args!("{name} joined"));
                self.join_counter += 1;
                let player = self.new_player(name, None);
                self.players.insert(id, player);
                self.send(id, &ServerMsg::Welcome { id, maps: crate::custom::list() });
                self.send_map_file(Some(id));
                // Roster first, so the newcomer knows everyone before any spawn arrives.
                let roster = self.roster_msg();
                self.broadcast(&roster);
                let paints: Vec<ServerMsg> = self
                    .players
                    .iter()
                    .filter_map(|(other, p)| {
                        let paint = p.paint.clone()?;
                        Some(ServerMsg::Paint { id: *other, paint })
                    })
                    .collect();
                for paint in &paints {
                    self.send(id, paint);
                }
                if self.phase == Phase::Playing {
                    self.send(
                        id,
                        &ServerMsg::MatchStarted {
                            score_limit: score_limit(self.settings.mode, self.settings.frag_limit),
                            time_left: self.time_left,
                            countdown: self.countdown,
                            elapsed: self.elapsed,
                        },
                    );
                    (self.rules().joined)(self, id);
                    // Where the dead stay out, so does whoever comes late: until
                    // the mode puts everybody back in.
                    if modes::info(self.settings.mode).respawns {
                        self.spawn_player(id);
                    } else {
                        self.roster_dirty = true;
                    }
                }
            }
            ClientMsg::Loadout { grenade } => {
                if let Some(player) = self.players.get_mut(&id) {
                    player.loadout_grenade = grenade % GRENADE_KINDS as GrenadeKind;
                }
            }
            ClientMsg::Paint { paint } => {
                let Some(player) = self.players.get_mut(&id) else {
                    return;
                };
                if Canvas::decode(&paint).is_none() || player.paint.as_ref() == Some(&paint) {
                    return;
                }
                player.paint = Some(paint.clone());
                self.broadcast(&ServerMsg::Paint { id, paint });
            }
            ClientMsg::StartMatch if is_leader && self.phase == Phase::Lobby => self.start_match(),
            ClientMsg::EndMatch if is_leader && self.phase == Phase::Playing => {
                // A match with no time limit only ends this way: whoever leads wins it.
                let winner = if modes::info(self.settings.mode).timed { Winner::Nobody } else { (self.rules().time_up)(self) };
                self.end_match(winner);
            }
            ClientMsg::NextMode if is_leader && self.phase == Phase::Lobby => {
                // The next mode the map is played in.
                let modes = modes::count();
                let next = (1..=modes)
                    .map(|step| (self.settings.mode + step) % modes)
                    .find(|mode| map::plays(self.settings.map, *mode))
                    .unwrap_or(self.settings.mode);
                self.set_mode(next);
            }
            ClientMsg::SetMode(mode) if is_leader && self.phase == Phase::Lobby => self.set_mode(mode),
            ClientMsg::NextAirAccel if is_leader && self.phase == Phase::Lobby => {
                self.settings.air_accel = next_air_accel(self.settings.air_accel);
                self.roster_dirty = true;
            }
            ClientMsg::SetAirAccel(air_accel)
                if is_leader && self.phase == Phase::Lobby && AIR_ACCEL_CHOICES.contains(&air_accel) =>
            {
                self.settings.air_accel = air_accel;
                self.roster_dirty = true;
            }
            ClientMsg::SetBotsHoldFire(hold) if is_leader && self.phase == Phase::Lobby => {
                self.settings.bots_hold_fire = hold;
                self.roster_dirty = true;
            }
            ClientMsg::SetBotDifficulty(difficulty)
                if is_leader && self.phase == Phase::Lobby && (difficulty as usize) < BOT_DIFFICULTY_NAMES.len() =>
            {
                self.settings.bot_difficulty = difficulty;
                self.roster_dirty = true;
            }
            ClientMsg::SwitchTeam if self.phase == Phase::Lobby => {
                // Not onto a team that is already full.
                let Some(to) = self.players.get(&id).map(|player| player.team ^ 1) else {
                    return;
                };
                if self.players.values().filter(|player| player.team == to).count() >= TEAM_SIZE {
                    return;
                }
                if let Some(player) = self.players.get_mut(&id) {
                    player.team = to;
                    self.roster_dirty = true;
                }
            }
            ClientMsg::AddBot if is_leader => self.add_bot(),
            ClientMsg::RemoveBot if is_leader => {
                // Bots' ids count up as they join.
                let last = self.players.iter().filter(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).max();
                if let Some(last) = last {
                    self.players.remove(&last);
                    self.roster_dirty = true;
                }
            }
            ClientMsg::NextMap if is_leader && self.phase == Phase::Lobby => {
                // The next map played in this mode.
                let maps = map::count();
                let next = (1..=maps)
                    .map(|step| (self.settings.map + step) % maps)
                    .find(|id| map::plays(*id, self.settings.mode))
                    .unwrap_or(self.settings.map);
                self.set_map(next);
            }
            ClientMsg::SetMap(id) if is_leader && self.phase == Phase::Lobby => self.set_map(id),
            ClientMsg::Fire { weapon, origin, dirs } => self.handle_fire(id, weapon, origin, dirs),
            ClientMsg::Hit {
                victim,
                weapon,
                headshot,
            } => self.handle_hit(id, victim, weapon, headshot),
            ClientMsg::Cook => self.handle_cook(id),
            ClientMsg::Grenade { kind, origin, vel } => self.handle_grenade(id, kind, origin, vel),
            ClientMsg::Ping { target, pos } => self.handle_ping(id, target, pos),
            ClientMsg::SpawnAt { point } => {
                if let Some(player) = self.players.get_mut(&id) {
                    player.spawn_at = point;
                }
            }
            ClientMsg::Run(event) if self.live() => (self.rules().run)(self, id, event),
            ClientMsg::Respawn if self.phase == Phase::Playing && modes::info(self.settings.mode).early_respawn => {
                if let Some(player) = self.players.get_mut(&id)
                    && !player.alive
                    && RESPAWN_DELAY - player.respawn_in >= MIN_RESPAWN_DELAY
                {
                    // `update_match` puts them in on its next tick.
                    player.respawn_in = 0.0;
                }
            }
            _ => {}
        }
    }

    /// Passes a ping on to the pinger's teammates, and nobody else. A ping
    /// on somebody who is not a living enemy becomes a ping on the spot.
    pub(crate) fn handle_ping(&mut self, from: PlayerId, target: Option<PlayerId>, pos: [f32; 3]) {
        let team_mode = is_team_mode(self.settings.mode);
        // Free for all: nobody is on your side, so nobody is told.
        if !team_mode || self.phase != Phase::Playing || !Vec3::from(pos).is_finite() {
            return;
        }
        let Some(pinger) = self.players.get_mut(&from) else {
            return;
        };
        if !pinger.alive {
            return;
        }
        let team = pinger.team;
        let target = target.filter(|id| {
            self.players
                .get(id)
                .is_some_and(|enemy| *id != from && enemy.alive && enemy.team != team)
        });
        let msg = encode(&ServerMsg::Ping { from, target, pos });
        for mate in self.teammates_of(from) {
            self.renet.send_message(mate, CH_RELIABLE, msg.clone());
        }
    }

    /// The people (not bots) on the same team as `id`, apart from `id`.
    pub(crate) fn teammates_of(&self, id: PlayerId) -> Vec<PlayerId> {
        let Some(team) = self.players.get(&id).map(|p| p.team) else {
            return Vec::new();
        };
        self.players
            .iter()
            .filter(|(other, p)| **other != id && p.team == team && p.bot.is_none())
            .map(|(other, _)| *other)
            .collect()
    }

    /// Plays `mode` from the next match on: on the map picked if it is played
    /// there, or else on the first map that it is.
    pub(crate) fn set_mode(&mut self, mode: u8) {
        if mode >= modes::count() {
            return;
        }
        // Nowhere to play it (a mode that needs a map made for it, and there is none).
        let maps = map::count();
        if !(0..maps).any(|id| map::plays(id, mode)) {
            return;
        }
        self.settings.mode = mode;
        // Nobody races bots.
        if !modes::info(mode).bots {
            self.players.retain(|_, player| player.bot.is_none());
        }
        if !map::plays(self.settings.map, mode) {
            if let Some(id) = (0..maps).find(|id| map::plays(*id, mode)) {
                self.settings.map = id;
            }
            self.rebuild_map();
        } else if self.map.has_flagpoles() != (modes::info(mode).needs == Needs::Flags) {
            // Into or out of conquest: the flagpoles go up or come down.
            self.rebuild_map();
        }
        self.roster_dirty = true;
    }

    /// Plays on map `id` from the next match on, if the mode is played there.
    pub(crate) fn set_map(&mut self, id: u8) {
        if id >= map::count() || !map::plays(id, self.settings.mode) || id == self.settings.map {
            return;
        }
        self.settings.map = id;
        self.rebuild_map();
        self.roster_dirty = true;
    }

    pub(crate) fn rebuild_map(&mut self) {
        self.send_map_file(None);
        self.map = Map::for_game(self.settings.map, self.settings.mode);
        self.nav = nav_for(&self.map, self.settings.mode);
    }

    /// The map being played's file, to `to` (everybody, for `None`), if it
    /// is one made in Blender: they build it from that.
    pub(crate) fn send_map_file(&mut self, to: Option<PlayerId>) {
        let map = self.settings.map;
        let Some(data) = map.checked_sub(map::BUILT_IN).and_then(|index| crate::custom::bytes(index as usize)) else { return };
        let msg = ServerMsg::MapFile { map, data: data.to_vec() };
        match to {
            Some(id) => self.send(id, &msg),
            None => self.broadcast(&msg),
        }
    }

    /// A map made in Blender is read again from its file at the start of
    /// each match: export it again and the next match is on the new one.
    pub(crate) fn reload_map_file(&mut self) {
        let Some(index) = self.settings.map.checked_sub(map::BUILT_IN) else { return };
        match crate::custom::reload(index as usize) {
            Ok(true) => {
                self.log(format_args!("{} changed: reloaded", map::name(self.settings.map)));
                self.rebuild_map();
                self.roster_dirty = true;
            }
            Ok(false) => {}
            Err(why) => self.log(format_args!("{why}; playing the last good one")),
        }
    }

    pub(crate) fn new_player(&self, name: String, bot: Option<Bot>) -> Player {
        // Pick the least used colour so players are easy to tell apart.
        let color = (0..PLAYER_COLOR_COUNT)
            .min_by_key(|c| self.players.values().filter(|p| p.color == *c).count())
            .unwrap_or(0);
        // And the smaller team.
        let ones = self.players.values().filter(|p| p.team == 1).count();
        let team = (ones * 2 < self.players.len()) as u8;
        Player {
            name,
            color,
            team,
            kills: 0,
            deaths: 0,
            assists: 0,
            damage: 0,
            hurt_by: Vec::new(),
            join_order: self.join_counter,
            alive: false,
            health: 0,
            armor: 0,
            pos: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            flags: 0,
            weapon: 0,
            sight: IRON_SIGHTS,
            respawn_in: 0.0,
            spawn_seq: 0,
            fire_credit: 0.0,
            bullets_out: 0,
            stab_ready: false,
            since_damage: 0.0,
            regen: 0.0,
            grenade: FRAG,
            grenades: 0.0,
            loadout_grenade: FRAG,
            cooking: None,
            paint: None,
            spawn_at: None,
            step_from: Vec3::ZERO,
            step_clock: 0.0,
            stride: 0.0,
            role: 0,
            kit: Kit::standard(),
            away: false,
            bot,
        }
    }

    /// The lobby leader is the human who has been connected the longest.
    pub(crate) fn leader(&self) -> Option<PlayerId> {
        self.players
            .iter()
            .filter(|(_, p)| p.bot.is_none())
            .min_by_key(|(_, p)| p.join_order)
            .map(|(id, _)| *id)
    }

    pub(crate) fn roster_msg(&self) -> ServerMsg {
        let mut players: Vec<(&PlayerId, &Player)> = self.players.iter().collect();
        players.sort_by_key(|(_, p)| p.join_order);
        ServerMsg::Roster {
            players: players
                .into_iter()
                .map(|(id, p)| PlayerInfo {
                    id: *id,
                    name: p.name.clone(),
                    color: p.color,
                    team: p.team,
                    kills: p.kills,
                    deaths: p.deaths,
                    assists: p.assists,
                    damage: p.damage,
                    is_bot: p.bot.is_some(),
                    role: p.role,
                    stats: (self.rules().stats)(self, *id),
                })
                .collect(),
            leader: self.leader().unwrap_or(0),
            map: self.settings.map,
            mode: self.settings.mode,
            air_accel: self.settings.air_accel,
            bots_hold_fire: self.settings.bots_hold_fire,
            bot_difficulty: self.settings.bot_difficulty,
        }
    }

    pub(crate) fn send(&mut self, id: PlayerId, msg: &ServerMsg) {
        self.renet.send_message(id, CH_RELIABLE, encode(msg));
    }

    pub(crate) fn broadcast(&mut self, msg: &ServerMsg) {
        self.renet.broadcast_message(CH_RELIABLE, encode(msg));
    }

    pub(crate) fn random(&mut self) -> u64 {
        next_random(&mut self.rng)
    }
}

/// Keeps the hosted server alive; dropping it shuts the server down.
pub struct ServerHandle {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) thread: Option<thread::JoinHandle<()>>,
}

impl ServerHandle {
    /// The server's thread has ended on its own: it crashed. Its last
    /// `LobbyStatus` stays as it was, so it must not be trusted any more.
    pub fn has_stopped(&self) -> bool {
        self.thread.as_ref().is_none_or(|thread| thread.is_finished())
    }

    /// A handle whose server crashed straight away, for tests.
    #[cfg(test)]
    pub fn crashed() -> ServerHandle {
        let thread = thread::spawn(|| panic!("test: the lobby's server crashed"));
        while !thread.is_finished() {
            thread::yield_now();
        }
        ServerHandle {
            stop: Arc::new(AtomicBool::new(false)),
            thread: Some(thread),
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// xorshift64
pub(crate) fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state >> 11
}

/// A random number in 0..1.
pub(crate) fn random_unit(state: &mut u64) -> f32 {
    (next_random(state) % (1 << 24)) as f32 / (1 << 24) as f32
}

pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(16)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        "Player".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leader_picks_the_air_acceleration_for_everyone() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        for id in [1, 2] {
            server.join_counter += 1;
            let player = server.new_player(format!("P{id}"), None);
            server.players.insert(id, player);
        }
        let leader = server.leader().unwrap();
        let other = if leader == 1 { 2 } else { 1 };
        server.handle_message(other, ClientMsg::NextAirAccel);
        assert_eq!(server.settings.air_accel, DEFAULT_AIR_ACCEL, "only the leader may");
        let mut seen = Vec::new();
        for _ in 0..AIR_ACCEL_CHOICES.len() {
            server.handle_message(leader, ClientMsg::NextAirAccel);
            seen.push(server.settings.air_accel);
        }
        assert_eq!(seen, [30, 100, 10, 20]);
        let ServerMsg::Roster { air_accel, .. } = server.roster_msg() else {
            panic!("not a roster");
        };
        assert_eq!(air_accel, DEFAULT_AIR_ACCEL);
    }

    /// A team never holds more than half the lobby: switching onto a full
    /// team is refused.
    #[test]
    fn nobody_switches_onto_a_full_team() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        for id in 1..=MAX_PLAYERS as PlayerId {
            server.join_counter += 1;
            let player = server.new_player(format!("P{id}"), None);
            server.players.insert(id, player);
        }
        let on = |server: &Server, team: u8| server.players.values().filter(|p| p.team == team).count();
        assert_eq!((on(&server, 0), on(&server, 1)), (TEAM_SIZE, TEAM_SIZE), "joins fill the smaller team");
        let team = server.players[&1].team;
        server.handle_message(1, ClientMsg::SwitchTeam);
        assert_eq!(server.players[&1].team, team, "the other team is full");
        // Once somebody leaves it, there is room.
        let gone = *server.players.iter().find(|(_, p)| p.team != team).unwrap().0;
        server.players.remove(&gone);
        server.handle_message(1, ClientMsg::SwitchTeam);
        assert_eq!(server.players[&1].team, team ^ 1);
    }

    /// Hamlet (map 3) is for free for all and team deathmatch only: changing
    /// the mode there skips the others, and changing the map in those other
    /// modes skips Hamlet.
    #[test]
    fn hamlet_is_only_offered_in_the_modes_it_is_played_in() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            map: 3,
            mode: MODE_CONQUEST,
            ..ServerSettings::default()
        })
        .unwrap();
        assert_eq!(server.settings.mode, MODE_FFA, "started in a mode Hamlet has");
        server.join_counter += 1;
        let player = server.new_player("P".into(), None);
        server.players.insert(1, player);
        let mut modes = Vec::new();
        for _ in 0..3 {
            server.handle_message(1, ClientMsg::NextMode);
            modes.push(server.settings.mode);
        }
        assert_eq!(modes, [MODE_TEAMS, MODE_FFA, MODE_TEAMS]);
        let mut maps = Vec::new();
        for _ in 0..5 {
            server.handle_message(1, ClientMsg::NextMap);
            maps.push(server.settings.map);
        }
        assert_eq!(maps, [4, 0, 1, 2, 3]);
        assert_eq!(server.map.id, 3);
        // In conquest, from Valley on to Arena (past Surf).
        server.handle_message(1, ClientMsg::NextMap);
        server.handle_message(1, ClientMsg::NextMap);
        assert_eq!(server.settings.map, 0);
        server.handle_message(1, ClientMsg::NextMode);
        server.handle_message(1, ClientMsg::NextMode);
        assert_eq!(server.settings.mode, MODE_CONQUEST);
        server.handle_message(1, ClientMsg::NextMap);
        server.handle_message(1, ClientMsg::NextMap);
        assert_eq!(server.settings.map, 2);
        server.handle_message(1, ClientMsg::NextMap);
        assert_eq!(server.settings.map, 0, "Hamlet and Surf skipped");
        assert!(server.map.has_flagpoles());
    }

    /// The lobby's chips: the leader picks a mode, map or air acceleration
    /// straight away. A mode Hamlet isn't played in moves the match off it,
    /// and Hamlet can't be picked then.
    #[test]
    fn the_leader_picks_mode_map_and_air_acceleration_directly() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            map: 3,
            ..ServerSettings::default()
        })
        .unwrap();
        for id in [1, 2] {
            server.join_counter += 1;
            let player = server.new_player(format!("P{id}"), None);
            server.players.insert(id, player);
        }
        let leader = server.leader().unwrap();
        let other = if leader == 1 { 2 } else { 1 };
        server.handle_message(other, ClientMsg::SetMode(MODE_CONQUEST));
        assert_eq!(server.settings.mode, MODE_FFA, "only the leader may");

        server.handle_message(leader, ClientMsg::SetMode(MODE_CONQUEST));
        assert_eq!(server.settings.mode, MODE_CONQUEST);
        assert_eq!(server.settings.map, 0, "off Hamlet, onto the first map conquest is played on");
        assert!(server.map.has_flagpoles());
        server.handle_message(leader, ClientMsg::SetMap(3));
        assert_eq!(server.settings.map, 0, "Hamlet has no conquest");
        server.handle_message(leader, ClientMsg::SetMap(2));
        assert_eq!(server.map.id, 2);

        server.handle_message(leader, ClientMsg::SetAirAccel(100));
        assert_eq!(server.settings.air_accel, 100);
        server.handle_message(leader, ClientMsg::SetAirAccel(55));
        assert_eq!(server.settings.air_accel, 100, "not one of the choices");
    }

    #[test]
    fn pings_go_to_teammates_only() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode: MODE_TEAMS,
            bots: 1,
            ..ServerSettings::default()
        })
        .unwrap();
        for (id, team) in [(1000, 0), (1001, 0), (1002, 1)] {
            server.join_counter += 1;
            let mut person = server.new_player(format!("P{id}"), None);
            person.team = team;
            server.players.insert(id, person);
        }
        for bot in server.players.values_mut().filter(|p| p.bot.is_some()) {
            bot.team = 0;
        }
        assert_eq!(server.teammates_of(1000), vec![1001]);
        assert_eq!(server.teammates_of(1002), Vec::<PlayerId>::new());
    }

    #[test]
    fn only_paint_jobs_that_read_back_are_kept() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        server.join_counter += 1;
        let person = server.new_player("Painter".into(), None);
        server.players.insert(1000, person);
        let good = Canvas::solid(4).encode();
        server.handle_message(1000, ClientMsg::Paint { paint: good.clone() });
        assert_eq!(server.players[&1000].paint.as_ref(), Some(&good));
        server.handle_message(1000, ClientMsg::Paint { paint: vec![1, 2, 3] });
        assert_eq!(server.players[&1000].paint.as_ref(), Some(&good), "a broken paint job replaced a good one");
        // Nobody who has not said hello can paint.
        server.handle_message(1001, ClientMsg::Paint { paint: good });
        assert!(!server.players.contains_key(&1001));
    }
}
