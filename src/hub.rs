//! The lobby server ("hub"): one always-on process, say on a Raspberry Pi,
//! that players ask for its list of lobbies, or to open a lobby of their own.
//!
//! Every lobby is an ordinary game server (`server.rs`) on its own thread and
//! UDP port: the hub listens on `--port` (7777) and gives lobbies the ports
//! right after it (7778, 7779...). A lobby closes once nobody has been in it
//! for `IDLE_CLOSE`.
//!
//! The hub speaks plain UDP: a request is one datagram, and so is its answer.
//! Both start with `MAGIC`, the game version (`hub_version`) and the number of
//! the request, followed by the message itself, postcard-encoded. A hub of
//! another game version answers with that header alone, so the game can say
//! so instead of failing to read the answer.

use std::{
    collections::HashMap,
    fmt, io,
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use serde::{Deserialize, Serialize};

use crate::{
    protocol::{decode, encode, protocol_id},
    server::{LobbyStatus, Server, ServerHandle, ServerSettings, sanitize_name},
};

const MAGIC: [u8; 4] = *b"SShb";
/// `MAGIC`, then the version (8 bytes) and the request number (4 bytes).
const HEADER: usize = 4 + 8 + 4;
/// Longest datagram either side sends; a full lobby list stays well under it.
const MAX_DATAGRAM: usize = 1200;
pub const DEFAULT_MAX_LOBBIES: usize = 8;
/// Most lobbies `--lobbies` allows, so the lobby list fits in one datagram.
pub const MAX_LOBBIES: usize = 32;
/// A lobby closes after this long without a person in it. A new lobby gets
/// this long for whoever asked for it to get in.
const IDLE_CLOSE: Duration = Duration::from_secs(30);
/// How often the game sends a request again while it has no answer...
const RESEND: Duration = Duration::from_millis(400);
/// ...and when it gives up.
const GIVE_UP: Duration = Duration::from_secs(3);

/// Changes whenever the game's network protocol does, like `protocol_id`.
pub fn hub_version() -> u64 {
    protocol_id("")
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum HubRequest {
    List,
    /// Open a lobby. `key` is the `protocol_id` of its password, so the
    /// password itself never goes over the network.
    Create { name: String, key: u64, locked: bool },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum HubReply {
    /// The lobbies with someone in them.
    Lobbies(Vec<LobbyInfo>),
    /// The new lobby, on this port of the hub's address.
    Created { port: u16 },
    /// Why no lobby was opened.
    Refused(String),
}

/// A line of the lobby list.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LobbyInfo {
    pub port: u16,
    /// Who leads it.
    pub host: String,
    pub map: u8,
    pub mode: u8,
    /// People in it, and bots.
    pub players: u8,
    pub bots: u8,
    /// It has a password.
    pub locked: bool,
    /// A match is being played.
    pub playing: bool,
}

fn pack(version: u64, nonce: u32, body: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(HEADER + body.len());
    packet.extend_from_slice(&MAGIC);
    packet.extend_from_slice(&version.to_le_bytes());
    packet.extend_from_slice(&nonce.to_le_bytes());
    packet.extend_from_slice(body);
    packet
}

/// (version, request number, message)
fn unpack(packet: &[u8]) -> Option<(u64, u32, &[u8])> {
    if packet.len() < HEADER || packet[..4] != MAGIC {
        return None;
    }
    let version = u64::from_le_bytes(packet[4..12].try_into().ok()?);
    let nonce = u32::from_le_bytes(packet[12..16].try_into().ok()?);
    Some((version, nonce, &packet[HEADER..]))
}

/// Turns what was typed as the lobby server's address into one to send to:
/// a name or IP, with or without a port.
pub fn resolve(address: &str, default_port: u16) -> Result<SocketAddr, String> {
    let address = address.trim();
    if address.is_empty() {
        return Err("Type in an address first.".to_string());
    }
    let with_port = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:{default_port}")
    };
    with_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.find(SocketAddr::is_ipv4))
        .ok_or_else(|| format!("'{address}' is not a valid address"))
}

// ---------------------------------------------------------------- asking a hub

#[derive(Debug, Clone, PartialEq)]
pub enum AskError {
    /// Nothing came back in time.
    NoAnswer,
    /// The hub runs another version of the game.
    OtherVersion,
    Io(String),
}

impl fmt::Display for AskError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AskError::NoAnswer => write!(f, "the lobby server did not answer"),
            AskError::OtherVersion => write!(f, "the lobby server runs another version of the game"),
            AskError::Io(err) => write!(f, "{err}"),
        }
    }
}

/// A request on its way to a hub. Polled until the answer is in, so the game
/// can wait for it without stopping.
pub struct HubQuery {
    socket: UdpSocket,
    to: SocketAddr,
    nonce: u32,
    packet: Vec<u8>,
    started: Instant,
    sent: Instant,
}

impl HubQuery {
    pub fn send(to: SocketAddr, request: &HubRequest) -> Result<HubQuery, AskError> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).map_err(|e| AskError::Io(e.to_string()))?;
        socket.set_nonblocking(true).map_err(|e| AskError::Io(e.to_string()))?;
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let nonce = nanos ^ std::process::id().rotate_left(16);
        let packet = pack(hub_version(), nonce, &encode(request));
        // A failed send is like a lost one: it is sent again.
        let _ = socket.send_to(&packet, to);
        let now = Instant::now();
        Ok(HubQuery {
            socket,
            to,
            nonce,
            packet,
            started: now,
            sent: now,
        })
    }

    /// The answer, once it is in (or it is clear none is coming).
    pub fn poll(&mut self) -> Option<Result<HubReply, AskError>> {
        let mut buf = [0u8; MAX_DATAGRAM];
        // Errors here are mostly Windows saying an earlier datagram could not
        // be delivered (nothing listening yet): keep waiting.
        while let Ok((len, _)) = self.socket.recv_from(&mut buf) {
            let Some((version, nonce, body)) = unpack(&buf[..len]) else { continue };
            if nonce != self.nonce {
                continue;
            }
            if version != hub_version() {
                return Some(Err(AskError::OtherVersion));
            }
            if let Some(reply) = decode::<HubReply>(body) {
                return Some(Ok(reply));
            }
        }
        if self.started.elapsed() >= GIVE_UP {
            return Some(Err(AskError::NoAnswer));
        }
        if self.sent.elapsed() >= RESEND {
            self.sent = Instant::now();
            let _ = self.socket.send_to(&self.packet, self.to);
        }
        None
    }
}

/// Asks a hub and waits for the answer.
pub fn ask(to: SocketAddr, request: &HubRequest) -> Result<HubReply, AskError> {
    let mut query = HubQuery::send(to, request)?;
    loop {
        if let Some(answer) = query.poll() {
            return answer;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

// ---------------------------------------------------------------- the hub

struct Lobby {
    port: u16,
    status: Arc<Mutex<LobbyStatus>>,
    locked: bool,
    /// Who asked for it, and from where.
    creator: String,
    creator_ip: IpAddr,
    /// When a person was last in it (or when it was opened).
    active: Instant,
    /// Somebody has been in it.
    joined: bool,
    /// Stops the lobby's server when dropped.
    _server: ServerHandle,
}

pub struct Hub {
    socket: UdpSocket,
    /// Settings every new lobby starts with (the port is its own).
    template: ServerSettings,
    max_lobbies: usize,
    lobbies: Vec<Lobby>,
    /// The answer last sent to each address that asked for a lobby, with the
    /// request's number and when. A request that comes again (its answer was
    /// lost) gets the same answer, instead of a second lobby.
    created: HashMap<SocketAddr, (u32, Vec<u8>, Instant)>,
}

impl Hub {
    /// Binds the hub's UDP port, `template.port`.
    pub fn bind(template: ServerSettings, max_lobbies: usize) -> io::Result<Hub> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, template.port))?;
        Ok(Hub {
            socket,
            template,
            max_lobbies: max_lobbies.clamp(1, MAX_LOBBIES),
            lobbies: Vec::new(),
            created: HashMap::new(),
        })
    }

    /// Answers requests and closes lobbies left empty, forever.
    pub fn run(mut self) {
        // Wakes up at least this often to close empty lobbies.
        let _ = self.socket.set_read_timeout(Some(Duration::from_millis(250)));
        let mut buf = [0u8; MAX_DATAGRAM];
        let mut swept = Instant::now();
        loop {
            // Errors are timeouts, or Windows saying an earlier answer could
            // not be delivered: nothing to do about either.
            if let Ok((len, from)) = self.socket.recv_from(&mut buf)
                && let Some(answer) = self.handle(&buf[..len], from)
            {
                let _ = self.socket.send_to(&answer, from);
            }
            if swept.elapsed() >= Duration::from_secs(1) {
                swept = Instant::now();
                self.sweep(swept);
            }
        }
    }

    /// The answer to one datagram, if it deserves one.
    fn handle(&mut self, packet: &[u8], from: SocketAddr) -> Option<Vec<u8>> {
        let (version, nonce, body) = unpack(packet)?;
        if version != hub_version() {
            return Some(pack(hub_version(), nonce, &[]));
        }
        let reply = match decode::<HubRequest>(body)? {
            HubRequest::List => HubReply::Lobbies(self.list()),
            HubRequest::Create { name, key, locked } => {
                if let Some((asked, answer, _)) = self.created.get(&from)
                    && *asked == nonce
                {
                    return Some(answer.clone());
                }
                let reply = self.create(&name, key, locked, from.ip());
                let answer = pack(hub_version(), nonce, &encode(&reply));
                self.created.insert(from, (nonce, answer.clone(), Instant::now()));
                return Some(answer);
            }
        };
        Some(pack(hub_version(), nonce, &encode(&reply)))
    }

    /// The lobbies someone is in. Empty ones are left out: they are about to
    /// close, or their creator has not got in yet (and should be its leader).
    fn list(&self) -> Vec<LobbyInfo> {
        let mut lobbies: Vec<LobbyInfo> = self
            .lobbies
            .iter()
            .filter_map(|lobby| {
                let status = lobby.status.lock().ok()?.clone();
                (status.humans > 0).then(|| LobbyInfo {
                    port: lobby.port,
                    host: if status.leader.is_empty() { lobby.creator.clone() } else { status.leader },
                    map: status.map,
                    mode: status.mode,
                    players: status.humans,
                    bots: status.bots,
                    locked: lobby.locked,
                    playing: status.playing,
                })
            })
            .collect();
        lobbies.sort_by_key(|lobby| lobby.port);
        lobbies
    }

    fn create(&mut self, name: &str, key: u64, locked: bool, from: IpAddr) -> HubReply {
        let name = sanitize_name(name);
        // Whoever asks again before getting into their last lobby could not
        // get in, or changed their mind: that one goes.
        self.close(|lobby| lobby.creator_ip == from && !lobby.joined, "replaced before anyone got in");
        if self.lobbies.len() >= self.max_lobbies {
            return HubReply::Refused(format!(
                "All {} lobbies on this server are in use. Try again later.",
                self.max_lobbies
            ));
        }
        let first = self.template.port as usize + 1;
        let free = (first..first + self.max_lobbies)
            .filter_map(|port| u16::try_from(port).ok())
            .filter(|port| self.lobbies.iter().all(|lobby| lobby.port != *port));
        for port in free {
            let settings = ServerSettings {
                port,
                ..self.template.clone()
            };
            // A port something else on this machine uses: try the next.
            let Ok(mut server) = Server::bind_with_key(settings, key) else {
                continue;
            };
            let status = Arc::new(Mutex::new(LobbyStatus::default()));
            server.report_to(status.clone());
            println!("[hub] opened a lobby for {name} on port {port}");
            self.lobbies.push(Lobby {
                port,
                status,
                locked,
                creator: name,
                creator_ip: from,
                active: Instant::now(),
                joined: false,
                _server: server.spawn_thread(),
            });
            return HubReply::Created { port };
        }
        HubReply::Refused("The server could not open a lobby: its ports are taken.".to_string())
    }

    /// Notes who is in which lobby, and closes the ones left empty too long.
    fn sweep(&mut self, now: Instant) {
        for lobby in &mut self.lobbies {
            if lobby.status.lock().is_ok_and(|status| status.humans > 0) {
                lobby.active = now;
                lobby.joined = true;
            }
        }
        self.close(|lobby| now.duration_since(lobby.active) >= IDLE_CLOSE, "nobody in it");
        self.created
            .retain(|_, (_, _, when)| now.duration_since(*when) < Duration::from_secs(60));
    }

    fn close(&mut self, which: impl Fn(&Lobby) -> bool, why: &str) {
        self.lobbies.retain(|lobby| {
            let close = which(lobby);
            if close {
                println!("[hub] closed the lobby on port {} ({why})", lobby.port);
            }
            !close
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hub on its own thread, on a port nobody else uses.
    fn start_hub(port: u16, max_lobbies: usize) -> SocketAddr {
        let template = ServerSettings {
            port,
            ..ServerSettings::default()
        };
        let hub = Hub::bind(template, max_lobbies).expect("hub port in use");
        std::thread::spawn(move || hub.run());
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    }

    #[test]
    fn a_hub_opens_lobbies_and_lists_only_those_with_people() {
        let hub = start_hub(17_100, 2);
        assert_eq!(ask(hub, &HubRequest::List), Ok(HubReply::Lobbies(vec![])));

        let create = |name: &str| HubRequest::Create {
            name: name.into(),
            key: protocol_id("secret"),
            locked: true,
        };
        let Ok(HubReply::Created { port }) = ask(hub, &create("Alice")) else {
            panic!("no lobby");
        };
        assert_eq!(port, 17_101);
        // Nobody in it yet, so not listed.
        assert_eq!(ask(hub, &HubRequest::List), Ok(HubReply::Lobbies(vec![])));
        // Asking again from the same address replaces the lobby nobody got
        // into, rather than piling up lobbies.
        let Ok(HubReply::Created { port }) = ask(hub, &create("Alice")) else {
            panic!("no second lobby");
        };
        assert_eq!(port, 17_101);
    }

    /// The whole way through: a lobby is opened, a player gets in with the
    /// password, and the lobby shows in the list under their name.
    #[test]
    fn a_player_gets_into_a_new_lobby_and_it_is_listed() {
        use renet::{ConnectionConfig, RenetClient};
        use renet_netcode::{ClientAuthentication, NetcodeClientTransport};

        use crate::protocol::{CH_RELIABLE, ClientMsg};

        let hub = start_hub(17_500, 2);
        let request = HubRequest::Create {
            name: "Alice".into(),
            key: protocol_id("secret"),
            locked: true,
        };
        let Ok(HubReply::Created { port }) = ask(hub, &request) else {
            panic!("no lobby");
        };

        let now = || SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let authentication = ClientAuthentication::Unsecure {
            protocol_id: protocol_id("secret"),
            client_id: 0xA11CE,
            server_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            user_data: None,
        };
        let mut transport = NetcodeClientTransport::new(now(), authentication, socket).unwrap();
        let mut client = RenetClient::new(ConnectionConfig::default());
        let mut said_hello = false;
        let started = Instant::now();
        let mut last = Instant::now();
        loop {
            assert!(started.elapsed() < Duration::from_secs(5), "never got in");
            let delta = last.elapsed();
            last = Instant::now();
            client.update(delta);
            transport.update(delta, &mut client).unwrap();
            if client.is_connected() && !said_hello {
                said_hello = true;
                client.send_message(CH_RELIABLE, encode(&ClientMsg::Hello { name: "Alice".into() }));
            }
            transport.send_packets(&mut client).unwrap();
            if said_hello && let Ok(HubReply::Lobbies(lobbies)) = ask(hub, &HubRequest::List) && !lobbies.is_empty() {
                assert_eq!(lobbies.len(), 1);
                assert_eq!(lobbies[0].port, port);
                assert_eq!(lobbies[0].host, "Alice");
                assert_eq!(lobbies[0].players, 1);
                assert!(lobbies[0].locked);
                break;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    #[test]
    fn a_hub_of_another_version_says_so() {
        let hub = start_hub(17_200, 1);
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        socket.set_read_timeout(Some(GIVE_UP)).unwrap();
        socket
            .send_to(&pack(hub_version() ^ 1, 7, &encode(&HubRequest::List)), hub)
            .unwrap();
        let mut buf = [0u8; MAX_DATAGRAM];
        let (len, _) = socket.recv_from(&mut buf).unwrap();
        assert_eq!(unpack(&buf[..len]), Some((hub_version(), 7, &[][..])));
    }

    #[test]
    fn a_full_hub_refuses_new_lobbies() {
        let mut hub = Hub::bind(
            ServerSettings {
                port: 17_300,
                ..ServerSettings::default()
            },
            1,
        )
        .unwrap();
        let alice = IpAddr::from([10, 0, 0, 1]);
        let bob = IpAddr::from([10, 0, 0, 2]);
        assert_eq!(hub.create("Alice", 1, false, alice), HubReply::Created { port: 17_301 });
        assert!(matches!(hub.create("Bob", 1, false, bob), HubReply::Refused(_)));
        // Once Alice's lobby has sat empty long enough, it closes.
        hub.sweep(Instant::now() + IDLE_CLOSE);
        assert_eq!(hub.create("Bob", 1, false, bob), HubReply::Created { port: 17_301 });
    }

    #[test]
    fn a_lost_answer_gets_the_same_lobby_again() {
        let mut hub = Hub::bind(
            ServerSettings {
                port: 17_400,
                ..ServerSettings::default()
            },
            4,
        )
        .unwrap();
        let from = SocketAddr::from(([10, 0, 0, 1], 5000));
        let request = pack(
            hub_version(),
            42,
            &encode(&HubRequest::Create {
                name: "Alice".into(),
                key: 1,
                locked: false,
            }),
        );
        let first = hub.handle(&request, from).unwrap();
        let again = hub.handle(&request, from).unwrap();
        assert_eq!(first, again);
        assert_eq!(hub.lobbies.len(), 1);
    }
}
