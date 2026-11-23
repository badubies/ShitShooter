//! The main menu's view of a lobby server (`hub.rs`): its lobbies, a button
//! to join each, and one to open a lobby of your own there.

use std::net::SocketAddr;

use bevy::{
    prelude::*,
    text::EditableText,
    ui_widgets::{Activate, observe},
};

use crate::{
    AppState,
    hub::{AskError, HubQuery, HubReply, HubRequest, LobbyInfo, resolve},
    map::MAP_NAMES,
    net::{self, MenuStatus},
    protocol::{DEFAULT_PORT, MODE_NAMES, protocol_id},
    settings::Settings,
    ui::{
        ACCENT, ADDRESS_CHARS, CARD, DIM, Field, MenuPrefs, PANEL, PANEL_EDGE, TEXT, field, finish_connect,
        label, primary_button, read_fields, section, small_button,
    },
};

/// Seconds between asking for the lobby list again, while on the main menu.
const REFRESH: f32 = 5.0;
/// Lobbies shown at most; the menu has room for no more.
const MAX_ROWS: usize = 5;

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Browser>()
            .add_systems(OnEnter(AppState::Menu), |mut browser: ResMut<Browser>| browser.refresh_in = 0.0)
            .add_systems(
                Update,
                (auto_refresh, poll, update_list, update_note)
                    .chain()
                    .run_if(in_state(AppState::Menu)),
            );
    }
}

/// What is being asked of the lobby server.
enum Asking {
    List,
    /// A new lobby, which we join as soon as it is open.
    Create { password: String, name: String, label: String },
}

#[derive(Resource, Default)]
struct Browser {
    query: Option<(HubQuery, SocketAddr, Asking)>,
    /// The lobbies last listed, and the server they are on.
    lobbies: Vec<LobbyInfo>,
    server: Option<SocketAddr>,
    /// How it went, under the list.
    note: String,
    /// Bumped whenever `lobbies` or `server` change, to rebuild the rows.
    version: u32,
    refresh_in: f32,
}

impl Browser {
    fn ask(&mut self, server: SocketAddr, request: HubRequest, asking: Asking) -> Result<(), String> {
        let query = HubQuery::send(server, &request).map_err(|err| err.to_string())?;
        self.query = Some((query, server, asking));
        Ok(())
    }

    fn show(&mut self, server: Option<SocketAddr>, lobbies: Vec<LobbyInfo>) {
        if self.server != server || self.lobbies != lobbies {
            self.server = server;
            self.lobbies = lobbies;
            self.version += 1;
        }
    }

    /// Asks for the lobby list of the server at `address`, unless something
    /// else is already being asked.
    fn list(&mut self, address: &str) {
        self.refresh_in = REFRESH;
        if self.query.is_some() {
            return;
        }
        if address.is_empty() {
            self.show(None, Vec::new());
            self.note = "Type in the address of a lobby server to see its lobbies.".to_string();
            return;
        }
        let result = resolve(address, DEFAULT_PORT).and_then(|server| self.ask(server, HubRequest::List, Asking::List));
        if let Err(message) = result {
            self.show(None, Vec::new());
            self.note = message;
        }
    }
}

/// Why a lobby server did not do what was asked, for people.
fn explain(err: &AskError, server: SocketAddr) -> String {
    match err {
        AskError::NoAnswer => format!(
            "No answer from {server}. Check the address, that the server is running, and that its UDP port {} is forwarded.",
            server.port()
        ),
        AskError::OtherVersion => {
            "The lobby server runs another version of the game. It updates itself once its lobbies are empty: try again in a few minutes.".to_string()
        }
        AskError::Io(err) => err.clone(),
    }
}

// ---------------------------------------------------------------- the card

/// The rows of lobbies, rebuilt when the list changes.
#[derive(Component, Default)]
struct LobbyRows {
    shown: Option<u32>,
}

#[derive(Component)]
struct BrowserNote;

/// The main menu's panel for the lobby server, beside the rest of the menu.
pub fn lobby_server_panel(server: &str) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            width: px(440),
            padding: UiRect::axes(px(26), px(26)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(PANEL_EDGE),
        children![
            label("ONLINE", 32.0, ACCENT),
            label("Lobbies on an always-on lobby server: join one, or open your own.", 14.0, DIM),
            (
                Node {
                    align_items: AlignItems::FlexEnd,
                    column_gap: px(8),
                    ..default()
                },
                children![
                    field("Lobby server", Field::Server, server, ADDRESS_CHARS, 4),
                    (small_button("Refresh"), observe(on_refresh)),
                ],
            ),
            (primary_button("Open a lobby"), observe(on_open_lobby)),
            section("Lobbies"),
            (
                LobbyRows::default(),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    ..default()
                },
            ),
            (label("", 14.0, DIM), BrowserNote),
        ],
    )
}

fn lobby_row(lobby: &LobbyInfo, server: SocketAddr) -> impl Bundle {
    let mut details = vec![
        MODE_NAMES.get(lobby.mode as usize).copied().unwrap_or("?").to_string(),
        MAP_NAMES.get(lobby.map as usize).copied().unwrap_or("?").to_string(),
        match (lobby.players, lobby.bots) {
            (1, 0) => "1 player".to_string(),
            (people, 0) => format!("{people} players"),
            (people, bots) => format!("{people} + {bots} bots"),
        },
    ];
    if lobby.playing {
        details.push("in a match".to_string());
    }
    if lobby.locked {
        details.push("password".to_string());
    }
    let (port, locked, host) = (lobby.port, lobby.locked, lobby.host.clone());
    (
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(16),
            padding: UiRect::axes(px(12), px(7)),
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BackgroundColor(CARD),
        children![
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    ..default()
                },
                children![
                    label(format!("{host}'s lobby"), 17.0, TEXT),
                    label(details.join("  /  "), 13.0, DIM),
                ],
            ),
            (
                small_button("Join"),
                observe(
                    move |_: On<Activate>,
                          mut commands: Commands,
                          fields: Query<(&Field, &EditableText)>,
                          mut prefs: ResMut<MenuPrefs>,
                          mut settings: ResMut<Settings>,
                          mut status: ResMut<MenuStatus>,
                          mut next_state: ResMut<NextState<AppState>>| {
                        read_fields(&fields, &mut prefs, &mut settings);
                        if locked && prefs.password.is_empty() {
                            status.0 = format!("{host}'s lobby has a password: type it in above first.");
                            return;
                        }
                        let address = SocketAddr::new(server.ip(), port);
                        let label = format!("{host}'s lobby on {}", prefs.server);
                        let result = net::connect(&mut commands, address, label, &prefs.password, &prefs.name);
                        finish_connect(result, &mut status, &mut next_state);
                    },
                ),
            ),
        ],
    )
}

fn on_refresh(
    _: On<Activate>,
    fields: Query<(&Field, &EditableText)>,
    mut prefs: ResMut<MenuPrefs>,
    mut settings: ResMut<Settings>,
    mut browser: ResMut<Browser>,
) {
    read_fields(&fields, &mut prefs, &mut settings);
    // A list on its way is from before the address was changed, perhaps.
    if matches!(browser.query, Some((_, _, Asking::List))) {
        browser.query = None;
    }
    browser.note = "Asking...".to_string();
    browser.list(&prefs.server);
}

fn on_open_lobby(
    _: On<Activate>,
    fields: Query<(&Field, &EditableText)>,
    mut prefs: ResMut<MenuPrefs>,
    mut settings: ResMut<Settings>,
    mut browser: ResMut<Browser>,
    mut status: ResMut<MenuStatus>,
) {
    read_fields(&fields, &mut prefs, &mut settings);
    let request = HubRequest::Create {
        name: prefs.name.clone(),
        key: protocol_id(&prefs.password),
        locked: !prefs.password.is_empty(),
    };
    let asking = Asking::Create {
        password: prefs.password.clone(),
        name: prefs.name.clone(),
        label: format!("your lobby on {}", prefs.server),
    };
    match resolve(&prefs.server, DEFAULT_PORT).and_then(|server| browser.ask(server, request, asking)) {
        Ok(()) => status.0 = "Opening a lobby...".to_string(),
        Err(message) => status.0 = message,
    }
}

// ---------------------------------------------------------------- systems

fn auto_refresh(time: Res<Time>, prefs: Res<MenuPrefs>, mut browser: ResMut<Browser>) {
    browser.refresh_in -= time.delta_secs();
    if browser.refresh_in <= 0.0 {
        browser.list(&prefs.server);
    }
}

fn poll(
    mut commands: Commands,
    mut browser: ResMut<Browser>,
    mut status: ResMut<MenuStatus>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let Some((query, _, _)) = &mut browser.query else { return };
    let Some(answer) = query.poll() else { return };
    let Some((_, server, asking)) = browser.query.take() else { return };
    match (asking, answer) {
        (Asking::List, Ok(HubReply::Lobbies(lobbies))) => {
            browser.note = match lobbies.len() {
                0 => "No lobbies open right now. Open one!".to_string(),
                n if n > MAX_ROWS => format!("{} more not shown.", n - MAX_ROWS),
                _ => String::new(),
            };
            browser.show(Some(server), lobbies);
        }
        (Asking::List, Err(err)) => {
            browser.note = explain(&err, server);
            browser.show(None, Vec::new());
        }
        (Asking::Create { password, name, label }, Ok(HubReply::Created { port })) => {
            let result = net::connect(&mut commands, SocketAddr::new(server.ip(), port), label, &password, &name);
            finish_connect(result, &mut status, &mut next_state);
        }
        (Asking::Create { .. }, Ok(HubReply::Refused(why))) => status.0 = why,
        (Asking::Create { .. }, Err(err)) => status.0 = explain(&err, server),
        (_, Ok(other)) => warn!("unexpected answer from the lobby server: {other:?}"),
    }
}

fn update_list(mut commands: Commands, browser: Res<Browser>, mut lists: Query<(Entity, &mut LobbyRows)>) {
    for (entity, mut rows) in &mut lists {
        if rows.shown == Some(browser.version) {
            continue;
        }
        rows.shown = Some(browser.version);
        commands.entity(entity).despawn_children();
        if let Some(server) = browser.server {
            commands.entity(entity).with_children(|list| {
                for lobby in browser.lobbies.iter().take(MAX_ROWS) {
                    list.spawn(lobby_row(lobby, server));
                }
            });
        }
    }
}

fn update_note(browser: Res<Browser>, mut notes: Query<(&mut Text, &mut Node), With<BrowserNote>>) {
    for (mut text, mut node) in &mut notes {
        if text.0 != browser.note {
            text.0 = browser.note.clone();
        }
        let display = if browser.note.is_empty() { Display::None } else { Display::Flex };
        if node.display != display {
            node.display = display;
        }
    }
}
