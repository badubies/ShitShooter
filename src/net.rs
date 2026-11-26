//! Client-side networking: the connection to the server (ours or someone
//! else's) and the session state that the server tells us about.

use std::{
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::prelude::*;
use renet::{ConnectionConfig, RenetClient};
use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

use crate::{
    AppState, hub,
    audio::{Sound, SoundCue},
    map::Map,
    paint::MyPaint,
    protocol::*,
    server::{Server, ServerHandle, ServerSettings},
    settings::Settings,
    world::GameMap,
};

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Session>()
            .init_resource::<MenuStatus>()
            .add_message::<NetMsg>()
            .add_message::<SnapshotMsg>()
            .add_systems(
                PreUpdate,
                // Checked before each system, not once for the group: `receive`
                // drops the connection when it fails, and the rest must then
                // skip this frame instead of asking for a connection that is gone.
                (receive, send_loadout, send_paint, update_session)
                    .chain()
                    .distributive_run_if(resource_exists::<Connection>),
            )
            .add_systems(PostUpdate, flush.run_if(resource_exists::<Connection>));
    }
}

/// A reliable message from the server.
#[derive(Message)]
pub struct NetMsg(pub ServerMsg);

/// A world snapshot from the server.
#[derive(Message)]
pub struct SnapshotMsg(pub Snapshot);

/// Error or info line shown on the main menu.
#[derive(Resource, Default)]
pub struct MenuStatus(pub String);

/// The server running inside this process when we are the host.
#[derive(Resource)]
pub struct HostedServer(#[allow(dead_code)] ServerHandle);

/// What we know about the lobby/match we are connected to.
#[derive(Resource, Default)]
pub struct Session {
    pub my_id: PlayerId,
    pub leader: PlayerId,
    pub roster: Vec<PlayerInfo>,
    /// One of the `MODE_*` constants.
    pub mode: u8,
    /// The host's air acceleration (see `AIR_ACCEL_CHOICES`); 0 until told.
    pub air_accel: u16,
    /// The bots never shoot (see `ClientMsg::SetBotsHoldFire`).
    pub bots_hold_fire: bool,
    /// How well the bots fight (see `ClientMsg::SetBotDifficulty`).
    pub bot_difficulty: u8,
    /// The score that wins: kills, team kills or seconds on the hill.
    pub score_limit: u32,
    pub team_scores: [u32; 2],
    /// Who holds the hill (`HILL_*`).
    pub hill: u8,
    /// Conquest: each of the map's flags.
    pub flags: Vec<FlagSnap>,
    /// Bit `i` is set while the armor at `map.armor_spots[i]` is taken.
    pub armor_taken: u32,
    pub time_left: f32,
    /// Seconds of the starting countdown left: nobody can move or shoot while
    /// it is above 0. It keeps going down to -1, so "GO" can show for a moment.
    pub countdown: f32,
    /// Result line of the previous match, shown in the lobby.
    pub last_result: Option<String>,
    /// The scoreboard the previous match ended on, which the lobby can show.
    pub last_match: Option<FinalScores>,
    /// Address to tell friends about, when hosting.
    pub share_address: Option<String>,
    pub server_label: String,
}

/// The scoreboard as a match ended: everybody who played it, as they were,
/// even if they leave or switch teams in the lobby afterwards.
#[derive(Clone)]
pub struct FinalScores {
    pub roster: Vec<PlayerInfo>,
    pub team_scores: [u32; 2],
    /// One of the `MODE_*` constants.
    pub mode: u8,
    pub map: u8,
    /// Our team then.
    pub my_team: u8,
}

impl Session {
    /// How quickly strafing in the air turns into speed (see `AIR_ACCEL_CHOICES`).
    pub fn air_accel(&self) -> f32 {
        if self.air_accel == 0 { DEFAULT_AIR_ACCEL } else { self.air_accel }.into()
    }

    pub fn is_leader(&self) -> bool {
        self.my_id != 0 && self.my_id == self.leader
    }

    pub fn name_of(&self, id: PlayerId) -> &str {
        self.roster
            .iter()
            .find(|p| p.id == id)
            .map_or("???", |p| p.name.as_str())
    }

    pub fn team_mode(&self) -> bool {
        is_team_mode(self.mode)
    }

    /// The starting countdown is running.
    pub fn frozen(&self) -> bool {
        self.countdown > 0.0
    }

    pub fn team_of(&self, id: PlayerId) -> u8 {
        self.roster.iter().find(|p| p.id == id).map_or(0, |p| p.team & 1)
    }

    /// True if `a` and `b` can hurt each other: two different players who are
    /// not on the same team.
    pub fn enemies(&self, a: PlayerId, b: PlayerId) -> bool {
        a != b && !(self.team_mode() && self.team_of(a) == self.team_of(b))
    }
}

#[derive(Resource)]
pub struct Connection {
    client: RenetClient,
    transport: NetcodeClientTransport,
    name: String,
    said_hello: bool,
    /// The grenade the server was last told we carry.
    loadout_sent: Option<GrenadeKind>,
    /// The `MyPaint::version` the server was last sent.
    paint_sent: Option<u32>,
}

impl Connection {
    pub fn open(server_addr: SocketAddr, password: &str, name: &str) -> Result<Connection, String> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| e.to_string())?;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let authentication = ClientAuthentication::Unsecure {
            protocol_id: protocol_id(password),
            // Client ids double as player ids; a clock-derived id is unique enough for a lobby.
            client_id: (now.as_nanos() as u64 ^ (std::process::id() as u64).rotate_left(40)) | (1 << 32),
            server_addr,
            user_data: None,
        };
        let transport =
            NetcodeClientTransport::new(now, authentication, socket).map_err(|e| e.to_string())?;
        Ok(Connection {
            client: RenetClient::new(ConnectionConfig::default()),
            transport,
            name: name.to_string(),
            said_hello: false,
            loadout_sent: None,
            paint_sent: None,
        })
    }

    pub fn send(&mut self, msg: &ClientMsg) {
        self.client.send_message(CH_RELIABLE, encode(msg));
    }

    pub fn send_state(&mut self, state: &ClientState) {
        self.client.send_message(CH_UNRELIABLE, encode(state));
    }

    /// Round trip time in milliseconds.
    pub fn ping_ms(&self) -> f64 {
        self.client.rtt() * 1000.0
    }
}

/// Starts a server in this process and connects to it.
pub fn host(commands: &mut Commands, settings: ServerSettings, name: &str) -> Result<(), String> {
    let port = settings.port;
    let password = settings.password.clone();
    let server = Server::bind(settings)
        .map_err(|e| format!("Could not host on port {port}: {e}"))?;
    let connection = Connection::open(SocketAddr::from((Ipv4Addr::LOCALHOST, port)), &password, name)?;
    commands.insert_resource(HostedServer(server.spawn_thread()));
    commands.insert_resource(connection);
    commands.insert_resource(Session {
        share_address: Some(match lan_ip() {
            Some(ip) => format!("{ip}:{port}"),
            None => format!("<your ip>:{port}"),
        }),
        server_label: "your own lobby".to_string(),
        ..default()
    });
    Ok(())
}

/// Connects to somebody else's server. `address` may omit the port.
pub fn join(commands: &mut Commands, address: &str, password: &str, name: &str) -> Result<(), String> {
    let address = address.trim();
    let server_addr = hub::resolve(address, DEFAULT_PORT)?;
    let label = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:{DEFAULT_PORT}")
    };
    connect(commands, server_addr, label, password, name)
}

/// Connects to a lobby whose address is known; `label` is how it is shown.
pub fn connect(
    commands: &mut Commands,
    server_addr: SocketAddr,
    label: String,
    password: &str,
    name: &str,
) -> Result<(), String> {
    commands.insert_resource(Connection::open(server_addr, password, name)?);
    commands.insert_resource(Session {
        server_label: label,
        ..default()
    });
    Ok(())
}

/// Closes the connection (and the hosted server, if any).
pub fn disconnect(commands: &mut Commands, connection: Option<&mut Connection>) {
    if let Some(connection) = connection {
        connection.transport.disconnect();
    }
    commands.remove_resource::<Connection>();
    commands.remove_resource::<HostedServer>();
}

/// The address other machines on the local network can reach us at.
/// "Connecting" a UDP socket sends nothing; it only makes the OS pick a route.
fn lan_ip() -> Option<String> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

fn receive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut connection: ResMut<Connection>,
    mut reliable: MessageWriter<NetMsg>,
    mut snapshots: MessageWriter<SnapshotMsg>,
    mut status: ResMut<MenuStatus>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let connection = &mut *connection;
    let delta = time.delta();
    connection.client.update(delta);
    let transport_result = connection.transport.update(delta, &mut connection.client);

    if transport_result.is_err() || connection.client.is_disconnected() {
        let never_connected = !connection.said_hello;
        status.0 = if never_connected {
            "Could not connect. Check the address, the password, and that the host's UDP port is reachable.".into()
        } else {
            "Disconnected from the server.".into()
        };
        disconnect(&mut commands, Some(connection));
        next_state.set(AppState::Menu);
        return;
    }
    if !connection.client.is_connected() {
        return;
    }
    if !connection.said_hello {
        connection.said_hello = true;
        let name = connection.name.clone();
        connection.send(&ClientMsg::Hello { name });
    }

    while let Some(bytes) = connection.client.receive_message(CH_RELIABLE) {
        if let Some(msg) = decode::<ServerMsg>(&bytes) {
            reliable.write(NetMsg(msg));
        }
    }
    // Only the newest snapshot matters.
    let mut latest = None;
    while let Some(bytes) = connection.client.receive_message(CH_UNRELIABLE) {
        latest = decode::<Snapshot>(&bytes).or(latest);
    }
    if let Some(snapshot) = latest {
        snapshots.write(SnapshotMsg(snapshot));
    }
}

/// Tells the server which grenade we carry: once we are in, and whenever
/// it is changed in the loadout.
fn send_loadout(settings: Res<Settings>, mut connection: ResMut<Connection>) {
    if connection.said_hello && connection.loadout_sent != Some(settings.grenade) {
        connection.loadout_sent = Some(settings.grenade);
        connection.send(&ClientMsg::Loadout {
            grenade: settings.grenade,
        });
    }
}

fn send_paint(paint: Res<MyPaint>, mut connection: ResMut<Connection>) {
    if connection.said_hello && connection.paint_sent != Some(paint.version) {
        connection.paint_sent = Some(paint.version);
        connection.send(&ClientMsg::Paint {
            paint: paint.canvas.encode(),
        });
    }
}

fn update_session(
    time: Res<Time>,
    mut messages: MessageReader<NetMsg>,
    mut snapshots: MessageReader<SnapshotMsg>,
    mut session: ResMut<Session>,
    mut game_map: ResMut<GameMap>,
    mut next_state: ResMut<NextState<AppState>>,
    mut sounds: MessageWriter<SoundCue>,
) {
    // A beep for each second of the countdown, and a higher one for the start.
    if session.countdown > -1.0 {
        let before = session.countdown;
        session.countdown = (before - time.delta_secs()).max(-1.0);
        let after = session.countdown;
        if before > 0.0 && before.ceil() != after.ceil() {
            sounds.write(SoundCue {
                sound: if after <= 0.0 { Sound::Go } else { Sound::Countdown },
                at: None,
            });
        }
    }
    for NetMsg(msg) in messages.read() {
        match msg {
            ServerMsg::Welcome { id } => {
                session.my_id = *id;
                next_state.set(AppState::Lobby);
            }
            ServerMsg::Roster {
                players,
                leader,
                map,
                mode,
                air_accel,
                bots_hold_fire,
                bot_difficulty,
            } => {
                session.roster = players.clone();
                session.leader = *leader;
                session.mode = *mode;
                session.air_accel = *air_accel;
                session.bots_hold_fire = *bots_hold_fire;
                session.bot_difficulty = *bot_difficulty;
                // The host picked another map, or went into or out of conquest
                // (the flagpoles): swap the scenery and the collision.
                if game_map.0.id != *map || game_map.0.has_flagpoles() != (*mode == MODE_CONQUEST) {
                    game_map.0 = Map::for_game(*map, *mode);
                }
            }
            ServerMsg::MatchStarted {
                score_limit,
                time_left,
                countdown,
            } => {
                // Joining a match already under way: no countdown, and no "GO".
                session.countdown = if *countdown > 0.0 { *countdown } else { -1.0 };
                if session.frozen() {
                    sounds.write(SoundCue {
                        sound: Sound::Countdown,
                        at: None,
                    });
                }
                session.score_limit = *score_limit;
                session.team_scores = [0, 0];
                session.hill = HILL_EMPTY;
                session.flags.clear();
                session.armor_taken = 0;
                session.time_left = *time_left;
                next_state.set(AppState::Playing);
            }
            ServerMsg::MatchEnded { winner, team_scores } => {
                // The roster that came just before this has everybody's final
                // kills and deaths.
                session.team_scores = *team_scores;
                session.last_match = Some(FinalScores {
                    roster: session.roster.clone(),
                    team_scores: *team_scores,
                    mode: session.mode,
                    map: game_map.0.id,
                    my_team: session.team_of(session.my_id),
                });
                session.last_result = Some(match winner {
                    Winner::Player(id) => format!("{} won the last match!", session.name_of(*id)),
                    Winner::Team(team) => {
                        let (won, lost) = (
                            session.team_scores[*team as usize & 1],
                            session.team_scores[!*team as usize & 1],
                        );
                        if *team & 1 == session.team_of(session.my_id) {
                            format!("Your team won the last match, {won} to {lost}!")
                        } else {
                            format!("The enemy team won the last match, {won} to {lost}.")
                        }
                    }
                    Winner::Nobody => "The last match ended without a winner.".to_string(),
                });
                session.armor_taken = 0;
                session.countdown = -1.0;
                next_state.set(AppState::Lobby);
            }
            // Hidden at once; the next snapshot says the same.
            ServerMsg::ArmorPickup { spot, .. } => session.armor_taken |= 1 << (*spot as u32 % 32),
            _ => {}
        }
    }
    for SnapshotMsg(snapshot) in snapshots.read() {
        session.time_left = snapshot.time_left;
        session.team_scores = snapshot.team_scores;
        session.hill = snapshot.hill;
        session.flags.clone_from(&snapshot.flags);
        session.armor_taken = snapshot.armor_taken;
    }
}

fn flush(mut connection: ResMut<Connection>) {
    let connection = &mut *connection;
    // Errors surface as a disconnect on the next `receive`.
    let _ = connection.transport.send_packets(&mut connection.client);
}
