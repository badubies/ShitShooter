//! The game server. It is plain Rust with no ECS: it runs on its own thread
//! inside the host's game process, or on the main thread with `--server`.

use std::{
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
    nav::NavGrid,
    canvas::Canvas,
    protocol::*,
    weapons::{self, DRAW_TIME, GUN_COUNT, IRON_SIGHTS, KNIFE, KNIFE_RANGE, SightId, WEAPONS, WeaponId, hit_damage, weapon},
};

const TICK: Duration = Duration::from_micros(1_000_000 / 60);
const BOT_NAMES: [&str; 8] = [
    "Clanker", "Tinhead", "Bolt", "Rusty", "Sprocket", "Gizmo", "Widget", "Scrap",
];

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

// How well bots fight. They see enemies in front of them, or close by, or
// whoever just hurt them; take a moment to react; turn at a human-ish rate;
// lead their shots, though not perfectly; and their aim wobbles, more so
// right after they find a target and while it darts across their view.
/// Bots notice enemies within this angle either side of where they look
/// (cosine of the angle)...
const BOT_VIEW_COS: f32 = 0.26;
/// ...or anywhere this close.
const BOT_HEARING: f32 = 6.0;
/// They see as far as there is a clear line, but fights are reckoned to go
/// no further than this when bots pick their guns.
const LONG_RANGE: f32 = 70.0;
/// Bots hear gunshots (not the knife), grenades going off and footsteps
/// (see `NoiseKind::reach`), through walls and all, and go and see what it
/// was, guessing where it came from: off by up to the first of these many
/// radians either side, and the second of the distance nearer or further.
/// Loud bangs are easy to place; footsteps are not.
const SHOT_GUESS: (f32, f32) = (0.09, 0.15);
const STEP_GUESS: (f32, f32) = (0.52, 0.4);
/// Seconds a noise can be heard for after it is made.
const NOISE_LIFETIME: f32 = 0.5;
/// Footsteps the way the clients play them (see `audio.rs`): one every
/// `STRIDE` metres running on the ground, faster than `STEP_MIN_SPEED`, not
/// crouched. Worked out from how far everyone moved every `STEP_SAMPLE`
/// seconds.
const STRIDE: f32 = 2.6;
const STEP_MIN_SPEED: f32 = 2.0;
const STEP_SAMPLE: f32 = 0.25;
/// Seconds a bot spends going to see what it heard, the first of them
/// turned towards it.
const BOT_INVESTIGATE_TIME: f32 = 10.0;
const BOT_GLANCE_TIME: f32 = 1.5;
/// Conquest: bots leave their flag only for noises this close.
const BOT_INVESTIGATE_NEAR: f32 = 30.0;
/// Radians per second, turning to look where it is going.
const BOT_TURN_RATE: f32 = 6.0;
/// Beyond this many metres, a far body is small and hard to make out (see
/// `BotSkill::spot_time` and `BotSkill::far_doubling`).
const BOT_FAR: f32 = 20.0;
/// Seconds over which a bot works out how fast its enemy is going.
const BOT_VEL_SMOOTHING: f32 = 0.15;

/// How well bots fight at one of the `BOT_DIFFICULTY_NAMES`.
struct BotSkill {
    /// Seconds an enemy must be in sight before the first shot: at least
    /// `reaction`, and up to `reaction_jitter` more...
    reaction: f32,
    reaction_jitter: f32,
    /// ...and `spot_time` more for every metre beyond `BOT_FAR`.
    spot_time: f32,
    /// Radians per second, turning towards an enemy.
    turn_rate: f32,
    /// Radius of the aim wobble, in radians, once settled on a target,
    /// growing beyond `BOT_FAR` to twice as wide `far_doubling` metres on...
    aim_error: f32,
    far_doubling: f32,
    /// ...plus this many metres at any range...
    aim_slop: f32,
    /// ...plus `tracking` radians for every radian per second the enemy
    /// sweeps across its view beyond `easy_track` (one strafing up close is
    /// hard to follow; far away, easy).
    easy_track: f32,
    tracking: f32,
    /// Seconds it takes the wobble to settle down from its widest.
    settle: f32,
    /// How much of the enemy's movement during the bullet's flight a bot
    /// aims ahead by: somewhere in this range, rolled with each wobble.
    lead: (f32, f32),
}

/// Easy, normal and hard.
const _: () = assert!(BOT_EASY == 0 && BOT_NORMAL == 1 && BOT_HARD == 2);
const BOT_SKILLS: [BotSkill; BOT_DIFFICULTY_NAMES.len()] = [
    BotSkill {
        reaction: 0.3,
        reaction_jitter: 0.2,
        spot_time: 0.008,
        turn_rate: 6.0,
        aim_error: 0.015,
        far_doubling: 30.0,
        aim_slop: 0.15,
        easy_track: 0.3,
        tracking: 0.12,
        settle: 1.5,
        lead: (0.5, 1.2),
    },
    BotSkill {
        reaction: 0.22,
        reaction_jitter: 0.15,
        spot_time: 0.005,
        turn_rate: 7.5,
        aim_error: 0.011,
        far_doubling: 45.0,
        aim_slop: 0.1,
        easy_track: 0.45,
        tracking: 0.08,
        settle: 1.1,
        lead: (0.7, 1.1),
    },
    BotSkill {
        reaction: 0.15,
        reaction_jitter: 0.1,
        spot_time: 0.003,
        turn_rate: 9.0,
        aim_error: 0.008,
        far_doubling: 70.0,
        aim_slop: 0.06,
        easy_track: 0.6,
        tracking: 0.05,
        settle: 0.7,
        lead: (0.85, 1.05),
    },
];

fn bot_skill(difficulty: u8) -> &'static BotSkill {
    BOT_SKILLS.get(difficulty as usize).unwrap_or(&BOT_SKILLS[BOT_NORMAL as usize])
}
/// Seconds a bot stays turned towards whoever hurt it.
const BOT_GRUDGE: f32 = 3.0;
/// A bot that kills somebody within this many metres, and has nothing
/// better to do, goes over to the body (giving up after `BOT_GLOAT_TIME`
/// seconds) and crouches up and down on it `BOT_BOBS` times, `BOT_BOB`
/// seconds down and as long up.
const BOT_GLOAT_RANGE: f32 = 15.0;
const BOT_GLOAT_TIME: f32 = 6.0;
const BOT_BOBS: u32 = 5;
const BOT_BOB: f32 = 0.25;
/// Close enough to the body to start (metres along the ground).
const BOT_GLOAT_REACH: f32 = 0.8;
/// Bullets vanish after this many seconds, as they do on the clients.
const BULLET_LIFETIME: f32 = 1.5;

struct Bot {
    vel: Vec3,
    /// It was standing on something last tick.
    grounded: bool,
    /// The way to `goal`, waypoint by waypoint (empty: run straight at it),
    /// and the goal it was found for.
    path: Vec<Vec3>,
    path_to: Option<Vec3>,
    /// Seconds spent wanting to move but hardly moving.
    stuck: f32,
    /// Conquest: the flag it is going for, and seconds until it thinks again.
    objective: Option<usize>,
    rethink_in: f32,
    /// Where it is running to.
    goal: Vec3,
    retarget_in: f32,
    /// The gun in its hands, with its magazine; the other one it carries
    /// this life, with its own; and which of the two it picked to lead with.
    gun: WeaponId,
    ammo: u32,
    spare: WeaponId,
    spare_ammo: u32,
    primary: WeaponId,
    /// Seconds until it may change guns again.
    switch_in: f32,
    /// Seconds of reloading left; 0 when not reloading.
    reload: f32,
    /// Seconds until it may fire again.
    cooldown: f32,
    /// Shots left in the current burst (automatic guns).
    burst: u32,
    /// The enemy it is fighting, and for how long it has had them in sight.
    enemy: Option<PlayerId>,
    sight: f32,
    /// Seconds of `sight` it needs before shooting this enemy.
    reaction: f32,
    /// Where the enemy was last tick, and how fast it reckons they are going.
    enemy_at: Vec3,
    enemy_vel: Vec3,
    /// How far off its aim is (yaw, pitch), and how much it leads by,
    /// re-rolled every `wobble_in` seconds.
    aim_error: Vec2,
    lead: f32,
    wobble_in: f32,
    /// Whoever hurt it last, and for how much longer it cares.
    provoked_by: Option<PlayerId>,
    provoked_for: f32,
    /// Seconds of being blinded by a flashbang left.
    blind: f32,
    /// Where it reckons a noise it heard came from, and for how much
    /// longer it will go and look; and the last `Noise::id` it listened to.
    investigate: Option<Vec3>,
    investigate_for: f32,
    last_noise: u64,
    /// Where lies the body of somebody it just killed close by, for how much
    /// longer it will go there, and for how long it has been bobbing on it.
    gloat: Option<Vec3>,
    gloat_for: f32,
    bobbing: f32,
    /// On the short box (see `bot_crouch`).
    crouched: bool,
}

impl Bot {
    fn new() -> Bot {
        Bot {
            vel: Vec3::ZERO,
            grounded: false,
            path: Vec::new(),
            path_to: None,
            stuck: 0.0,
            objective: None,
            rethink_in: 0.0,
            goal: Vec3::ZERO,
            retarget_in: 0.0,
            gun: 0,
            ammo: 0,
            spare: weapons::PISTOL,
            spare_ammo: 0,
            primary: 0,
            switch_in: 0.0,
            reload: 0.0,
            cooldown: 0.0,
            burst: 0,
            enemy: None,
            sight: 0.0,
            reaction: 0.0,
            enemy_at: Vec3::ZERO,
            enemy_vel: Vec3::ZERO,
            aim_error: Vec2::ZERO,
            lead: 1.0,
            wobble_in: 0.0,
            provoked_by: None,
            provoked_for: 0.0,
            blind: 0.0,
            investigate: None,
            investigate_for: 0.0,
            last_noise: 0,
            gloat: None,
            gloat_for: 0.0,
            bobbing: 0.0,
            crouched: false,
        }
    }
}

/// Something for bots to hear.
struct Noise {
    /// Counts up, so a bot can tell which it has listened to already.
    id: u64,
    /// Who made it: the shooter, the runner, or whoever threw the grenade.
    source: PlayerId,
    team: u8,
    pos: Vec3,
    kind: NoiseKind,
    age: f32,
}

/// From quietest to loudest: a bot hearing several goes for the loudest.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
enum NoiseKind {
    Step,
    /// A smoke grenade popping.
    Pop,
    Shot,
    /// A frag or a flashbang going off.
    Bang,
}

impl NoiseKind {
    /// How far away it can be heard (metres).
    fn reach(self) -> f32 {
        match self {
            NoiseKind::Step => 16.0,
            NoiseKind::Pop => 40.0,
            NoiseKind::Shot => 100.0,
            NoiseKind::Bang => 150.0,
        }
    }

    /// How far off a guess of where it came from may be (see `SHOT_GUESS`).
    fn guess(self) -> (f32, f32) {
        match self {
            NoiseKind::Step => STEP_GUESS,
            _ => SHOT_GUESS,
        }
    }
}

/// A bullet fired by a bot. People's bullets are flown by their own clients;
/// bots have none, so the server flies theirs.
struct BotBullet {
    shooter: PlayerId,
    team: u8,
    weapon: WeaponId,
    pos: Vec3,
    vel: Vec3,
    age: f32,
}

/// What a bot needs to know about everyone else this tick.
struct Body {
    id: PlayerId,
    team: u8,
    center: Vec3,
}

/// A bot's shot: shooter, team, weapon, origin and one direction per bullet.
type BotShot = (PlayerId, u8, WeaponId, Vec3, Vec<Vec3>);

struct Player {
    name: String,
    color: u8,
    team: u8,
    kills: u32,
    deaths: u32,
    /// Kills somebody else finished after this player hurt the victim.
    assists: u32,
    /// Who has hurt them this life, and how many seconds ago they last did.
    /// When they die, each but the killer who hurt them within
    /// `ASSIST_WINDOW` gets an assist, healed since or not.
    hurt_by: Vec<(PlayerId, f32)>,
    join_order: u64,
    alive: bool,
    health: i32,
    armor: i32,
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    /// `FLAG_*` bits reported by the client.
    flags: u8,
    /// The weapon in hand, and the sight on it, as reported by the client.
    weapon: WeaponId,
    sight: SightId,
    respawn_in: f32,
    spawn_seq: u32,
    /// Seconds of firing this player has saved up. Every shot costs most of
    /// the weapon's interval, so nobody can fire faster than their gun allows,
    /// yet two shots arriving in one tick (network jitter) are both accepted.
    fire_credit: f32,
    /// Bullets fired that have not hit anything yet. A hit is only accepted
    /// while this is above zero.
    bullets_out: u32,
    /// A knife swing was announced and has not been used for a hit yet.
    stab_ready: bool,
    since_damage: f32,
    /// Fraction of a health point regenerated so far.
    regen: f32,
    /// The grenade carried this life, and how many of it, counted the same
    /// way the client counts them.
    grenade: GrenadeKind,
    grenades: f32,
    /// The grenade picked in their loadout: carried from the next spawn on.
    loadout_grenade: GrenadeKind,
    /// Seconds left on the fuse of the frag with its pin pulled, if any.
    cooking: Option<f32>,
    /// Their paint job, encoded, once they have sent one.
    paint: Option<Vec<u8>>,
    /// Conquest: where they asked to come back in (see `ClientMsg::SpawnAt`).
    spawn_at: Option<u8>,
    /// For their footsteps: where they were at the last sample, seconds
    /// since, and metres run since the last step.
    step_from: Vec3,
    step_clock: f32,
    stride: f32,
    bot: Option<Bot>,
}

const MAX_FIRE_CREDIT: f32 = 1.5;
/// Hurting somebody earns an assist if they die within this many seconds.
const ASSIST_WINDOW: f32 = 10.0;
const MAX_BULLETS_OUT: u32 = 40;

struct Grenade {
    id: u32,
    kind: GrenadeKind,
    owner: PlayerId,
    pos: Vec3,
    vel: Vec3,
    fuse: f32,
}

/// A smoke cloud: bots cannot see through it.
struct Smoke {
    center: Vec3,
    left: f32,
}

#[derive(PartialEq)]
enum Phase {
    Lobby,
    Playing,
}

pub struct Server {
    renet: RenetServer,
    transport: NetcodeServerTransport,
    settings: ServerSettings,
    map: Map,
    players: HashMap<PlayerId, Player>,
    phase: Phase,
    /// Seconds of the starting countdown left; 0 once the match is under way.
    countdown: f32,
    time_left: f32,
    join_counter: u64,
    next_bot_id: PlayerId,
    snapshot_timer: f32,
    roster_dirty: bool,
    rng: u64,
    grenades: Vec<Grenade>,
    smokes: Vec<Smoke>,
    bullets: Vec<BotBullet>,
    /// Gunshots and footsteps of the last `NOISE_LIFETIME` seconds.
    noises: Vec<Noise>,
    next_noise_id: u64,
    next_grenade_id: u32,
    /// Time not yet consumed by whole grenade steps.
    grenade_clock: f32,
    /// Team modes: kills (team deathmatch) or seconds on the hill.
    team_scores: [u32; 2],
    /// Who holds the hill right now (`HILL_*`).
    hill: u8,
    /// Time held that has not become a whole point yet.
    hill_clock: f32,
    /// Seconds until each of the map's armor pickups is back; 0 = it is there.
    armor_respawn: Vec<f32>,
    /// Conquest: who holds each of the map's flags, and the tickets each
    /// team has bled that are not whole yet.
    flags: Vec<FlagState>,
    bleed: [f32; 2],
    /// Where bots can walk, on this map.
    nav: NavGrid,
    /// How far apart fights on this map in this mode tend to be (metres):
    /// what bots pick their guns for.
    bot_range: f32,
    /// Where to report how the lobby is doing, when run by a lobby server.
    status: Option<Arc<Mutex<LobbyStatus>>>,
}

/// A conquest flag, as the server keeps it.
#[derive(Clone, Copy, Default)]
struct FlagState {
    owner: Option<u8>,
    /// -1 (all team 0's) to 1 (all team 1's).
    control: f32,
}

impl FlagState {
    /// Fully held by `team`, with nothing to win back.
    fn secure(&self, team: u8) -> bool {
        self.owner == Some(team) && self.control * if team == 0 { -1.0 } else { 1.0 } >= 0.999
    }
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
            settings.mode = (0..MODE_NAMES.len() as u8).find(|mode| map::plays(settings.map, *mode)).unwrap_or(MODE_FFA);
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
            nav: NavGrid::new(&map),
            bot_range: typical_range(&map, settings.mode),
            map,
            flags: Vec::new(),
            bleed: [0.0; 2],
            grenades: Vec::new(),
            smokes: Vec::new(),
            bullets: Vec::new(),
            noises: Vec::new(),
            next_noise_id: 1,
            next_grenade_id: 0,
            grenade_clock: 0.0,
            team_scores: [0, 0],
            hill: HILL_EMPTY,
            hill_clock: 0.0,
            armor_respawn: Vec::new(),
            players: HashMap::new(),
            phase: Phase::Lobby,
            countdown: 0.0,
            time_left: 0.0,
            join_counter: 0,
            next_bot_id: 1,
            snapshot_timer: 0.0,
            roster_dirty: false,
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

    fn report_status(&self) {
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
    fn log(&self, line: std::fmt::Arguments) {
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

    fn tick(&mut self, delta: Duration) {
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
                player.flags = state.flags;
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

        if self.roster_dirty {
            self.roster_dirty = false;
            let roster = self.roster_msg();
            self.broadcast(&roster);
        }
        self.transport.send_packets(&mut self.renet);
        self.report_status();
    }

    /// True once the match is under way: playing, and the countdown is over.
    fn live(&self) -> bool {
        self.phase == Phase::Playing && self.countdown <= 0.0
    }

    fn update_match(&mut self, dt: f32) {
        // Everybody is in place, frozen, while the countdown runs.
        if self.countdown > 0.0 {
            self.countdown = (self.countdown - dt).max(0.0);
            self.send_snapshot(dt);
            return;
        }
        self.time_left -= dt;

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
            // Time is up: the best score wins, a tie has no winner.
            let winner = if is_team_mode(self.settings.mode) {
                match self.team_scores[0].cmp(&self.team_scores[1]) {
                    std::cmp::Ordering::Greater => Winner::Team(0),
                    std::cmp::Ordering::Less => Winner::Team(1),
                    std::cmp::Ordering::Equal => Winner::Nobody,
                }
            } else {
                let best = self.players.values().map(|p| p.kills).max().unwrap_or(0);
                let mut leaders = self.players.iter().filter(|(_, p)| p.kills == best);
                match (leaders.next(), leaders.next()) {
                    (Some((id, _)), None) => Winner::Player(*id),
                    _ => Winner::Nobody,
                }
            };
            self.end_match(winner);
        }
    }

    /// Sends everybody a snapshot, `SNAPSHOT_RATE` times a second.
    fn send_snapshot(&mut self, dt: f32) {
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
    fn update_armor_pickups(&mut self, dt: f32) {
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
    fn update_hill(&mut self, dt: f32) {
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
    fn update_conquest(&mut self, dt: f32) {
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

    fn handle_message(&mut self, id: PlayerId, msg: ClientMsg) {
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
                self.send(id, &ServerMsg::Welcome { id });
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
                        },
                    );
                    self.spawn_player(id);
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
            ClientMsg::EndMatch if is_leader && self.phase == Phase::Playing => self.end_match(Winner::Nobody),
            ClientMsg::NextMode if is_leader && self.phase == Phase::Lobby => {
                // The next mode the map is played in.
                let modes = MODE_NAMES.len() as u8;
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
                let maps = map::MAP_NAMES.len() as u8;
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
            ClientMsg::Respawn if self.phase == Phase::Playing && early_respawn(self.settings.mode) => {
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

    /// A shot or knife swing. Checked against the weapon's fire rate, counted
    /// towards the hits this player may claim, and shown to everyone else.
    fn handle_fire(&mut self, shooter_id: PlayerId, weapon_id: WeaponId, origin: [f32; 3], dirs: Vec<[f32; 3]>) {
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
        if !shooter.alive || shooter.fire_credit < cost || origin_v.distance(shooter.pos) > 6.0 {
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
    fn handle_hit(&mut self, shooter_id: PlayerId, victim_id: PlayerId, weapon_id: WeaponId, headshot: bool) {
        if !self.live() || shooter_id == victim_id || weapon_id as usize >= WEAPONS.len() {
            return;
        }
        let Some(victim_pos) = self.players.get(&victim_id).filter(|v| v.alive).map(|v| v.pos) else {
            return;
        };
        let Some(shooter) = self.players.get_mut(&shooter_id) else {
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

    /// A frag's pin is pulled: the fuse burns from now, thrown or not.
    fn handle_cook(&mut self, id: PlayerId) {
        if !self.live() {
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
    fn handle_grenade(&mut self, thrower_id: PlayerId, kind: GrenadeKind, origin: [f32; 3], vel: [f32; 3]) {
        let (pos, vel) = (Vec3::from(origin), Vec3::from(vel));
        if !self.live() || !pos.is_finite() || !vel.is_finite() {
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

    /// Passes a ping on to the pinger's teammates, and nobody else. A ping
    /// on somebody who is not a living enemy becomes a ping on the spot.
    fn handle_ping(&mut self, from: PlayerId, target: Option<PlayerId>, pos: [f32; 3]) {
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
    fn teammates_of(&self, id: PlayerId) -> Vec<PlayerId> {
        let Some(team) = self.players.get(&id).map(|p| p.team) else {
            return Vec::new();
        };
        self.players
            .iter()
            .filter(|(other, p)| **other != id && p.team == team && p.bot.is_none())
            .map(|(other, _)| *other)
            .collect()
    }

    fn launch_grenade(&mut self, owner: PlayerId, kind: GrenadeKind, pos: Vec3, vel: Vec3, fuse: f32) {
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
    fn update_cooking(&mut self, dt: f32) {
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

    fn update_grenades(&mut self, dt: f32) {
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

    fn explode(&mut self, grenade: Grenade) {
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
    fn blast(&mut self, grenade: Grenade) {
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
    fn flash(&mut self, pos: Vec3) {
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

    fn damage(&mut self, victim_id: PlayerId, attacker_id: PlayerId, amount: i32, weapon_id: WeaponId) {
        let mode = self.settings.mode;
        let attacker_team = self.players.get(&attacker_id).map(|attacker| attacker.team);
        // The dead take no more hurt: the rest of a shotgun's pellets, or a
        // grenade going off over a body, must not kill them again.
        let Some(victim) = self.players.get_mut(&victim_id).filter(|victim| victim.alive) else {
            return;
        };
        let victim_team = victim.team & 1;
        let body = victim.pos;
        // No friendly fire. Your own grenade still hurts you.
        if is_team_mode(mode) && attacker_id != victim_id && attacker_team == Some(victim.team) {
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
        (victim.health, victim.armor) = take_damage(victim.health, victim.armor, amount);
        let (health, armor) = (victim.health, victim.armor);
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
        // In conquest every death costs the team a ticket.
        if mode == MODE_CONQUEST {
            let tickets = &mut self.team_scores[victim_team as usize];
            *tickets = tickets.saturating_sub(1);
            if *tickets == 0 {
                self.end_match(Winner::Team(victim_team ^ 1));
            }
            return;
        }
        let Some((kills, team)) = scorer else {
            return;
        };
        let limit = score_limit(mode, self.settings.frag_limit);
        match mode {
            MODE_FFA if kills >= limit => self.end_match(Winner::Player(attacker_id)),
            MODE_TEAMS => {
                self.team_scores[team] += 1;
                if self.team_scores[team] >= limit {
                    self.end_match(Winner::Team(team as u8));
                }
            }
            // On the hill, kills only clear the way.
            _ => {}
        }
    }

    fn start_match(&mut self) {
        self.log(format_args!("match started"));
        self.phase = Phase::Playing;
        self.countdown = COUNTDOWN;
        self.time_left = self.settings.match_minutes * 60.0;
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
            player.hurt_by.clear();
            player.alive = false;
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
        });
        let ids: Vec<PlayerId> = self.players.keys().copied().collect();
        for id in ids {
            self.spawn_player(id);
        }
    }

    /// Moves bots across until the teams differ by at most one player. People
    /// stay on the team they picked.
    fn balance_bots(&mut self) {
        loop {
            let ones = self.players.values().filter(|p| p.team == 1).count();
            let zeros = self.players.len() - ones;
            let (big, small) = if zeros > ones { (0, 1) } else { (1, 0) };
            if zeros.abs_diff(ones) < 2 {
                break;
            }
            let Some(bot) = self.players.values_mut().find(|p| p.bot.is_some() && p.team == big) else {
                break;
            };
            bot.team = small;
        }
    }

    fn end_match(&mut self, winner: Winner) {
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

    /// Spawns at one of the points farthest away from every living player (in
    /// the team modes: from every living enemy). In conquest, at the team's
    /// base or one of the flags it holds.
    fn spawn_player(&mut self, id: PlayerId) {
        let team_mode = is_team_mode(self.settings.mode);
        let my_team = self.players.get(&id).map_or(0, |p| p.team);
        let others: Vec<Vec3> = self
            .players
            .iter()
            .filter(|(other, p)| **other != id && p.alive && !(team_mode && p.team == my_team))
            .map(|(_, p)| p.pos)
            .collect();
        let pool = if self.settings.mode == MODE_CONQUEST {
            let choice = self.players.get(&id).and_then(|p| p.spawn_at);
            let group = self.conquest_spawn_group(my_team & 1, &others, choice);
            match group {
                Some(index) => &self.map.flags[index].spawns,
                None => &self.map.bases[my_team as usize & 1],
            }
        } else {
            &self.map.spawns
        };
        let pool = if pool.is_empty() { &self.map.spawns } else { pool };
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
            .filter(|spawn| !taken(spawn.pos))
            .map(|spawn| (away(spawn.pos), spawn.pos, spawn.yaw))
            .collect();
        if ranked.is_empty() {
            // Every point taken (more players than points, or a crowd standing
            // on them): somewhere clear near one of them instead.
            ranked = pool
                .iter()
                .flat_map(|spawn| {
                    let around = map::spawn_ring(&self.map, Vec2::new(spawn.pos.x, spawn.pos.z), 1.5, 6.0, 24);
                    around.into_iter().map(move |near| (near.pos, spawn.yaw))
                })
                .filter(|(pos, _)| !taken(*pos))
                .map(|(pos, yaw)| (away(pos), pos, yaw))
                .collect();
        }
        if ranked.is_empty() {
            // Nowhere clear at all: no better than overlapping somebody.
            ranked = pool.iter().map(|spawn| (0.0, spawn.pos, spawn.yaw)).collect();
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

        let Some(player) = self.players.get_mut(&id) else {
            return;
        };
        player.alive = true;
        player.health = MAX_HEALTH;
        player.armor = START_ARMOR;
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
        });
    }

    /// Conquest: which of the flags `team` holds to spawn at, or `None` for
    /// its base. The one picked (`choice`, see `ClientMsg::SpawnAt`) if it
    /// can be; otherwise mostly the one nearest a flag still to take (the
    /// front), sometimes another, never one with an enemy on it.
    fn conquest_spawn_group(&mut self, team: u8, enemies: &[Vec3], choice: Option<u8>) -> Option<usize> {
        let (roll, any) = (self.random() % 4, self.random() as usize);
        let flags = &self.map.flags;
        let ours: Vec<usize> = (0..flags.len())
            .filter(|&i| self.flags.get(i).is_some_and(|f| f.owner == Some(team)))
            .filter(|&i| !enemies.iter().any(|enemy| flags[i].holds(*enemy)))
            .collect();
        match choice {
            Some(0) => return None,
            Some(point) => return ours.contains(&(point as usize - 1)).then_some(point as usize - 1),
            None => {}
        }
        if ours.is_empty() {
            return None;
        }
        if roll == 0 {
            return None;
        }
        let to_take: Vec<Vec3> = (0..flags.len())
            .filter(|&i| self.flags.get(i).is_some_and(|f| f.owner != Some(team)))
            .map(|i| flags[i].pos)
            .collect();
        if roll == 1 || to_take.is_empty() {
            return Some(ours[any % ours.len()]);
        }
        let front = |i: &usize| to_take.iter().map(|p| p.distance(flags[*i].pos)).fold(f32::INFINITY, f32::min);
        ours.iter().copied().min_by(|a, b| front(a).total_cmp(&front(b)))
    }

    /// Plays `mode` from the next match on: on the map picked if it is played
    /// there, or else on the first map that it is.
    fn set_mode(&mut self, mode: u8) {
        if mode as usize >= MODE_NAMES.len() {
            return;
        }
        self.settings.mode = mode;
        if !map::plays(self.settings.map, mode) {
            let maps = map::MAP_NAMES.len() as u8;
            if let Some(id) = (0..maps).find(|id| map::plays(*id, mode)) {
                self.settings.map = id;
            }
            self.rebuild_map();
        } else if self.map.has_flagpoles() != (mode == MODE_CONQUEST) {
            // Into or out of conquest: the flagpoles go up or come down.
            self.rebuild_map();
        }
        self.roster_dirty = true;
    }

    /// Plays on map `id` from the next match on, if the mode is played there.
    fn set_map(&mut self, id: u8) {
        if id as usize >= map::MAP_NAMES.len() || !map::plays(id, self.settings.mode) || id == self.settings.map {
            return;
        }
        self.settings.map = id;
        self.rebuild_map();
        self.roster_dirty = true;
    }

    fn rebuild_map(&mut self) {
        self.map = Map::for_game(self.settings.map, self.settings.mode);
        self.nav = NavGrid::new(&self.map);
    }

    fn add_bot(&mut self) {
        if self.players.len() >= MAX_PLAYERS {
            return;
        }
        let id = self.next_bot_id;
        self.next_bot_id += 1;
        let name = format!("[bot] {}", BOT_NAMES[(id as usize - 1) % BOT_NAMES.len()]);
        self.join_counter += 1;
        let player = self.new_player(name, Some(Bot::new()));
        self.players.insert(id, player);
        self.roster_dirty = true;
        if self.phase == Phase::Playing {
            let roster = self.roster_msg();
            self.broadcast(&roster);
            self.roster_dirty = false;
            self.spawn_player(id);
        }
    }

    /// Bots run between random points and hop around, using the same
    /// collision code as real players, and shoot at the enemies they see.
    fn update_bots(&mut self, dt: f32) {
        let team_mode = is_team_mode(self.settings.mode);
        let conquest = self.settings.mode == MODE_CONQUEST;
        let hill = (self.settings.mode == MODE_HILL).then_some(self.map.hill);
        let bodies: Vec<Body> = self
            .players
            .iter()
            .filter(|(_, p)| p.alive)
            .map(|(id, p)| Body {
                id: *id,
                team: p.team,
                center: p.pos,
            })
            .collect();
        let skill = bot_skill(self.settings.bot_difficulty);
        let mut rng = self.rng;
        let mut shots = Vec::new();
        for (id, player) in self.players.iter_mut() {
            if !player.alive {
                continue;
            }
            let Some(mut bot) = player.bot.take() else {
                continue;
            };
            let enemy = bot_pick_enemy(&self.map, &self.smokes, *id, player, &mut bot, &bodies, team_mode, skill, &mut rng, dt);
            bot_listen(*id, player, &mut bot, &self.noises, team_mode, conquest, &mut rng);
            let target = if conquest {
                bot_objective(&self.map, &self.flags, player, &mut bot, &mut rng, dt)
                    .map(|flag| (self.map.flags[flag].pos, self.map.flags[flag].radius * 0.7, true))
            } else {
                hill.map(|hill| (hill, 5.5, false))
            };
            bot_move(&self.map, &self.nav, player, &mut bot, target, enemy, &mut rng, dt);
            let trigger = !self.settings.bots_hold_fire;
            if let Some(shot) = bot_fight(*id, player, &mut bot, enemy, trigger, skill, &mut rng, dt) {
                shots.push(shot);
            }
            player.bot = Some(bot);
        }
        self.rng = rng;

        // Everyone sees and hears a bot's shot like anybody else's.
        for (shooter, team, weapon_id, origin, dirs) in shots {
            self.make_noise(shooter, origin, NoiseKind::Shot);
            let speed = weapon(weapon_id).speed;
            self.bullets.extend(dirs.iter().map(|dir| BotBullet {
                shooter,
                team,
                weapon: weapon_id,
                pos: origin,
                vel: *dir * speed,
                age: 0.0,
            }));
            self.broadcast(&ServerMsg::Fire {
                shooter,
                weapon: weapon_id,
                origin: origin.to_array(),
                dirs: dirs.iter().map(|dir| dir.to_array()).collect(),
            });
        }
    }

    /// Something bots may hear, made by `source` at `pos`.
    fn make_noise(&mut self, source: PlayerId, pos: Vec3, kind: NoiseKind) {
        let Some(team) = self.players.get(&source).map(|player| player.team) else {
            return;
        };
        self.noises.push(Noise {
            id: self.next_noise_id,
            source,
            team,
            pos,
            kind,
            age: 0.0,
        });
        self.next_noise_id += 1;
    }

    /// Everyone's footsteps, worked out from how they move as the clients
    /// work out each other's: running level, not crouched, a step a stride.
    fn update_footsteps(&mut self, dt: f32) {
        let mut steps = Vec::new();
        for (id, player) in self.players.iter_mut() {
            if !player.alive {
                (player.step_from, player.step_clock, player.stride) = (player.pos, 0.0, 0.0);
                continue;
            }
            player.step_clock += dt;
            if player.step_clock < STEP_SAMPLE {
                continue;
            }
            let moved = player.pos - player.step_from;
            let (run, climb) = (moved.with_y(0.0).length() / player.step_clock, moved.y.abs() / player.step_clock);
            (player.step_from, player.step_clock) = (player.pos, 0.0);
            // Too fast is a respawn or a jump pad, not running.
            let running = (STEP_MIN_SPEED..15.0).contains(&run) && climb < 1.5 && player.flags & FLAG_CROUCH == 0;
            if !running {
                player.stride = STRIDE * 0.5;
                continue;
            }
            player.stride += moved.with_y(0.0).length();
            if player.stride >= STRIDE {
                player.stride %= STRIDE;
                steps.push((*id, player.pos - Vec3::Y * map::PLAYER_HALF.y));
            }
        }
        for (id, feet) in steps {
            self.make_noise(id, feet, NoiseKind::Step);
        }
    }

    /// Flies the bots' bullets the way the clients fly theirs: each stops at
    /// the first wall or enemy in its way, and hurts the enemy.
    fn update_bullets(&mut self, dt: f32) {
        let team_mode = is_team_mode(self.settings.mode);
        let (map, players) = (&self.map, &self.players);
        let mut hits = Vec::new();
        self.bullets.retain_mut(|bullet| {
            let step = bullet.vel * dt;
            let distance = step.length();
            if distance > 0.0 {
                let dir = step / distance;
                let wall = map.raycast(bullet.pos, dir, distance);
                let mut reach = wall.unwrap_or(distance);
                let mut victim = None;
                // Bullets pass through the shooter's own team.
                for (id, player) in players {
                    if !player.alive || *id == bullet.shooter || (team_mode && player.team == bullet.team) {
                        continue;
                    }
                    let crouching = player.flags & FLAG_CROUCH != 0;
                    let half = map::hitbox_half(crouching);
                    if let Some(t) = map::ray_box(bullet.pos, dir, player.pos - half, player.pos + half)
                        && t < reach
                    {
                        reach = t;
                        let (head_min, head_max) = map::head_box(player.pos, crouching);
                        victim = Some((*id, map::ray_box(bullet.pos, dir, head_min, head_max).is_some()));
                    }
                }
                if let Some((victim, headshot)) = victim {
                    hits.push((victim, bullet.shooter, bullet.weapon, headshot));
                    return false;
                }
                if wall.is_some() {
                    return false;
                }
            }
            bullet.pos += step;
            bullet.vel.y -= weapon(bullet.weapon).gravity * dt;
            bullet.age += dt;
            bullet.age <= BULLET_LIFETIME
        });
        for (victim, shooter, weapon_id, headshot) in hits {
            // A kill can end the match, and then nobody else gets hurt.
            if self.phase == Phase::Playing {
                self.damage(victim, shooter, hit_damage(weapon_id, headshot), weapon_id);
            }
        }
    }

    fn new_player(&self, name: String, bot: Option<Bot>) -> Player {
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
            bot,
        }
    }

    /// The lobby leader is the human who has been connected the longest.
    fn leader(&self) -> Option<PlayerId> {
        self.players
            .iter()
            .filter(|(_, p)| p.bot.is_none())
            .min_by_key(|(_, p)| p.join_order)
            .map(|(id, _)| *id)
    }

    fn roster_msg(&self) -> ServerMsg {
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
                    is_bot: p.bot.is_some(),
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

    fn send(&mut self, id: PlayerId, msg: &ServerMsg) {
        self.renet.send_message(id, CH_RELIABLE, encode(msg));
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        self.renet.broadcast_message(CH_RELIABLE, encode(msg));
    }

    fn random(&mut self) -> u64 {
        next_random(&mut self.rng)
    }
}

/// Keeps the hosted server alive; dropping it shuts the server down.
pub struct ServerHandle {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
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

/// True if a player coming in with their box centred at `spawn` would be
/// inside, or right up against, one whose box is centred at `body`.
fn crowds(body: Vec3, spawn: Vec3) -> bool {
    let apart = body - spawn;
    apart.with_y(0.0).length() < SPAWN_CLEARANCE && apart.y.abs() < 2.0 * map::PLAYER_HALF.y
}

/// How far apart (along the ground) two players' centres must be for one
/// to spawn by the other: their boxes' width and a step to spare.
const SPAWN_CLEARANCE: f32 = 2.0 * map::PLAYER_HALF.x + 0.7;

/// Where a bot's eyes are.
fn bot_eye(player: &Player) -> Vec3 {
    let crouched = player.flags & FLAG_CROUCH != 0;
    let eye = if crouched { map::CROUCH_EYE } else { map::STAND_EYE };
    player.pos + Vec3::Y * (eye - map::player_half(crouched).y)
}

/// Crouches the bot, or stands it back up if there is room, the way people
/// do: the box shrinks towards the feet, and clients are told.
fn bot_crouch(map: &Map, player: &mut Player, bot: &mut Bot, crouch: bool) {
    let drop = map::PLAYER_HALF.y - map::CROUCH_HALF.y;
    if crouch && !bot.crouched {
        player.pos.y -= drop;
    } else if !crouch && bot.crouched {
        let standing = player.pos + Vec3::Y * drop;
        if !map.is_free(standing, map::PLAYER_HALF) {
            return;
        }
        player.pos = standing;
    } else {
        return;
    }
    bot.crouched = crouch;
    player.flags = (player.flags & !FLAG_CROUCH) | if crouch { FLAG_CROUCH } else { 0 };
}

/// The point a bot aims at on a body: the upper chest.
fn aim_point(center: Vec3) -> Vec3 {
    center + Vec3::Y * 0.25
}

/// The distances (metres) at which a bot reckons a weapon does best.
fn bot_gun_band(gun: WeaponId) -> (f32, f32) {
    match gun {
        weapons::RIFLE => (5.0, 40.0),
        weapons::SMG => (0.0, 15.0),
        weapons::SHOTGUN => (0.0, 7.0),
        weapons::DMR => (15.0, 60.0),
        weapons::SNIPER => (25.0, LONG_RANGE),
        weapons::PISTOL => (0.0, 20.0),
        _ => (0.0, weapons::KNIFE_RANGE),
    }
}

/// How well a weapon suits a fight `distance` away: 1 within its band,
/// less the further outside it, never quite nothing.
fn bot_gun_fit(gun: WeaponId, distance: f32) -> f32 {
    let (near, far) = bot_gun_band(gun);
    let off = (near - distance).max(0.0) + (distance - far).max(0.0);
    (1.0 - off / 20.0).max(0.05)
}

/// The two guns a bot carries for a life, when fights tend to be `range`
/// away: mostly one that suits that range, less often one `teammates`
/// already lead with; and a second for where the first is weak.
fn bot_loadout(range: f32, teammates: &[WeaponId], rng: &mut u64) -> (WeaponId, WeaponId) {
    let pick = |rng: &mut u64, weight: &dyn Fn(WeaponId) -> f32| {
        let weights: Vec<f32> = (0..GUN_COUNT).map(weight).collect();
        let mut roll = random_unit(rng) * weights.iter().sum::<f32>();
        for (gun, weight) in weights.iter().enumerate() {
            roll -= weight;
            if roll <= 0.0 {
                return gun as WeaponId;
            }
        }
        GUN_COUNT - 1
    };
    let primary = pick(rng, &|gun| {
        let taken = teammates.iter().filter(|other| **other == gun).count() as f32;
        bot_gun_fit(gun, range).powi(2) / (1.0 + taken)
    });
    // A long gun wants something for up close, and a short one something
    // that reaches.
    let long = |gun: WeaponId| bot_gun_band(gun).0 >= 10.0;
    let secondary = if long(primary) {
        pick(rng, &|gun| if long(gun) { 0.0 } else { bot_gun_fit(gun, 6.0).powi(2) })
    } else {
        pick(rng, &|gun| if gun == primary { 0.0 } else { bot_gun_fit(gun, range.max(30.0)).powi(2) })
    };
    (primary, secondary)
}

/// Whether a bot would rather have its other gun in hand: in a fight
/// `distance` away, if it suits the distance clearly better, counting an
/// empty magazine against a gun (drawing the other beats reloading); with
/// nobody about, to reload the other, or to go back to its first.
fn bot_wants_spare(bot: &Bot, distance: Option<f32>) -> bool {
    match distance {
        Some(distance) => {
            let score = |gun: WeaponId, ammo: u32| bot_gun_fit(gun, distance) * if ammo == 0 { 0.35 } else { 1.0 };
            score(bot.spare, bot.spare_ammo) > score(bot.gun, bot.ammo) + 0.2
        }
        None => {
            let loaded = bot.reload <= 0.0 && bot.ammo >= weapon(bot.gun).magazine;
            loaded && (bot.spare_ammo < weapon(bot.spare).magazine || bot.gun != bot.primary)
        }
    }
}

/// How far apart fights on `map` tend to be in `mode`: how far one can
/// see from where people come in (around the hill or the flags, in those
/// modes), the longer half of the sight lines counting.
fn typical_range(map: &Map, mode: u8) -> f32 {
    let lift = Vec3::Y * (map::STAND_EYE - map::PLAYER_HALF.y);
    let centers: Vec<Vec3> = match mode {
        MODE_CONQUEST if !map.flags.is_empty() => map.flags.iter().flat_map(|flag| flag.spawns.iter().map(|s| s.pos)).collect(),
        MODE_HILL => (0..8)
            .map(|k| {
                let around = k as f32 * std::f32::consts::TAU / 8.0;
                map.hill + Vec3::new(around.cos() * 4.0, map::PLAYER_HALF.y, around.sin() * 4.0)
            })
            .filter(|center| map.is_free(*center, map::PLAYER_HALF))
            .collect(),
        _ => map.spawns.iter().map(|s| s.pos).collect(),
    };
    let mut sights: Vec<f32> = centers
        .iter()
        .flat_map(|center| {
            (0..16).map(move |k| {
                let dir = map::look_dir(k as f32 * std::f32::consts::TAU / 16.0, 0.0);
                map.raycast(*center + lift, dir, LONG_RANGE).unwrap_or(LONG_RANGE)
            })
        })
        .collect();
    if sights.is_empty() {
        return 25.0;
    }
    sights.sort_by(f32::total_cmp);
    let longer = &sights[sights.len() / 2..];
    longer.iter().sum::<f32>() / longer.len() as f32
}

/// Decides whom the bot fights, if anybody: the enemy it is already fighting
/// while it can still see them, otherwise the nearest one it notices.
/// Returns where that enemy is.
#[allow(clippy::too_many_arguments)]
fn bot_pick_enemy(
    map: &Map,
    smokes: &[Smoke],
    id: PlayerId,
    player: &Player,
    bot: &mut Bot,
    bodies: &[Body],
    team_mode: bool,
    skill: &BotSkill,
    rng: &mut u64,
    dt: f32,
) -> Option<Vec3> {
    bot.provoked_for -= dt;
    if bot.provoked_for <= 0.0 {
        bot.provoked_by = None;
    }
    bot.blind = (bot.blind - dt).max(0.0);
    if bot.blind > 0.0 {
        bot.enemy = None;
        bot.sight = 0.0;
        return None;
    }
    let eye = bot_eye(player);
    let facing = map::look_dir(player.yaw, 0.0);
    let mut current = None;
    let mut nearest: Option<(f32, &Body)> = None;
    for body in bodies {
        if body.id == id || (team_mode && body.team == player.team) {
            continue;
        }
        let offset = (body.center - player.pos).with_y(0.0);
        let distance = offset.length();
        let noticed = distance < BOT_HEARING
            || bot.provoked_by == Some(body.id)
            || offset.dot(facing) > BOT_VIEW_COS * distance;
        let fighting = Some(body.id) == bot.enemy;
        // No looking through walls (the dearer test) for one it would not notice anyway.
        if !noticed && !fighting {
            continue;
        }
        // As far as the eye can see.
        let to = aim_point(body.center);
        let in_sight = (map.line_of_sight(eye, to) || map.line_of_sight(eye, to + Vec3::Y * 0.5))
            && !smokes.iter().any(|smoke| map::smoke_blocks(smoke.center, eye, to));
        if !in_sight {
            continue;
        }
        if fighting {
            current = Some(body);
        }
        if noticed && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, body));
        }
    }
    let Some(target) = current.or(nearest.map(|(_, body)| body)) else {
        bot.enemy = None;
        bot.sight = 0.0;
        return None;
    };
    if bot.enemy == Some(target.id) {
        bot.sight += dt;
        // People's positions come in with the network's hiccups, so the
        // speed is an average over a moment, not this tick's jump.
        if dt > 0.0 {
            let measured = ((target.center - bot.enemy_at) / dt).clamp_length_max(60.0);
            bot.enemy_vel += (measured - bot.enemy_vel) * (dt / BOT_VEL_SMOOTHING).min(1.0);
        }
    } else {
        // A new enemy: take a moment to react, and dodge sideways right away.
        bot.enemy = Some(target.id);
        bot.sight = 0.0;
        let far = (target.center.distance(player.pos) - BOT_FAR).max(0.0);
        bot.reaction = skill.reaction + skill.reaction_jitter * random_unit(rng) + skill.spot_time * far;
        bot.enemy_vel = Vec3::ZERO;
        bot.wobble_in = 0.0;
        bot.retarget_in = 0.0;
    }
    bot.enemy_at = target.center;
    Some(target.center)
}

/// Hears the enemies' gunshots, grenades and footsteps made since it last
/// listened, and while not fighting anybody, guesses where the one that
/// matters most (the loudest kind, then the nearest) came from, to go and
/// see. In conquest only noises close by draw it off its flag.
fn bot_listen(id: PlayerId, player: &Player, bot: &mut Bot, noises: &[Noise], team_mode: bool, conquest: bool, rng: &mut u64) {
    let mut loudest: Option<(&Noise, f32)> = None;
    let heard_up_to = bot.last_noise;
    for noise in noises.iter().filter(|noise| noise.id > heard_up_to) {
        bot.last_noise = noise.id;
        if noise.source == id || (team_mode && noise.team == player.team) {
            continue;
        }
        let distance = noise.pos.distance(player.pos);
        if distance > noise.kind.reach() || (conquest && distance > BOT_INVESTIGATE_NEAR) {
            continue;
        }
        let louder = loudest.is_none_or(|(best, best_distance)| (noise.kind, -distance) > (best.kind, -best_distance));
        if louder {
            loudest = Some((noise, distance));
        }
    }
    let Some((noise, _)) = loudest else {
        return;
    };
    if bot.enemy.is_some() {
        return;
    }
    let (angle, depth) = noise.kind.guess();
    let turn = Quat::from_rotation_y(angle * (random_unit(rng) * 2.0 - 1.0));
    let scale = 1.0 + depth * (random_unit(rng) * 2.0 - 1.0);
    let guess = player.pos + turn * (noise.pos - player.pos).with_y(0.0) * scale;
    bot.investigate = Some(guess.with_y(noise.pos.y));
    bot.investigate_for = BOT_INVESTIGATE_TIME;
}

/// Conquest: the flag the bot is going for. It thinks again every so often,
/// and as soon as the flag it was after is safely its team's: then it goes
/// for one still to take, mostly the nearest, or else guards one it holds.
fn bot_objective(map: &Map, flags: &[FlagState], player: &Player, bot: &mut Bot, rng: &mut u64, dt: f32) -> Option<usize> {
    if flags.is_empty() {
        return None;
    }
    let team = player.team & 1;
    bot.rethink_in -= dt;
    let done = bot.objective.is_some_and(|i| flags[i].secure(team));
    if bot.objective.is_some() && bot.rethink_in > 0.0 && !done {
        return bot.objective;
    }
    bot.rethink_in = 20.0 + random_unit(rng) * 15.0;
    let mut to_take: Vec<usize> = (0..flags.len()).filter(|&i| !flags[i].secure(team)).collect();
    if to_take.is_empty() {
        bot.objective = Some(next_random(rng) as usize % flags.len());
        return bot.objective;
    }
    to_take.sort_by(|a, b| {
        let distance = |i: &usize| map.flags[*i].pos.distance(player.pos);
        distance(a).total_cmp(&distance(b))
    });
    let roll = random_unit(rng);
    let pick = if roll < 0.6 || to_take.len() == 1 {
        0
    } else if roll < 0.85 {
        1
    } else {
        next_random(rng) as usize % to_take.len()
    };
    bot.objective = Some(to_take[pick]);
    bot.objective
}

/// Runs towards the bot's goal, finding the way round buildings and the
/// like, hopping over whatever is still in the way. While fighting, the
/// goals are short dashes from side to side. `target` is somewhere to be
/// (centre, radius, and whether to stay there or only drop by now and then).
#[allow(clippy::too_many_arguments)]
fn bot_move(
    map: &Map,
    nav: &NavGrid,
    player: &mut Player,
    bot: &mut Bot,
    target: Option<(Vec3, f32, bool)>,
    enemy: Option<Vec3>,
    rng: &mut u64,
    dt: f32,
) {
    // Going to see what it heard: until it gets there, it has been at it
    // long enough, or it finds somebody to fight.
    bot.investigate_for -= dt;
    let looked = |spot: Vec3| bot.investigate_for <= 0.0 || (spot - player.pos).with_y(0.0).length() < 2.5;
    if (enemy.is_some() || bot.investigate.is_some_and(looked)) && bot.investigate.take().is_some() {
        bot.retarget_in = 0.0;
    }
    // Gloating over a body, while there is nothing better to do: nobody to
    // fight, nothing heard, nobody to get back at. Done, it stands up.
    bot.gloat_for -= dt;
    let distracted = enemy.is_some() || bot.investigate.is_some() || bot.provoked_by.is_some();
    let bobbed = bot.bobbing >= BOT_BOBS as f32 * 2.0 * BOT_BOB;
    let gave_up = bot.gloat_for <= 0.0 && bot.bobbing == 0.0;
    if (distracted || bobbed || gave_up) && bot.gloat.take().is_some() {
        bot.retarget_in = 0.0;
    }
    let on_body = bot.gloat.is_some_and(|body| (body - player.pos).with_y(0.0).length() < BOT_GLOAT_REACH);
    if on_body {
        bot.bobbing += dt;
    }
    let down = on_body && ((bot.bobbing / BOT_BOB) as u32).is_multiple_of(2);
    bot_crouch(map, player, bot, down);
    let half = map::player_half(bot.crouched);
    // Careful when fighting or creeping up on a noise; still on a body.
    let speed = if on_body {
        0.0
    } else if enemy.is_some() || bot.investigate.is_some() {
        5.5
    } else {
        7.0
    };
    bot.retarget_in -= dt;
    let to_goal = (bot.goal - player.pos).with_y(0.0);
    // A new target (a conquest flag) takes over at once.
    let off_target = target.is_some_and(|(center, radius, stay)| stay && (bot.goal - center).with_y(0.0).length() > radius + 0.5);
    if let Some(spot) = bot.investigate.or(bot.gloat) {
        bot.goal = spot;
    } else if bot.retarget_in <= 0.0 || to_goal.length() < 1.5 || (enemy.is_none() && off_target) {
        if let Some(enemy) = enemy {
            let towards = (enemy - player.pos).with_y(0.0).normalize_or_zero();
            let side = Vec3::new(-towards.z, 0.0, towards.x);
            let sign = if next_random(rng).is_multiple_of(2) { 1.0 } else { -1.0 };
            // Closing in, or backing off, to where its gun does best.
            let (near, far) = bot_gun_band(bot.gun);
            let distance = (enemy - player.pos).with_y(0.0).length();
            let closer = if distance > far {
                1.0
            } else if distance < near {
                -1.0
            } else {
                0.0
            };
            bot.goal = player.pos + side * sign * (3.0 + random_unit(rng) * 4.0) + towards * closer * (2.0 + random_unit(rng) * 3.0);
            bot.retarget_in = 0.6 + random_unit(rng);
        } else {
            let [min, max] = map.bounds;
            let x = min.x + random_unit(rng) * (max.x - min.x);
            let z = min.y + random_unit(rng) * (max.y - min.y);
            bot.goal = match target {
                // On the hill half the time, so there is something to fight
                // over; on a conquest flag, all the time.
                Some((center, radius, stay)) if stay || next_random(rng).is_multiple_of(2) => {
                    let (dx, dz) = (random_unit(rng) * 2.0 - 1.0, random_unit(rng) * 2.0 - 1.0);
                    center + Vec3::new(dx, 0.0, dz) * radius
                }
                _ => Vec3::new(x, 0.0, z),
            };
            // Far goals take a while to reach.
            let far = (bot.goal - player.pos).with_y(0.0).length() > 20.0;
            bot.retarget_in = if far { 45.0 } else { 2.0 + random_unit(rng) * 3.0 };
        }
    }
    // Follow the way to the goal while not fighting.
    let mut steer = bot.goal;
    if enemy.is_none() {
        let feet = player.pos - Vec3::Y * half.y;
        let goal = bot.goal.with_y(map.ground(bot.goal.x, bot.goal.z).unwrap_or(0.0));
        if bot.path_to.is_none_or(|to| to.distance(goal) > 0.5) {
            bot.path = nav.path(map, feet, goal).unwrap_or_default();
            bot.path_to = Some(goal);
        }
        crate::nav::pass_waypoints(map, &mut bot.path, player.pos);
        if let Some(next) = bot.path.first() {
            steer = *next;
        }
    } else {
        bot.path_to = None;
    }
    let to_goal = (steer - player.pos).with_y(0.0);
    let wish = to_goal.normalize_or_zero() * speed;
    let blend = (8.0 * dt).min(1.0);
    bot.vel.x += (wish.x - bot.vel.x) * blend;
    bot.vel.z += (wish.z - bot.vel.z) * blend;
    bot.vel.y -= 22.0 * dt;
    let wanted = Vec3::new(bot.vel.x, 0.0, bot.vel.z);
    let mut grounded = map.move_and_slide(&mut player.pos, &mut bot.vel, half, dt);
    if bot.grounded && !grounded && bot.vel.y <= 0.0 {
        let drop = map::STEP_UP.max(speed * dt);
        if map.snap_to_ground(&mut player.pos, half, drop) {
            grounded = true;
            bot.vel.y = 0.0;
        }
    }
    bot.grounded = grounded;
    if let Some(pad) = map.pad_at(player.pos - Vec3::Y * half.y) {
        // Bots aim with arrow pads from a standstill so they land where intended.
        bot.vel = match pad.direction() {
            Some(_) => pad.launch,
            None => pad.launch_velocity(bot.vel),
        };
    } else if grounded {
        // Hop when blocked by something, and every now and then for fun.
        let blocked = Vec3::new(bot.vel.x, 0.0, bot.vel.z).length() < wanted.length() * 0.5;
        if !on_body && (blocked || next_random(rng).is_multiple_of(90)) {
            bot.vel.y = 8.0;
        }
    }
    // Getting nowhere for a while: find the way again, or give up on the goal.
    let moving = Vec3::new(bot.vel.x, 0.0, bot.vel.z).length() > 1.0;
    bot.stuck = if moving || wanted.length() < 0.1 { 0.0 } else { bot.stuck + dt };
    if bot.stuck > 1.5 {
        bot.stuck = 0.0;
        if bot.path_to.is_some() {
            bot.path_to = None;
        } else {
            // A noise, or a body, somewhere it cannot get to is not worth the trouble.
            bot.investigate = None;
            bot.gloat = None;
            bot.retarget_in = 0.0;
        }
    }
    // Not fighting: look where it is going, or, having just heard
    // something, the way it came from.
    if enemy.is_none() {
        let glance = bot.investigate.filter(|_| bot.investigate_for > BOT_INVESTIGATE_TIME - BOT_GLANCE_TIME);
        if let Some(spot) = glance {
            let to = spot - player.pos;
            player.yaw = turn_towards(player.yaw, (-to.x).atan2(-to.z), BOT_TURN_RATE * 2.0 * dt);
        } else if wish.length_squared() > 0.01 {
            player.yaw = turn_towards(player.yaw, (-wish.x).atan2(-wish.z), BOT_TURN_RATE * 2.0 * dt);
        }
        // Looking down at the body it is on.
        let pitch = if on_body { -0.6 } else { 0.0 };
        player.pitch = turn_towards(player.pitch, pitch, BOT_TURN_RATE * dt);
    }
}

/// Keeps a bot's gun loaded and aims it at `enemy` as well as its `skill`
/// allows, and shoots when it is lined up, unless it may not pull the
/// `trigger`.
#[allow(clippy::too_many_arguments)]
fn bot_fight(
    id: PlayerId,
    player: &mut Player,
    bot: &mut Bot,
    enemy: Option<Vec3>,
    trigger: bool,
    skill: &BotSkill,
    rng: &mut u64,
    dt: f32,
) -> Option<BotShot> {
    bot.cooldown = (bot.cooldown - dt).max(0.0);
    bot.switch_in -= dt;
    if bot.switch_in <= 0.0 && bot_wants_spare(bot, enemy.map(|enemy| enemy.distance(player.pos))) {
        std::mem::swap(&mut bot.gun, &mut bot.spare);
        std::mem::swap(&mut bot.ammo, &mut bot.spare_ammo);
        bot.reload = 0.0;
        bot.burst = 0;
        bot.cooldown = DRAW_TIME;
        bot.switch_in = if enemy.is_some() { 1.0 } else { 0.5 };
    }
    let stats = weapon(bot.gun);
    if bot.reload > 0.0 {
        bot.reload -= dt;
        if bot.reload <= 0.0 {
            bot.reload = 0.0;
            if stats.reload_per_round {
                // Round by round until it is full, unless there is someone to shoot.
                bot.ammo += 1;
                if bot.ammo < stats.magazine && enemy.is_none() {
                    bot.reload = stats.reload;
                }
            } else {
                bot.ammo = stats.magazine;
            }
        }
    } else if bot.ammo == 0 || (enemy.is_none() && bot.ammo < stats.magazine) {
        // Empty, or a quiet moment to top up.
        bot.reload = stats.reload;
    }
    player.weapon = bot.gun;
    player.flags = (bot.crouched as u8 * FLAG_CROUCH) | if bot.reload > 0.0 { FLAG_RELOAD } else { 0 };
    let enemy = enemy?;
    // Aims down sights at whoever it is after, between reloads.
    if bot.reload <= 0.0 && player.weapon != weapons::KNIFE {
        player.flags |= FLAG_AIM;
    }

    let eye = bot_eye(player);
    // The aim wobbles around the target: widely at first, then less; and
    // more while the enemy (or the bot itself) moves fast across its view.
    bot.wobble_in -= dt;
    if bot.wobble_in <= 0.0 {
        bot.wobble_in = 0.25 + random_unit(rng) * 0.25;
        let offset = aim_point(enemy) - eye;
        let distance = offset.length().max(1.0);
        let along = offset / distance;
        let relative = bot.enemy_vel - bot.vel;
        let sweep = (relative - along * relative.dot(along)).length() / distance;
        let far = 1.0 + (distance - BOT_FAR).max(0.0) / skill.far_doubling;
        let tracking = skill.tracking * (sweep - skill.easy_track).max(0.0);
        let error = skill.aim_error * far + skill.aim_slop / distance + tracking;
        let unsettled = 1.0 + 1.5 * (1.0 - bot.sight / skill.settle).max(0.0);
        let radius = error * unsettled * random_unit(rng).sqrt();
        let around = random_unit(rng) * std::f32::consts::TAU;
        bot.aim_error = Vec2::new(around.cos(), around.sin()) * radius;
        bot.lead = skill.lead.0 + (skill.lead.1 - skill.lead.0) * random_unit(rng);
    }
    // Aim where the enemy will be when the bullet gets there, and above it
    // by as much as the bullet will drop on the way.
    let mut target = aim_point(enemy);
    if stats.speed > 0.0 {
        let flight = eye.distance(target) / stats.speed;
        let flight = eye.distance(target + bot.enemy_vel * flight) / stats.speed;
        target += bot.enemy_vel * flight * bot.lead;
        target.y += 0.5 * stats.gravity * flight * flight;
    }
    let to = target - eye;
    let yaw = (-to.x).atan2(-to.z) + bot.aim_error.x;
    let pitch = to.y.atan2(to.with_y(0.0).length()) + bot.aim_error.y;
    player.yaw = turn_towards(player.yaw, yaw, skill.turn_rate * dt);
    player.pitch = turn_towards(player.pitch, pitch, skill.turn_rate * dt);
    let lined_up = angle_between(player.yaw, yaw).abs() < 0.1 && (player.pitch - pitch).abs() < 0.1;
    if !trigger || !lined_up || bot.sight < bot.reaction || bot.reload > 0.0 || bot.cooldown > 0.0 || bot.ammo == 0 {
        return None;
    }

    bot.ammo -= 1;
    bot.cooldown = stats.interval;
    if stats.auto {
        // Automatic guns go in bursts.
        if bot.burst == 0 {
            bot.burst = 3 + (next_random(rng) % 5) as u32;
        }
        bot.burst -= 1;
        if bot.burst == 0 {
            bot.cooldown += 0.3 + random_unit(rng) * 0.4;
        }
    } else {
        // Nobody clicks at exactly the fire rate.
        bot.cooldown += 0.1 + random_unit(rng) * 0.25;
    }
    // Every bullet goes to a random point inside the spread cone, like
    // people's: the aimed one, as bots shoot aimed (their own aim is off
    // enough without the hip's spread on top).
    let rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    let cone = stats.aim_spread;
    let dirs = (0..stats.pellets)
        .map(|_| {
            let off_axis = cone * random_unit(rng).sqrt();
            let around = random_unit(rng) * std::f32::consts::TAU;
            (rotation * Vec3::new(off_axis * around.cos(), off_axis * around.sin(), -1.0)).normalize()
        })
        .collect();
    Some((id, player.team, bot.gun, eye, dirs))
}

/// `angle - from`, wrapped to -π..π.
fn angle_between(from: f32, angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle - from + PI).rem_euclid(TAU) - PI
}

/// Turns `current` towards `wanted` by at most `max` radians, the short way round.
fn turn_towards(current: f32, wanted: f32, max: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let turned = current + angle_between(current, wanted).clamp(-max, max);
    (turned + PI).rem_euclid(TAU) - PI
}

/// `(health, armor)` after taking `amount` damage. Armor takes the hit first;
/// whatever it cannot hold goes through to health.
fn take_damage(health: i32, armor: i32, amount: i32) -> (i32, i32) {
    let absorbed = amount.clamp(0, armor.max(0));
    ((health - (amount - absorbed)).max(0), armor - absorbed)
}

/// xorshift64
fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state >> 11
}

/// A random number in 0..1.
fn random_unit(state: &mut u64) -> f32 {
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
        for _ in 0..4 {
            server.handle_message(1, ClientMsg::NextMap);
            maps.push(server.settings.map);
        }
        assert_eq!(maps, [0, 1, 2, 3]);
        assert_eq!(server.map.id, 3);
        // In conquest, from Valley on to Arena.
        server.handle_message(1, ClientMsg::NextMap);
        server.handle_message(1, ClientMsg::NextMode);
        server.handle_message(1, ClientMsg::NextMode);
        assert_eq!(server.settings.mode, MODE_CONQUEST);
        server.handle_message(1, ClientMsg::NextMap);
        server.handle_message(1, ClientMsg::NextMap);
        assert_eq!(server.settings.map, 2);
        server.handle_message(1, ClientMsg::NextMap);
        assert_eq!(server.settings.map, 0, "Hamlet skipped");
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
    fn removing_a_bot_takes_the_last_one_in() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        server.join_counter += 1;
        let player = server.new_player("P".into(), None);
        // Clear of the bots' ids, which count up from 1.
        let me = 1000;
        server.players.insert(me, player);
        server.handle_message(me, ClientMsg::AddBot);
        server.handle_message(me, ClientMsg::AddBot);
        let bots = |server: &Server| {
            let mut ids: Vec<_> = server.players.iter().filter(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).collect();
            ids.sort();
            ids
        };
        let before = bots(&server);
        assert_eq!(before.len(), 2);
        server.handle_message(me, ClientMsg::RemoveBot);
        assert_eq!(bots(&server), [before[0]], "the first bot stays");
        assert!(server.players.contains_key(&me), "people are never taken out");
    }

    #[test]
    fn bots_load_a_shotgun_shell_by_shell() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Loader".into(), None);
        let mut bot = Bot::new();
        bot.gun = weapons::SHOTGUN;
        let stats = weapon(weapons::SHOTGUN);
        let mut rng = 1;
        let dt = 0.01;
        let mut loading_for = 0.0;
        // Empty: it starts loading, one shell per `reload` seconds, until full.
        loop {
            bot_fight(1, &mut player, &mut bot, None, true, bot_skill(BOT_NORMAL), &mut rng, dt);
            if bot.reload <= 0.0 {
                break;
            }
            loading_for += dt;
            let loaded = (loading_for / stats.reload) as u32;
            assert!(bot.ammo.abs_diff(loaded) <= 1, "{} shells after {loading_for:.2} s", bot.ammo);
            assert!(loading_for < 10.0, "never done");
        }
        assert_eq!(bot.ammo, stats.magazine);
        assert!((loading_for - stats.reload * stats.magazine as f32).abs() < 0.1, "{loading_for}");
    }

    #[test]
    fn bots_fight_each_other() {
        let settings = ServerSettings {
            port: 0,
            bots: 4,
            ..ServerSettings::default()
        };
        let mut server = Server::bind(settings).unwrap();
        server.start_match();
        server.countdown = 0.0;
        // A minute of play.
        for _ in 0..60 * 60 {
            server.update_match(1.0 / 60.0);
            if server.phase != Phase::Playing {
                break;
            }
        }
        let kills: u32 = server.players.values().map(|p| p.kills).sum();
        assert!(kills > 0, "bots never killed anybody");
    }

    /// A conquest match on `map` with people on the given teams.
    fn conquest_with(map: u8, teams: &[u8]) -> Server {
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

    fn stand_on_flag(server: &mut Server, id: PlayerId, flag: usize) {
        let pos = server.map.flags[flag].spawns[0].pos;
        server.players.get_mut(&id).unwrap().pos = pos;
    }

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
            for mode in (0..MODE_NAMES.len() as u8).filter(|mode| map::plays(map, *mode)) {
                let mut server = Server::bind(ServerSettings { port: 0, mode, map, ..ServerSettings::default() }).unwrap();
                while server.players.len() < MAX_PLAYERS {
                    server.add_bot();
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

    #[test]
    fn conquest_spawns_are_at_the_base_or_a_flag_held() {
        let mut server = conquest_with(2, &[0]);
        let near = |pos: Vec3, spawns: &[map::Spawn]| spawns.iter().any(|s| s.pos.distance(pos) < 0.01);
        for _ in 0..10 {
            server.spawn_player(1000);
            assert!(near(server.players[&1000].pos, &server.map.bases[0]));
        }
        // A flag picked but not held: the base.
        server.handle_message(1000, ClientMsg::SpawnAt { point: Some(3) });
        server.spawn_player(1000);
        assert!(near(server.players[&1000].pos, &server.map.bases[0]));
        server.flags[2] = FlagState { owner: Some(0), control: -1.0 };
        let mut at_flag = 0;
        for _ in 0..40 {
            server.spawn_player(1000);
            let pos = server.players[&1000].pos;
            assert!(near(pos, &server.map.bases[0]) || near(pos, &server.map.flags[2].spawns));
            at_flag += near(pos, &server.map.flags[2].spawns) as u32;
        }
        assert!(at_flag > 20, "only {at_flag} of 40 at the flag");
        // Picked and held: always there.
        server.handle_message(1000, ClientMsg::SpawnAt { point: Some(3) });
        for _ in 0..10 {
            server.spawn_player(1000);
            assert!(near(server.players[&1000].pos, &server.map.flags[2].spawns));
        }
    }

    #[test]
    fn bots_take_flags_on_the_valley() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode: MODE_CONQUEST,
            map: 2,
            bots: 4,
            ..ServerSettings::default()
        })
        .unwrap();
        server.start_match();
        server.countdown = 0.0;
        // Two minutes of play.
        for _ in 0..60 * 120 {
            server.update_match(1.0 / 60.0);
            if server.phase != Phase::Playing {
                break;
            }
        }
        let taken: Vec<Option<u8>> = server.flags.iter().map(|f| f.owner).collect();
        println!("flags after two minutes: {taken:?}, tickets {:?}", server.team_scores);
        assert!(taken.iter().filter(|owner| owner.is_some()).count() >= 2, "{taken:?}");
    }

    #[test]
    fn bots_shoot_a_person_standing_in_front_of_them() {
        let (fired, hurt) = bot_in_front_of_a_person(false);
        assert!(fired && hurt, "the bot never hit the person in front of it");
    }

    /// Bots carry short guns where fights are close and long ones where they
    /// are far, with a second gun for the other end.
    #[test]
    fn bots_pick_guns_for_the_map() {
        for id in 0..map::MAP_NAMES.len() as u8 {
            for mode in 0..MODE_NAMES.len() as u8 {
                if map::plays(id, mode) {
                    let range = typical_range(&Map::for_game(id, mode), mode);
                    println!("{} / {}: {range:.1} m", map::MAP_NAMES[id as usize], MODE_NAMES[mode as usize]);
                }
            }
        }
        let share = |range: f32, guns: &[WeaponId]| {
            let mut rng = 99;
            let picks: Vec<(WeaponId, WeaponId)> = (0..2000).map(|_| bot_loadout(range, &[], &mut rng)).collect();
            assert!(picks.iter().all(|(a, b)| a != b && *a < GUN_COUNT && *b < GUN_COUNT));
            picks.iter().filter(|(primary, _)| guns.contains(primary)).count() as f32 / picks.len() as f32
        };
        let long = [weapons::DMR, weapons::SNIPER];
        let short = [weapons::SMG, weapons::SHOTGUN, weapons::PISTOL];
        assert!(share(10.0, &short) > 0.55, "close quarters: {}", share(10.0, &short));
        assert!(share(10.0, &[weapons::SNIPER]) < 0.03);
        assert!(share(55.0, &long) > 0.55, "open ground: {}", share(55.0, &long));
        assert!(share(55.0, &[weapons::SHOTGUN]) < 0.02);
        // A sniper's second gun is for up close.
        let mut rng = 5;
        for _ in 0..200 {
            let (primary, secondary) = bot_loadout(65.0, &[], &mut rng);
            if primary == weapons::SNIPER {
                assert!(bot_gun_band(secondary).0 < 10.0, "sniper with {}", weapon(secondary).name);
            }
        }
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

    /// A bot draws its other gun when it suits the fight better, or when
    /// the one in hand is empty with an enemy close; and with nobody about,
    /// loads both and goes back to the first.
    #[test]
    fn bots_change_guns_for_the_fight() {
        let mut bot = Bot::new();
        bot.gun = weapons::SNIPER;
        bot.ammo = weapon(bot.gun).magazine;
        bot.spare = weapons::PISTOL;
        bot.spare_ammo = weapon(bot.spare).magazine;
        bot.primary = weapons::SNIPER;
        assert!(!bot_wants_spare(&bot, Some(50.0)), "pistol for a far fight");
        assert!(bot_wants_spare(&bot, Some(4.0)), "sniper up close");
        assert!(!bot_wants_spare(&bot, None), "both loaded, first in hand");
        bot.gun = weapons::RIFLE;
        bot.primary = weapons::RIFLE;
        bot.spare = weapons::SMG;
        assert!(!bot_wants_spare(&bot, Some(10.0)), "swapping between two that both do");
        bot.ammo = 0;
        assert!(bot_wants_spare(&bot, Some(10.0)), "reloading with an enemy close");
        bot.ammo = weapon(bot.gun).magazine;
        bot.spare_ammo = 3;
        assert!(bot_wants_spare(&bot, None), "the other left half empty");
        std::mem::swap(&mut bot.gun, &mut bot.spare);
        std::mem::swap(&mut bot.ammo, &mut bot.spare_ammo);
        bot.ammo = weapon(bot.gun).magazine;
        assert!(bot_wants_spare(&bot, None), "back to the first");
    }

    /// Nothing in the way, a bot sees an enemy in front of it however far.
    #[test]
    fn bots_see_as_far_as_there_is_a_clear_line() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        // High above the map, where nothing is in the way.
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::new(0.0, 300.0, 0.0);
        player.yaw = 0.0;
        player.team = 0;
        let mut bot = Bot::new();
        let far = Body {
            id: 2,
            team: 1,
            center: Vec3::new(0.0, 300.0, -250.0),
        };
        let mut rng = 1;
        let seen = bot_pick_enemy(&server.map, &[], 1, &player, &mut bot, &[far], false, bot_skill(BOT_NORMAL), &mut rng, 0.1);
        assert_eq!(seen, Some(Vec3::new(0.0, 300.0, -250.0)));
    }

    /// Running makes footsteps, a step a stride; crouching or standing, none.
    #[test]
    fn running_makes_footsteps() {
        let steps_made = |crouched: bool, speed: f32| {
            let mut server = Server::bind(ServerSettings {
                port: 0,
                ..ServerSettings::default()
            })
            .unwrap();
            let mut person = server.new_player("Runner".into(), None);
            person.alive = true;
            person.pos = Vec3::new(0.0, 50.0, 0.0);
            person.step_from = person.pos;
            person.flags = if crouched { FLAG_CROUCH } else { 0 };
            server.players.insert(1000, person);
            let dt = 1.0 / 60.0;
            let mut steps = 0;
            // Five seconds.
            for _ in 0..300 {
                server.players.get_mut(&1000).unwrap().pos.x += speed * dt;
                server.update_footsteps(dt);
                steps += server.noises.drain(..).filter(|noise| noise.kind == NoiseKind::Step).count();
            }
            steps
        };
        let running = steps_made(false, 7.0);
        // 35 m at a step every 2.6.
        assert!((12..=14).contains(&running), "{running} steps");
        assert_eq!(steps_made(true, 4.0), 0, "crouched");
        assert_eq!(steps_made(false, 0.0), 0, "standing");
    }

    /// Having killed somebody close by, with nobody else about, a bot goes
    /// over to the body and crouches up and down on it, then gets up and
    /// goes about its business. Killed from far off, or with a noise to go
    /// and see, it does not bother.
    #[test]
    fn bots_teabag_who_they_killed_close_by() {
        // (crouched on the body at some point, ended up standing)
        let gloat = |apart: f32, noise: bool| {
            let mut server = Server::bind(ServerSettings {
                port: 0,
                bots: 1,
                ..ServerSettings::default()
            })
            .unwrap();
            server.join_counter += 1;
            let human = server.new_player("Victim".into(), None);
            server.players.insert(1000, human);
            server.start_match();
            server.countdown = 0.0;
            let bot = server.players.iter().find(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).unwrap();
            // A spawn point with open ground `apart` metres in some direction.
            let (bot_pos, victim_pos) = server
                .map
                .spawns
                .iter()
                .flat_map(|spawn| (0..16).map(move |step| (spawn.pos, step as f32 * std::f32::consts::TAU / 16.0)))
                .map(|(from, yaw)| (from, from + map::look_dir(yaw, 0.0) * apart))
                .find(|(from, to)| {
                    server.map.is_free(*to, map::PLAYER_HALF)
                        && server.map.line_of_sight(*from, *to)
                        && server.map.line_of_sight(*from - Vec3::Y * 0.8, *to - Vec3::Y * 0.8)
                        // Standing on something, not in the air.
                        && !server.map.is_free(*to - Vec3::Y * 0.2, map::PLAYER_HALF)
                })
                .expect("no open ground on the map");
            server.players.get_mut(&bot).unwrap().pos = bot_pos;
            let victim = server.players.get_mut(&1000).unwrap();
            victim.pos = victim_pos;
            victim.armor = 0;
            server.damage(1000, bot, 1000, weapons::RIFLE);
            assert!(!server.players[&1000].alive);
            if noise {
                server.make_noise(1000, victim_pos + Vec3::X * 20.0, NoiseKind::Shot);
                // Somebody else's, really: the dead make no noise.
                server.noises.last_mut().unwrap().source = 2000;
            }
            let mut bobbed = false;
            for _ in 0..60 * 8 {
                server.update_bots(1.0 / 60.0);
                let player = &server.players[&bot];
                let crouched = player.flags & FLAG_CROUCH != 0;
                let on_body = (player.pos - victim_pos).with_y(0.0).length() < 1.0;
                bobbed |= crouched && on_body;
            }
            (bobbed, server.players[&bot].flags & FLAG_CROUCH == 0)
        };
        assert_eq!(gloat(6.0, false), (true, true), "killed close by");
        assert_eq!(gloat(30.0, false), (false, true), "killed far off");
        assert_eq!(gloat(6.0, true), (false, true), "heard a shot");
    }

    /// A bot hears an enemy's step close by, and goes to see roughly where
    /// it was; a shot from much further, and knows better where it was.
    /// Teammates' noises, far steps, and noises while it fights, it ignores.
    #[test]
    fn bots_hear_steps_and_shots_and_go_to_see() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::ZERO;
        player.team = 0;
        let noise = |id: u64, team: u8, pos: Vec3, kind: NoiseKind| Noise {
            id,
            source: 2,
            team,
            pos,
            kind,
            age: 0.0,
        };
        let guessed = |noises: &[Noise], team_mode: bool, rng: &mut u64| {
            let mut bot = Bot::new();
            bot_listen(1, &player, &mut bot, noises, team_mode, false, rng);
            bot.investigate
        };
        let mut rng = 7;
        let step = Vec3::new(0.0, 0.0, -10.0);
        for _ in 0..200 {
            let guess = guessed(&[noise(1, 1, step, NoiseKind::Step)], true, &mut rng).expect("not heard");
            let off = guess.with_y(0.0).angle_between(step);
            let distance = guess.with_y(0.0).length();
            assert!(off <= STEP_GUESS.0 + 1e-3, "{off} rad off");
            assert!((6.0 - 1e-3..=14.0 + 1e-3).contains(&distance), "{distance} m");
        }
        let shot = Vec3::new(80.0, 0.0, 0.0);
        let guess = guessed(&[noise(1, 1, shot, NoiseKind::Shot)], true, &mut rng).expect("shot not heard");
        assert!(guess.with_y(0.0).angle_between(shot) <= SHOT_GUESS.0 + 1e-3);
        assert_eq!(guessed(&[noise(1, 0, step, NoiseKind::Step)], true, &mut rng), None, "a teammate");
        assert!(guessed(&[noise(1, 0, step, NoiseKind::Step)], false, &mut rng).is_some(), "everyone's an enemy");
        assert_eq!(guessed(&[noise(1, 1, step * 2.0, NoiseKind::Step)], true, &mut rng), None, "steps 20 m off");
        // The shot wins over a step, wherever they are.
        let both = [noise(1, 1, step, NoiseKind::Step), noise(2, 1, shot, NoiseKind::Shot)];
        let guess = guessed(&both, true, &mut rng).unwrap();
        assert!(guess.with_y(0.0).angle_between(shot) < 0.2);
        // Busy fighting.
        let mut bot = Bot::new();
        bot.enemy = Some(3);
        bot_listen(1, &player, &mut bot, &both, true, false, &mut rng);
        assert_eq!(bot.investigate, None);
        assert_eq!(bot.last_noise, 2, "heard them anyway");
    }

    /// A grenade going off is a noise where it went off, made by whoever
    /// threw it: a frag or a flash a bang heard from far off, a smoke a
    /// quieter pop.
    #[test]
    fn grenades_going_off_are_heard() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        server.start_match();
        let mut thrower = server.new_player("Thrower".into(), None);
        thrower.team = 1;
        server.players.insert(1000, thrower);
        let at = Vec3::new(0.0, 300.0, 0.0);
        for (k, kind) in [FRAG, FLASH, SMOKE].into_iter().enumerate() {
            server.explode(Grenade {
                id: k as u32,
                kind,
                owner: 1000,
                pos: at,
                vel: Vec3::ZERO,
                fuse: 0.0,
            });
        }
        let heard: Vec<(NoiseKind, PlayerId, u8, Vec3)> = server.noises.iter().map(|n| (n.kind, n.source, n.team, n.pos)).collect();
        assert_eq!(
            heard,
            [
                (NoiseKind::Bang, 1000, 1, at),
                (NoiseKind::Bang, 1000, 1, at),
                (NoiseKind::Pop, 1000, 1, at)
            ]
        );
        // A bot 120 m off hears the bangs but not the pop.
        let mut listener = server.new_player("Bot".into(), None);
        listener.team = 0;
        let mut rng = 4;
        let mut bot = Bot::new();
        listener.pos = at + Vec3::X * 120.0;
        bot_listen(1, &listener, &mut bot, &server.noises[2..], true, false, &mut rng);
        assert_eq!(bot.investigate, None, "a smoke from 120 m");
        let mut bot = Bot::new();
        bot_listen(1, &listener, &mut bot, &server.noises[..1], true, false, &mut rng);
        assert!(bot.investigate.is_some(), "a frag from 120 m");
    }

    /// Having heard something behind it, a bot turns that way.
    #[test]
    fn a_bot_turns_to_what_it_heard() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::new(0.0, 300.0, 0.0);
        player.yaw = 0.0;
        let mut bot = Bot::new();
        bot.investigate = Some(Vec3::new(0.0, 300.0, 12.0));
        bot.investigate_for = BOT_INVESTIGATE_TIME;
        let mut rng = 3;
        for _ in 0..30 {
            bot_move(&server.map, &server.nav, &mut player, &mut bot, None, None, &mut rng, 1.0 / 60.0);
        }
        assert!(angle_between(player.yaw, std::f32::consts::PI).abs() < 0.2, "facing {}", player.yaw);
        assert_eq!(bot.goal.with_y(0.0), Vec3::new(0.0, 0.0, 12.0));
    }

    /// Someone running across the bot's view 40 m away: it aims ahead of
    /// them, in the way they are running.
    #[test]
    fn bots_lead_a_running_target() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::ZERO;
        player.yaw = 0.0;
        player.pitch = 0.0;
        let mut bot = Bot::new();
        bot.gun = weapons::SMG;
        bot.ammo = weapon(bot.gun).magazine;
        // Long settled on them, running to the right (+x) at 7 m/s.
        bot.sight = 10.0;
        bot.enemy_vel = Vec3::X * 7.0;
        let enemy = Vec3::new(0.0, 0.0, -40.0);
        let to = aim_point(enemy) - bot_eye(&player);
        let straight = (-to.x).atan2(-to.z);
        // Its aim wobbles, so on average over ten seconds (after turning).
        let mut rng = 12345;
        let mut off = 0.0;
        for tick in 0..60 * 11 {
            bot_fight(1, &mut player, &mut bot, Some(enemy), false, bot_skill(BOT_EASY), &mut rng, 1.0 / 60.0);
            if tick >= 60 {
                off += angle_between(straight, player.yaw) / (60.0 * 10.0);
            }
        }
        // Yaw grows to the left, so aiming right of them is a smaller yaw.
        // Running 7 m/s, a bullet's 0.27 s flight ahead is 0.047 rad.
        assert!(off < -0.02, "not leading: {off}");
    }

    #[test]
    fn bots_holding_fire_never_shoot() {
        let (fired, hurt) = bot_in_front_of_a_person(true);
        assert!(!fired && !hurt, "the bot fired anyway");
    }

    #[test]
    fn only_the_leader_tells_the_bots_to_hold_fire() {
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
        server.handle_message(other, ClientMsg::SetBotsHoldFire(true));
        assert!(!server.settings.bots_hold_fire, "only the leader may");
        server.handle_message(leader, ClientMsg::SetBotsHoldFire(true));
        let ServerMsg::Roster { bots_hold_fire, .. } = server.roster_msg() else {
            panic!("not a roster");
        };
        assert!(bots_hold_fire);
        server.start_match();
        server.handle_message(leader, ClientMsg::SetBotsHoldFire(false));
        assert!(server.settings.bots_hold_fire, "not during a match");
    }

    #[test]
    fn only_the_leader_picks_how_well_bots_fight() {
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
        assert_eq!(server.settings.bot_difficulty, BOT_NORMAL);
        server.handle_message(other, ClientMsg::SetBotDifficulty(BOT_HARD));
        assert_eq!(server.settings.bot_difficulty, BOT_NORMAL, "only the leader may");
        server.handle_message(leader, ClientMsg::SetBotDifficulty(BOT_DIFFICULTY_NAMES.len() as u8));
        assert_eq!(server.settings.bot_difficulty, BOT_NORMAL, "no such level");
        server.handle_message(leader, ClientMsg::SetBotDifficulty(BOT_EASY));
        let ServerMsg::Roster { bot_difficulty, .. } = server.roster_msg() else {
            panic!("not a roster");
        };
        assert_eq!(bot_difficulty, BOT_EASY);
        server.start_match();
        server.handle_message(leader, ClientMsg::SetBotDifficulty(BOT_HARD));
        assert_eq!(server.settings.bot_difficulty, BOT_EASY, "not during a match");
    }

    /// Each level is at least as good as the one below it, in every way.
    #[test]
    fn harder_bots_are_better_at_everything() {
        for pair in BOT_SKILLS.windows(2) {
            let (worse, better) = (&pair[0], &pair[1]);
            assert!(better.reaction <= worse.reaction && better.reaction_jitter <= worse.reaction_jitter);
            assert!(better.spot_time <= worse.spot_time && better.turn_rate >= worse.turn_rate);
            assert!(better.aim_error <= worse.aim_error && better.far_doubling >= worse.far_doubling);
            assert!(better.aim_slop <= worse.aim_slop && better.settle <= worse.settle);
            assert!(better.easy_track >= worse.easy_track && better.tracking <= worse.tracking);
            assert!(better.lead.1 - better.lead.0 <= worse.lead.1 - worse.lead.0);
        }
    }

    /// Five seconds of a bot facing a person 12 m away across open ground,
    /// both held in place: whether it fired, and whether it hurt them.
    fn bot_in_front_of_a_person(hold_fire: bool) -> (bool, bool) {
        let settings = ServerSettings {
            port: 0,
            bots: 1,
            bots_hold_fire: hold_fire,
            ..ServerSettings::default()
        };
        let mut server = Server::bind(settings).unwrap();
        server.join_counter += 1;
        let human = server.new_player("Target".into(), None);
        server.players.insert(1000, human);
        server.start_match();
        server.countdown = 0.0;
        let bot = server.players.iter().find(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).unwrap();
        // A spawn point with open ground 12 m in some direction.
        let (bot_pos, target_pos) = server
            .map
            .spawns
            .iter()
            .flat_map(|spawn| {
                (0..16).map(|step| {
                    let yaw = step as f32 * std::f32::consts::TAU / 16.0;
                    (spawn.pos, spawn.pos + map::look_dir(yaw, 0.0) * 12.0)
                })
            })
            .find(|(from, to)| {
                let eye = from.with_y(from.y + 0.7);
                server.map.is_free(*to, map::PLAYER_HALF)
                    && server.map.line_of_sight(eye, aim_point(*to))
                    && server.map.line_of_sight(eye, *to - Vec3::Y * 0.5)
            })
            .expect("no open line on the map");
        let to = target_pos - bot_pos;
        server.players.get_mut(&bot).unwrap().yaw = (-to.x).atan2(-to.z);
        let (mut fired, mut hurt) = (false, false);
        for _ in 0..60 * 5 {
            // Both stay put: the bot may turn and shoot but not walk away.
            server.players.get_mut(&bot).unwrap().pos = bot_pos;
            server.players.get_mut(&1000).unwrap().pos = target_pos;
            server.update_match(1.0 / 60.0);
            fired |= !server.bullets.is_empty();
            let target = &server.players[&1000];
            hurt |= target.armor < START_ARMOR || target.deaths > 0;
        }
        (fired, hurt)
    }

    /// A server with one person in a running match (the countdown over),
    /// carrying `grenade`, and that person's id.
    fn match_with_one_person_carrying(grenade: GrenadeKind) -> (Server, PlayerId) {
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

    fn match_with_one_person() -> (Server, PlayerId) {
        match_with_one_person_carrying(FRAG)
    }

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
