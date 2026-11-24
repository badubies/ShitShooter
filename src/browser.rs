//! The main menu's Play online page: the lobbies on a lobby server (`hub.rs`),
//! each with a picture of its map, and beside them the one picked, with the
//! button to join it. Or, picked instead, a lobby of your own to open there.

use std::net::SocketAddr;

use bevy::{
    picking::hover::Hovered,
    prelude::*,
    text::EditableText,
    ui_widgets::{Activate, Button, observe},
};

use crate::{
    AppState,
    hub::{AskError, HubQuery, HubReply, HubRequest, LobbyInfo, resolve},
    map::MAP_NAMES,
    menu::{
        ADDRESS_CHARS, Field, MenuPrefs, OnlineSummary, finish_connect, page_column, read_fields, status_banner,
        title_bar,
    },
    net::{self, MenuStatus},
    protocol::{DEFAULT_PORT, MAX_PLAYERS, MODE_NAMES, protocol_id},
    settings::Settings,
    ui::{
        ACCENT, BUTTON, ButtonKind, DIM, GOOD, PANEL, PANEL_EDGE, TEXT, button, field_sized, heading, label,
        primary_button, section, small_button, soft_shadow, text_box, weighted,
    },
};

/// Seconds between asking for the lobby list again, while on the main menu.
const REFRESH: f32 = 5.0;
/// Lobbies shown at most; the page has room for no more.
const MAX_ROWS: usize = 5;

pub struct BrowserPlugin;

impl Plugin for BrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Browser>()
            .add_systems(OnEnter(AppState::Menu), |mut browser: ResMut<Browser>| browser.refresh_in = 0.0)
            .add_systems(
                Update,
                (auto_refresh, poll, update_list, update_detail, update_note, style_filters)
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

/// What the panel beside the list shows.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Picked {
    /// Nothing yet: the first lobby is picked once the list comes.
    #[default]
    Nothing,
    /// A lobby of your own, to open.
    New,
    /// The lobby on this port.
    Lobby(u16),
}

#[derive(Resource, Default)]
pub struct Browser {
    query: Option<(HubQuery, SocketAddr, Asking)>,
    /// The lobbies last listed, and the server they are on.
    lobbies: Vec<LobbyInfo>,
    server: Option<SocketAddr>,
    /// How it went, under the list.
    note: String,
    /// Bumped whenever `lobbies`, `server`, `mode` or `picked` change, to
    /// rebuild the rows.
    version: u32,
    refresh_in: f32,
    /// Only lobbies of this mode are listed, if set.
    mode: Option<u8>,
    picked: Picked,
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
            self.keep_pick();
        }
    }

    /// The lobbies the list shows: of the mode picked, as many as fit.
    fn listed(&self) -> impl Iterator<Item = &LobbyInfo> {
        self.lobbies
            .iter()
            .filter(|lobby| self.mode.is_none_or(|mode| lobby.mode == mode))
            .take(MAX_ROWS)
    }

    /// Picks the first lobby listed if the one picked is gone (or nothing is),
    /// or a new lobby of your own if none is.
    fn keep_pick(&mut self) {
        let keep = match self.picked {
            Picked::Lobby(port) => self.listed().any(|lobby| lobby.port == port),
            Picked::New => true,
            Picked::Nothing => false,
        };
        if !keep {
            let first = self.listed().next().map(|lobby| lobby.port);
            self.picked = first.map_or(Picked::New, Picked::Lobby);
        }
    }

    fn pick(&mut self, picked: Picked) {
        if self.picked != picked {
            self.picked = picked;
            self.version += 1;
        }
    }

    fn picked_lobby(&self) -> Option<&LobbyInfo> {
        match self.picked {
            Picked::Lobby(port) => self.lobbies.iter().find(|lobby| lobby.port == port),
            _ => None,
        }
    }

    /// One line on what is on the server, for the main menu.
    pub fn summary(&self) -> String {
        if self.server.is_none() {
            return "Lobbies on the always-on server".to_string();
        }
        let people: u32 = self.lobbies.iter().map(|lobby| lobby.players as u32).sum();
        match self.lobbies.len() {
            0 => "No lobbies open: open the first".to_string(),
            1 => format!("1 lobby open  ·  {people} playing"),
            n => format!("{n} lobbies open  ·  {people} playing"),
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
            self.note = "Type in the address of a lobby server, top right, to see its lobbies.".to_string();
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

/// The picture of map `id`, from `assets/ui/maps`.
pub(crate) fn map_picture(asset_server: &AssetServer, id: u8) -> Option<Handle<Image>> {
    let name = MAP_NAMES.get(id as usize)?;
    Some(asset_server.load(format!("ui/maps/{}.jpg", name.to_lowercase())))
}

fn mode_name(mode: u8) -> &'static str {
    MODE_NAMES.get(mode as usize).copied().unwrap_or("?")
}

fn map_name(map: u8) -> &'static str {
    MAP_NAMES.get(map as usize).copied().unwrap_or("?")
}

// ---------------------------------------------------------------- the page

/// The rows of lobbies, rebuilt when the list or what is picked changes.
#[derive(Component, Default)]
struct LobbyRows {
    shown: Option<u32>,
}

/// The panel beside the list, rebuilt when what is picked changes.
#[derive(Component, Default)]
struct LobbyDetail {
    shown: Option<(Picked, Option<LobbyInfo>, Option<SocketAddr>)>,
}

#[derive(Component)]
struct BrowserNote;

/// A button that lists only one mode (or all, for `None`).
#[derive(Component)]
struct ModeFilter(Option<u8>);

pub fn online_page(prefs: &MenuPrefs) -> impl Bundle {
    (
        page_column(),
        children![
            title_bar(
                "Play online",
                (
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(10),
                        ..default()
                    },
                    children![
                        section("Server"),
                        text_box(Field::Server, &prefs.server, ADDRESS_CHARS, 4, 14.0),
                        (small_button("Refresh"), observe(on_refresh)),
                        (
                            button("+  Open a lobby"),
                            observe(|_: On<Activate>, mut browser: ResMut<Browser>| browser.pick(Picked::New)),
                        ),
                    ],
                ),
            ),
            status_banner(),
            (
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                },
                children![
                    mode_filter(None, "All modes"),
                    mode_filter(Some(0), mode_name(0)),
                    mode_filter(Some(1), mode_name(1)),
                    mode_filter(Some(2), mode_name(2)),
                    mode_filter(Some(3), mode_name(3)),
                    (
                        Node {
                            flex_grow: 1.0,
                            justify_content: JustifyContent::FlexEnd,
                            ..default()
                        },
                        children![(label("", 14.0, DIM), OnlineSummary)],
                    ),
                ],
            ),
            (
                Node {
                    flex_grow: 1.0,
                    column_gap: px(24),
                    min_height: px(0),
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(8),
                            ..default()
                        },
                        children![
                            (
                                LobbyRows::default(),
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(8),
                                    ..default()
                                },
                            ),
                            (label("", 14.0, DIM), BrowserNote),
                        ],
                    ),
                    (
                        LobbyDetail::default(),
                        Node {
                            width: px(400),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                    ),
                ],
            ),
        ],
    )
}

fn mode_filter(mode: Option<u8>, name: &str) -> impl Bundle {
    (
        ModeFilter(mode),
        small_button(name),
        observe(move |_: On<Activate>, mut browser: ResMut<Browser>| {
            if browser.mode != mode {
                browser.mode = mode;
                browser.version += 1;
                browser.keep_pick();
            }
        }),
    )
}

/// Marks the mode the list shows.
fn style_filters(browser: Res<Browser>, mut filters: Query<(&ModeFilter, &mut ButtonKind, &mut BorderColor)>) {
    for (filter, mut kind, mut border) in &mut filters {
        let wanted = if filter.0 == browser.mode { ButtonKind::Picked } else { ButtonKind::Normal };
        if *kind != wanted {
            *kind = wanted;
            *border = BorderColor::all(wanted.edge());
        }
    }
}

fn lobby_row(lobby: &LobbyInfo, picked: bool, picture: Option<Handle<Image>>) -> impl Bundle {
    let kind = if picked { ButtonKind::Picked } else { ButtonKind::Tile };
    let port = lobby.port;
    let mut name = format!("{}'s lobby", lobby.host);
    if lobby.locked {
        name.push_str("  ·  password");
    }
    let count = match lobby.bots {
        0 => format!("{}", lobby.players),
        bots => format!("{} + {bots} bots", lobby.players),
    };
    let full = (lobby.players as f32 + lobby.bots as f32) / MAX_PLAYERS as f32;
    (
        Button,
        kind,
        Hovered::default(),
        Node {
            height: px(84),
            align_items: AlignItems::Center,
            column_gap: px(16),
            padding: UiRect::new(px(10), px(18), px(0), px(0)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(16)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(kind.edge()),
        observe(move |_: On<Activate>, mut browser: ResMut<Browser>| browser.pick(Picked::Lobby(port))),
        children![
            map_thumb(picture, 112.0, 63.0, 10.0),
            (
                Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (weighted(name, 18.0, 700, TEXT), Pickable::IGNORE),
                    (
                        label(format!("{}  ·  {}", mode_name(lobby.mode), map_name(lobby.map)), 14.0, DIM),
                        Pickable::IGNORE,
                    ),
                ],
            ),
            (
                Node {
                    width: px(120),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (weighted(format!("{count}  / {MAX_PLAYERS}"), 14.0, 600, TEXT), Pickable::IGNORE),
                    fill_bar(full),
                ],
            ),
            status_pill(lobby.playing),
        ],
    )
}

/// A map's picture, or a plain box when there is none.
fn map_thumb(picture: Option<Handle<Image>>, width: f32, height: f32, round: f32) -> impl Bundle {
    (
        picture.map(ImageNode::new).unwrap_or_default(),
        Node {
            width: px(width),
            height: px(height),
            flex_shrink: 0.0,
            border_radius: BorderRadius::all(px(round)),
            ..default()
        },
        BackgroundColor(BUTTON),
        Pickable::IGNORE,
    )
}

/// How full a lobby is.
fn fill_bar(full: f32) -> impl Bundle {
    (
        Node {
            height: px(4),
            border_radius: BorderRadius::all(px(2)),
            ..default()
        },
        BackgroundColor(BUTTON),
        Pickable::IGNORE,
        children![(
            Node {
                width: percent(100.0 * full.clamp(0.0, 1.0)),
                height: px(4),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            BackgroundColor(TEXT),
            Pickable::IGNORE,
        )],
    )
}

/// Playing, or waiting in the lobby for the match to start.
fn status_pill(playing: bool) -> impl Bundle {
    let (text, color) = if playing { ("PLAYING", ACCENT) } else { ("IN LOBBY", GOOD) };
    (
        Node {
            width: px(96),
            height: px(28),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(14)),
            ..default()
        },
        BackgroundColor(color.with_alpha(0.16)),
        Pickable::IGNORE,
        children![(weighted(text, 12.0, 700, color), Pickable::IGNORE)],
    )
}

fn update_list(
    mut commands: Commands,
    browser: Res<Browser>,
    asset_server: Res<AssetServer>,
    mut lists: Query<(Entity, &mut LobbyRows)>,
) {
    for (entity, mut rows) in &mut lists {
        if rows.shown == Some(browser.version) {
            continue;
        }
        rows.shown = Some(browser.version);
        commands.entity(entity).despawn_children();
        if browser.server.is_some() {
            commands.entity(entity).with_children(|list| {
                for lobby in browser.listed() {
                    let picked = browser.picked == Picked::Lobby(lobby.port);
                    list.spawn(lobby_row(lobby, picked, map_picture(&asset_server, lobby.map)));
                }
            });
        }
    }
}

/// Fills the panel beside the list with what is picked. What was typed into
/// its password field is kept when it is rebuilt.
fn update_detail(
    mut commands: Commands,
    browser: Res<Browser>,
    asset_server: Res<AssetServer>,
    mut prefs: ResMut<MenuPrefs>,
    fields: Query<(&Field, &EditableText)>,
    mut panels: Query<(Entity, &mut LobbyDetail)>,
) {
    let wanted = (browser.picked, browser.picked_lobby().cloned(), browser.server);
    for (entity, mut detail) in &mut panels {
        if detail.shown.as_ref() == Some(&wanted) {
            continue;
        }
        detail.shown = Some(wanted.clone());
        if let Some((_, typed)) = fields.iter().find(|(field, _)| **field == Field::Password) {
            prefs.password = typed.value().to_string();
        }
        commands.entity(entity).despawn_children();
        commands.entity(entity).with_children(|panel| match (&wanted.1, wanted.2) {
            (Some(lobby), Some(server)) => {
                panel.spawn(lobby_detail(lobby, server, &prefs, map_picture(&asset_server, lobby.map)));
            }
            _ if browser.picked == Picked::New || browser.server.is_none() => {
                panel.spawn(new_lobby_detail(&prefs));
            }
            _ => {}
        });
    }
}

/// The box the panel's contents are in.
fn detail_box() -> impl Bundle {
    (
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(20)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(PANEL_EDGE),
        soft_shadow(),
    )
}

/// The inside of the panel, under the picture.
fn detail_body() -> Node {
    Node {
        flex_grow: 1.0,
        flex_direction: FlexDirection::Column,
        row_gap: px(16),
        padding: UiRect::axes(px(24), px(22)),
        ..default()
    }
}

fn lobby_detail(lobby: &LobbyInfo, server: SocketAddr, prefs: &MenuPrefs, picture: Option<Handle<Image>>) -> impl Bundle {
    let host = lobby.host.clone();
    let (port, locked) = (lobby.port, lobby.locked);
    let note = if lobby.playing {
        "A match is on: you join it straight away.".to_string()
    } else {
        format!("The match hasn't started: you wait in the lobby with {host} until they start it.")
    };
    // The password field, only for a lobby that has one.
    let password = (
        Node {
            display: if locked { Display::Flex } else { Display::None },
            flex_direction: FlexDirection::Column,
            ..default()
        },
        children![field_sized("Password", Field::Password, &prefs.password, 32, 5, 18.0)],
    );
    (
        detail_box(),
        children![
            (
                Node {
                    width: percent(100),
                    height: px(225),
                    flex_shrink: 0.0,
                    ..default()
                },
                children![(
                    map_thumb(picture, 400.0, 225.0, 0.0),
                    children![(
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(16),
                            top: px(16),
                            ..default()
                        },
                        children![status_pill(lobby.playing)],
                    )],
                )],
            ),
            (
                detail_body(),
                children![
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(4),
                            ..default()
                        },
                        children![
                            heading(&format!("{host}'s lobby"), 24.0, TEXT),
                            label(format!("{} on {}", mode_name(lobby.mode), map_name(lobby.map)), 15.0, DIM),
                        ],
                    ),
                    (
                        Node {
                            column_gap: px(8),
                            ..default()
                        },
                        children![
                            fact("People", lobby.players.to_string()),
                            fact("Bots", lobby.bots.to_string()),
                            fact("Password", if locked { "Yes" } else { "None" }.to_string()),
                        ],
                    ),
                    label(note, 14.0, DIM),
                    password,
                    (
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ),
                    (
                        primary_button(&format!("Join {host}'s lobby")),
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
                                    status.0 = format!("{host}'s lobby has a password: type it in first.");
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
            ),
        ],
    )
}

/// One number about a lobby, under its caption.
fn fact(caption: &str, value: String) -> impl Bundle {
    (
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            padding: UiRect::axes(px(12), px(10)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(BUTTON.with_alpha(0.05)),
        children![weighted(caption.to_uppercase(), 11.0, 700, DIM), weighted(value, 18.0, 700, TEXT)],
    )
}

fn new_lobby_detail(prefs: &MenuPrefs) -> impl Bundle {
    (
        detail_box(),
        children![(
            detail_body(),
            children![
                (
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8),
                        ..default()
                    },
                    children![
                        heading("Open your own", 24.0, TEXT),
                        label(
                            "You lead it: you pick the mode, the map and the bots. It shows up in this list for everyone.",
                            15.0,
                            DIM,
                        ),
                    ],
                ),
                field_sized("Password (optional)", Field::Password, &prefs.password, 32, 5, 18.0),
                (
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                ),
                (primary_button("Open a lobby"), observe(on_open_lobby)),
            ],
        )],
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
    if prefs.server.is_empty() {
        status.0 = "Type in the address of a lobby server first, top right.".to_string();
        return;
    }
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
            let total = lobbies.len();
            browser.show(Some(server), lobbies);
            let listed = browser.listed().count();
            browser.note = match total {
                0 => "No lobbies open right now. Open one!".to_string(),
                _ if listed == 0 => "No lobbies of this mode right now.".to_string(),
                n if n > listed && browser.mode.is_none() => format!("{} more not shown.", n - listed),
                _ => String::new(),
            };
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
