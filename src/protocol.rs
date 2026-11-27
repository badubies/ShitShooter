//! Wire protocol shared by the client and the server.

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub use crate::course::RunEvent;
use crate::weapons::{SightId, WeaponId};

pub const DEFAULT_PORT: u16 = 7777;
pub const MAX_PLAYERS: usize = 16;
/// Most players on one team: half the lobby, so both teams fit the lobby's
/// side-by-side slots.
pub const TEAM_SIZE: usize = MAX_PLAYERS / 2;
pub const PLAYER_COLOR_COUNT: u8 = 8;

/// renet channel ids (see `renet::DefaultChannel`).
pub const CH_UNRELIABLE: u8 = 0;
pub const CH_RELIABLE: u8 = 2;

// Gameplay constants both sides must agree on.
pub const MAX_HEALTH: i32 = 100;
/// Health comes back after this many seconds without being hurt...
pub const REGEN_DELAY: f32 = 5.0;
/// ...at this many points per second.
pub const REGEN_RATE: f32 = 20.0;
/// Seconds dead before being put back in by the server...
pub const RESPAWN_DELAY: f32 = 10.0;
/// ...unless the player asks sooner (`ClientMsg::Respawn`), which counts only
/// after this long, so a shot fired as you die does not skip the death screen.
pub const MIN_RESPAWN_DELAY: f32 = 0.5;
/// Armor soaks up damage before health does. Every life starts with
/// `START_ARMOR`; pickups on the ground add `ARMOR_PICKUP`, up to `MAX_ARMOR`.
/// Armor does not come back by itself.
pub const START_ARMOR: i32 = 25;
pub const MAX_ARMOR: i32 = 50;
pub const ARMOR_PICKUP: i32 = 25;
/// Seconds before a taken armor pickup is back.
pub const ARMOR_RESPAWN: f32 = 20.0;
pub const DEFAULT_FRAG_LIMIT: u32 = 20;
pub const DEFAULT_MATCH_MINUTES: f32 = 10.0;
pub const SNAPSHOT_RATE: f32 = 30.0;
/// Seconds between everybody spawning and the match starting. Nobody can
/// move, shoot or throw until it has run out, and the clock does not run.
pub const COUNTDOWN: f32 = 3.0;
/// Seconds a ping stays on teammates' screens (less if its enemy dies).
pub const PING_TIME: f32 = 5.0;
/// How quickly strafing in the air turns into speed, like Source's
/// `sv_airaccelerate`. The host picks one of these in the lobby, and every
/// player moves with it (movement runs on each client).
pub const AIR_ACCEL_CHOICES: [u16; 4] = [10, 20, 30, 100];
/// Picked in play (discussion #5): a little more than CS's 10.
pub const DEFAULT_AIR_ACCEL: u16 = 20;

/// The air acceleration after `current` in `AIR_ACCEL_CHOICES`, going round.
pub fn next_air_accel(current: u16) -> u16 {
    let here = AIR_ACCEL_CHOICES.iter().position(|&choice| choice == current);
    AIR_ACCEL_CHOICES[here.map_or(0, |i| (i + 1) % AIR_ACCEL_CHOICES.len())]
}

// The game modes are in `modes`: their ids, what each is like, and each
// one's own constants. They are passed on here so that `protocol::*` still
// brings every number both sides must agree on.
#[allow(unused_imports)]
pub use crate::modes::{
    Guns, Kit, MODE_CONQUEST, MODE_FFA, MODE_HILL, MODE_RACE, MODE_TEAMS, MODE_TRIAL, ModeSnap, ModeStats,
    conquest::{CAPTURE_CROWD, CAPTURE_TIME, CONQUEST_TICKETS, FLAG_HEIGHT, TICKET_BLEED},
    course::{RACE_END_DELAY, RACE_FINISH_GRACE},
    hill::{HILL_CONTESTED, HILL_EMPTY, HILL_HELD, HILL_RADIUS, HILL_SCORE_LIMIT},
    is_course_mode, is_team_mode, score_limit,
};
/// How well the bots fight (see `ClientMsg::SetBotDifficulty`).
pub const BOT_EASY: u8 = 0;
pub const BOT_NORMAL: u8 = 1;
pub const BOT_HARD: u8 = 2;
pub const BOT_DIFFICULTY_NAMES: [&str; 3] = ["Easy", "Normal", "Hard"];
/// The kinds of grenade, indexes into `GRENADES`.
pub type GrenadeKind = u8;
pub const FRAG: GrenadeKind = 0;
pub const FLASH: GrenadeKind = 1;
pub const SMOKE: GrenadeKind = 2;
pub const GRENADE_KINDS: usize = 3;

/// Every player carries one kind of grenade, picked in their loadout.
pub struct GrenadeStats {
    pub name: &'static str,
    /// Carried at most. One comes back every `recharge` seconds.
    pub max: f32,
    pub recharge: f32,
    /// Goes off as soon as it hits something, instead of on a timer.
    pub impact: bool,
    /// Timed grenades: seconds from pulling the pin to it going off. Holding
    /// on to it (cooking) uses some of that up before the throw. Impact
    /// grenades: the longest one flies before going off anyway.
    pub fuse: f32,
}

pub const GRENADES: [GrenadeStats; GRENADE_KINDS] = [
    GrenadeStats {
        name: "Frag",
        max: 2.0,
        recharge: 12.0,
        impact: false,
        fuse: 2.2,
    },
    GrenadeStats {
        name: "Flash",
        max: 2.0,
        recharge: 15.0,
        impact: true,
        fuse: 5.0,
    },
    GrenadeStats {
        name: "Smoke",
        max: 1.0,
        recharge: 20.0,
        impact: true,
        fuse: 5.0,
    },
];

pub fn grenade(kind: GrenadeKind) -> &'static GrenadeStats {
    &GRENADES[kind as usize % GRENADE_KINDS]
}

/// The most grenades of any kind somebody can carry.
pub const MOST_GRENADES: usize = 2;

pub const GRENADE_THROW_SPEED: f32 = 27.0;
/// Right-click throws are lobbed gently, this much as hard.
pub const GRENADE_LOB: f32 = 0.35;
/// Frag damage at the centre of the blast, falling off to nothing at `GRENADE_RADIUS`.
pub const GRENADE_DAMAGE: f32 = 130.0;
pub const GRENADE_RADIUS: f32 = 7.0;
/// A flashbang blinds anyone who can see it within this many metres...
pub const FLASH_RANGE: f32 = 30.0;
/// ...for up to this many seconds (right next to it, looking at it).
pub const FLASH_BLIND: f32 = 4.0;
/// A smoke cloud is about this big around, and lasts this many seconds.
pub const SMOKE_RADIUS: f32 = 4.5;
pub const SMOKE_TIME: f32 = 14.0;
/// The cloud sits this high above where the grenade went off.
pub const SMOKE_RISE: f32 = 1.2;
/// Grenades are simulated in steps of exactly this long on the server and on
/// every client, so they all see the same bounces.
pub const GRENADE_STEP: f32 = 1.0 / 60.0;

/// Bits of `ClientState::flags` and `PlayerSnap::flags`.
pub const FLAG_CROUCH: u8 = 1;
pub const FLAG_RELOAD: u8 = 2;
/// Aiming down sights (so a teammate watching through their eyes sees it).
pub const FLAG_AIM: u8 = 4;

pub type PlayerId = u64;

/// The lobby password is folded into the netcode protocol id, so a client with
/// the wrong password (or a different game version) is simply never accepted.
pub fn protocol_id(password: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in b"shitshooter-proto-24:".iter().chain(password.as_bytes()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: PlayerId,
    pub name: String,
    /// Index into `paint::PALETTE`: the colour of their plain coat, worn
    /// until their paint job arrives (bots always wear it).
    pub color: u8,
    /// 0 or 1. Only means something in the team modes.
    pub team: u8,
    /// What they are in a mode where players are not all alike; each such
    /// mode has its own numbers for it. 0 everywhere else.
    pub role: u8,
    pub kills: u32,
    pub deaths: u32,
    /// Kills somebody else finished after this player hurt the victim.
    pub assists: u32,
    /// Health and armor taken off other players this match.
    pub damage: u32,
    pub is_bot: bool,
    /// What only the mode being played keeps of them.
    pub stats: ModeStats,
}

impl PlayerInfo {
    /// The racing modes: a race's finishing time, or the best time of a
    /// time trial, once there is one.
    pub fn time(&self) -> Option<f32> {
        match self.stats {
            ModeStats::Course { time, .. } => time,
            _ => None,
        }
    }

    /// The racing modes: how many checkpoints they are through (in a time
    /// trial, on the run they are on).
    pub fn gates(&self) -> u8 {
        match self.stats {
            ModeStats::Course { gates, .. } => gates,
            _ => 0,
        }
    }
}

/// Client -> server, reliable ordered.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClientMsg {
    Hello { name: String },
    /// The grenade we want to carry, from our next spawn on. Sent after
    /// `Hello` and whenever it is changed.
    Loadout { grenade: GrenadeKind },
    /// What we look like: a `paint::Canvas`, encoded. Sent after `Hello` and
    /// whenever it is changed; the server passes it on to everyone.
    Paint { paint: Vec<u8> },
    /// Lobby leader only.
    StartMatch,
    /// Lobby leader only.
    AddBot,
    /// Lobby leader only: takes out the bot that joined last.
    RemoveBot,
    /// Lobby leader only, in the lobby: switch to the next map.
    NextMap,
    /// Lobby leader only, in the lobby: switch to the next game mode.
    NextMode,
    /// Lobby leader only, in the lobby: switch to the next of `AIR_ACCEL_CHOICES`.
    NextAirAccel,
    /// Lobby leader only, in the lobby: play this game mode, on the map if it
    /// is played there, or else on the first map it is.
    SetMode(u8),
    /// Lobby leader only, in the lobby: play on this map, if the game mode is
    /// played there.
    SetMap(u8),
    /// Lobby leader only, in the lobby: one of `AIR_ACCEL_CHOICES`.
    SetAirAccel(u16),
    /// Lobby leader only, in the lobby: whether the bots hold their fire.
    /// They still move about and turn to their enemies, but never shoot.
    SetBotsHoldFire(bool),
    /// Lobby leader only, in the lobby: how well the bots fight, an index
    /// into `BOT_DIFFICULTY_NAMES`.
    SetBotDifficulty(u8),
    /// In the lobby: move yourself to the other team.
    SwitchTeam,
    /// Lobby leader only: abort the running match and return to the lobby.
    EndMatch,
    /// A weapon was fired: one bullet per entry of `dirs` (none for the knife).
    /// The server passes it on so that everyone else sees and hears it.
    Fire {
        weapon: WeaponId,
        origin: [f32; 3],
        dirs: Vec<[f32; 3]>,
    },
    /// One of our bullets (or our knife) reached somebody. The shooter decides
    /// what it hit, because that feels right at any ping; the server only
    /// rejects claims that are plainly impossible.
    Hit {
        victim: PlayerId,
        weapon: WeaponId,
        /// It struck the head (never for the knife).
        headshot: bool,
    },
    /// Pull the pin of a frag: its fuse starts burning in our hand. If it is
    /// held too long it goes off there; if we die holding it, it drops.
    Cook,
    /// Throw a grenade: the frag being cooked, or a flash or smoke. The
    /// server simulates it and deals the damage.
    Grenade {
        kind: GrenadeKind,
        origin: [f32; 3],
        vel: [f32; 3],
    },
    /// Point something out to the team: an enemy (`target`), or a spot.
    /// Only teammates are told.
    Ping {
        target: Option<PlayerId>,
        pos: [f32; 3],
    },
    /// While dead: put us back in now rather than at the end of `RESPAWN_DELAY`.
    Respawn,
    /// Conquest: where to come back in from now on. `None`: wherever the
    /// server thinks best; 0: our base; `1 + i`: the map's flag `i`, while
    /// our team holds it (else the base).
    SpawnAt { point: Option<u8> },
    /// The racing modes: what just happened on our run, as we timed it.
    Run(RunEvent),
}

/// Client -> server, unreliable, sent every fixed tick while alive.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct ClientState {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
    /// The weapon in hand.
    pub weapon: WeaponId,
    /// The sight on it.
    pub sight: SightId,
    /// Number of spawns this client has processed. Lets the server ignore stale
    /// positions that were sent before a respawn teleport arrived.
    pub spawn_seq: u32,
}

/// Server -> client, reliable ordered.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ServerMsg {
    Welcome {
        id: PlayerId,
        /// The maps made in Blender this server has (`custom.rs`), in
        /// order: each one's name, and the modes it is played in.
        maps: Vec<(String, Vec<u8>)>,
    },
    /// The file of map `map`, one made in Blender (`custom.rs`), sent before
    /// it is played and again whenever the host's copy changes.
    MapFile {
        map: u8,
        data: Vec<u8>,
    },
    Roster {
        players: Vec<PlayerInfo>,
        leader: PlayerId,
        /// Index into `map::MAP_NAMES`, or past it (`map::BUILT_IN`).
        map: u8,
        /// One of the `MODE_*` constants.
        mode: u8,
        /// See `AIR_ACCEL_CHOICES`.
        air_accel: u16,
        /// The bots never shoot (see `ClientMsg::SetBotsHoldFire`).
        bots_hold_fire: bool,
        /// See `ClientMsg::SetBotDifficulty`.
        bot_difficulty: u8,
    },
    MatchStarted {
        /// See `score_limit`.
        score_limit: u32,
        time_left: f32,
        /// Seconds of the starting countdown left (see `COUNTDOWN`).
        countdown: f32,
        /// Seconds played since it ended (a race's clock).
        elapsed: f32,
    },
    MatchEnded {
        winner: Winner,
        /// The team scores it ended on, which the last snapshot may not have
        /// had yet (a winning kill ends the match before the next one).
        team_scores: [u32; 2],
    },
    Spawn {
        id: PlayerId,
        pos: [f32; 3],
        yaw: f32,
        spawn_seq: u32,
        /// The grenade they carry this life, and start with `max` of.
        grenade: GrenadeKind,
        /// What they have this life: guns, health, how they move.
        kit: Kit,
    },
    /// Somebody else fired. Sent to everyone but the shooter.
    Fire {
        shooter: PlayerId,
        weapon: WeaponId,
        origin: [f32; 3],
        dirs: Vec<[f32; 3]>,
    },
    Hit {
        victim: PlayerId,
        attacker: PlayerId,
        health: i32,
        armor: i32,
        /// This hit took the victim's last armor.
        armor_broken: bool,
    },
    /// Somebody picked up the armor at `map.armor_spots[spot]`.
    ArmorPickup {
        id: PlayerId,
        spot: u8,
        armor: i32,
    },
    Kill {
        killer: PlayerId,
        victim: PlayerId,
        /// What did it; `weapons::GRENADE` for a grenade.
        weapon: WeaponId,
    },
    Grenade {
        id: u32,
        kind: GrenadeKind,
        pos: [f32; 3],
        vel: [f32; 3],
        /// Seconds until it goes off.
        fuse: f32,
    },
    Explosion {
        /// The grenade that went off (or one never thrown, gone off in hand).
        id: u32,
        kind: GrenadeKind,
        pos: [f32; 3],
    },
    /// A teammate pinged an enemy (`target`) or a spot. Sent only to the
    /// pinger's team.
    Ping {
        from: PlayerId,
        target: Option<PlayerId>,
        pos: [f32; 3],
    },
    /// What player `id` looks like (see `ClientMsg::Paint`). A newcomer is
    /// sent everyone's after the roster.
    Paint {
        id: PlayerId,
        paint: Vec<u8>,
    },
    /// The racing modes: a run's checkpoint or finish, taken by the server.
    Run {
        id: PlayerId,
        event: RunEvent,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Winner {
    Nobody,
    Player(PlayerId),
    Team(u8),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct PlayerSnap {
    pub id: PlayerId,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
    pub weapon: WeaponId,
    /// The sight on the weapon in hand.
    pub sight: SightId,
    pub health: i32,
    pub armor: i32,
    pub alive: bool,
}

/// One conquest flag, as the snapshot tells it.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct FlagSnap {
    /// The team holding it, if any.
    pub owner: Option<u8>,
    /// Who it is leaning to, -100 (all team 0's) to 100 (all team 1's).
    pub control: i8,
}

impl FlagSnap {
    /// -1 (team 0's) to 1 (team 1's).
    pub fn control(&self) -> f32 {
        self.control as f32 / 100.0
    }
}

/// Server -> client, unreliable, `SNAPSHOT_RATE` times per second during a match.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snapshot {
    pub time_left: f32,
    /// Team modes: each team's score (conquest: tickets left).
    pub team_scores: [u32; 2],
    /// What only the mode being played has to tell: who holds the hill, the flags.
    pub mode: ModeSnap,
    /// Bit `i` is set while the armor at `map.armor_spots[i]` is taken.
    pub armor_taken: u32,
    pub players: Vec<PlayerSnap>,
}

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    postcard::to_stdvec(value).expect("message serialization cannot fail")
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    postcard::from_bytes(bytes).ok()
}
