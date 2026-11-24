//! The main menu: a home page with the three ways to play (Play online, Host,
//! Join a friend), and a page for each of them. The Play online page is the
//! lobby browser, in `browser.rs`.
//!
//! One page shows at a time, under a dark see-through layer that lets the map
//! circling behind show. What is typed into the fields is kept in `MenuPrefs`
//! when moving between pages, so each page shows only the fields it needs.

use bevy::{
    clipboard::Clipboard,
    input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup},
    picking::hover::Hovered,
    prelude::*,
    text::{EditableText, TextEdit},
    ui_widgets::{Activate, Button, observe},
};

use crate::{
    AppState, LaunchArgs,
    browser::{Browser, online_page},
    net::{self, MenuStatus},
    paint_editor::on_open_paint,
    protocol::DEFAULT_PORT,
    server::ServerSettings,
    settings::{Settings, on_open_settings},
    ui::{
        ACCENT, ButtonKind, CARD, DIM, FAINT, ON_PRIMARY, PANEL, PANEL_EDGE, SCRIM, TEXT, button, danger_button,
        field_sized, heading, label, panel, primary_button, section, set_shown, small_button, weighted,
    },
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        let args = app.world().resource::<LaunchArgs>().clone();
        // What was typed last time, unless the command line says otherwise.
        let saved = app.world().resource::<Settings>().clone();
        app.insert_resource(MenuPrefs {
            name: args.name.clone().unwrap_or(saved.name),
            address: args.join.clone().unwrap_or(saved.address),
            port: if args.server.port == DEFAULT_PORT {
                saved.port
            } else {
                args.server.port.to_string()
            },
            password: args.server.password.clone(),
            server: saved.lobby_server,
        })
        .init_resource::<MenuPage>()
        .add_systems(Startup, auto_launch)
        .add_systems(
            Update,
            (show_page, update_status, update_online_summary)
                .chain()
                .run_if(in_state(AppState::Menu)),
        );
    }
}

/// What was last typed into the menu, so it survives leaving and coming back.
#[derive(Resource, Clone)]
pub(crate) struct MenuPrefs {
    pub name: String,
    pub address: String,
    pub port: String,
    pub password: String,
    /// The lobby server's address.
    pub server: String,
}

#[derive(Component, Clone, Copy, PartialEq)]
pub(crate) enum Field {
    Name,
    Address,
    Port,
    Password,
    Server,
}

/// The page of the main menu that is up. It stays put when the game comes back
/// to the menu after a failed connection, so the error shows where it happened.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum MenuPage {
    #[default]
    Home,
    Online,
    Host,
    Join,
}

/// The root of the page on screen.
#[derive(Component)]
struct PageRoot(MenuPage);

/// The box that says what went wrong (or what is happening), and its text.
#[derive(Component)]
struct StatusBanner;
#[derive(Component)]
struct StatusText;

/// On the Play online tile: how many lobbies are open.
#[derive(Component)]
pub(crate) struct OnlineSummary;

/// Longest address the Address field takes.
pub(crate) const ADDRESS_CHARS: usize = 64;

/// Behind an error.
const ERROR_BACK: Color = Color::srgba(1.0, 0.42, 0.38, 0.12);
const ERROR_EDGE: Color = Color::srgba(1.0, 0.42, 0.38, 0.45);

// ---------------------------------------------------------------- moving between pages

/// Puts up the page in `MenuPage` whenever it is not the one on screen,
/// including on coming back to the menu.
fn show_page(
    mut commands: Commands,
    page: Res<MenuPage>,
    roots: Query<(Entity, &PageRoot)>,
    prefs: Res<MenuPrefs>,
    browser: Res<Browser>,
) {
    if roots.iter().any(|(_, root)| root.0 == *page) {
        return;
    }
    for (entity, _) in &roots {
        commands.entity(entity).despawn();
    }
    let root = commands.spawn(page_frame(*page)).id();
    let mut root = commands.entity(root);
    match *page {
        MenuPage::Home => root.with_child(home_page(&prefs, &browser)),
        MenuPage::Online => root.with_child(online_page(&prefs)),
        MenuPage::Host => root.with_child(host_page(&prefs)),
        MenuPage::Join => root.with_child(join_page(&prefs)),
    };
}

/// An observer that keeps what was typed and goes to `page`.
pub(crate) fn go(
    page: MenuPage,
) -> impl Fn(
    On<Activate>,
    Query<(&Field, &EditableText)>,
    ResMut<MenuPrefs>,
    ResMut<Settings>,
    ResMut<MenuStatus>,
    ResMut<MenuPage>,
) {
    move |_, fields, mut prefs, mut settings, mut status, mut current| {
        read_fields(&fields, &mut prefs, &mut settings);
        status.0.clear();
        *current = page;
    }
}

/// The whole screen: the dark layer over the map, and a column in the middle
/// no wider than a laptop screen, which the page goes in.
fn page_frame(page: MenuPage) -> impl Bundle {
    (
        PageRoot(page),
        TabGroup::new(0),
        DespawnOnExit(AppState::Menu),
        Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        // The home page lets more of the map show; the others are for reading.
        BackgroundColor(if page == MenuPage::Home { SCRIM } else { SCRIM.with_alpha(0.86) }),
    )
}

/// The column a page's parts go in, top to bottom.
pub(crate) fn page_column() -> Node {
    Node {
        width: percent(100),
        max_width: px(1240),
        height: percent(100),
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(px(48), px(40)),
        row_gap: px(24),
        ..default()
    }
}

/// Back to the home page, and the page's title.
pub(crate) fn title_bar(title: &str, right: impl Bundle) -> impl Bundle {
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: px(20),
            ..default()
        },
        children![
            (button("‹  Back"), observe(go(MenuPage::Home))),
            heading(title, 40.0, TEXT),
            (
                Node {
                    flex_grow: 1.0,
                    justify_content: JustifyContent::FlexEnd,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                },
                children![right],
            ),
        ],
    )
}

/// What went wrong, or what is happening. Takes up no room when there is
/// nothing to say.
pub(crate) fn status_banner() -> impl Bundle {
    (
        StatusBanner,
        Node {
            display: Display::None,
            align_self: AlignSelf::FlexStart,
            max_width: px(760),
            padding: UiRect::axes(px(18), px(14)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(14)),
            ..default()
        },
        BackgroundColor(ERROR_BACK),
        BorderColor::all(ERROR_EDGE),
        children![(weighted("", 15.0, 600, TEXT), StatusText)],
    )
}

fn update_status(
    status: Res<MenuStatus>,
    mut banners: Query<(&mut Node, &mut BackgroundColor, &mut BorderColor), With<StatusBanner>>,
    mut texts: Query<&mut Text, With<StatusText>>,
) {
    // "Opening a lobby..." is news, not an error.
    let busy = status.0.ends_with("...");
    let (back, edge) = if busy { (CARD, PANEL_EDGE) } else { (ERROR_BACK, ERROR_EDGE) };
    for (mut node, mut background, mut border) in &mut banners {
        set_shown(&mut node, !status.0.is_empty());
        background.set_if_neq(BackgroundColor(back));
        if border.top != edge {
            *border = BorderColor::all(edge);
        }
    }
    for mut text in &mut texts {
        if text.0 != status.0 {
            text.0 = status.0.clone();
        }
    }
}

fn update_online_summary(browser: Res<Browser>, mut texts: Query<&mut Text, With<OnlineSummary>>) {
    let summary = browser.summary();
    for mut text in &mut texts {
        if text.0 != summary {
            text.0 = summary.clone();
        }
    }
}

// ---------------------------------------------------------------- home

fn home_page(prefs: &MenuPrefs, browser: &Browser) -> impl Bundle {
    let last = if prefs.address.is_empty() {
        "Ask them for their address".to_string()
    } else {
        format!("Last time: {}", prefs.address)
    };
    (
        page_column(),
        children![
            (
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::FlexStart,
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(8),
                            ..default()
                        },
                        children![
                            (
                                Text::new("SHITSHOOTER"),
                                TextFont {
                                    font_size: FontSize::Px(64.0),
                                    weight: FontWeight::BLACK,
                                    width: FontWidth::EXPANDED,
                                    ..default()
                                },
                                TextColor(ACCENT),
                            ),
                            label("fast arena shooter, hosted by you", 17.0, DIM),
                        ],
                    ),
                    (
                        Node {
                            padding: UiRect::all(px(14)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(18)),
                            ..default()
                        },
                        BackgroundColor(PANEL),
                        BorderColor::all(PANEL_EDGE),
                        children![field_sized("Playing as", Field::Name, &prefs.name, 16, 0, 11.0)],
                    ),
                ],
            ),
            status_banner(),
            (
                Node {
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    column_gap: px(20),
                    ..default()
                },
                children![
                    tile(
                        ButtonKind::Primary,
                        1.3,
                        "Play online",
                        "Jump into a lobby on the server, or open your own.",
                        (weighted(browser.summary(), 14.0, 700, ON_PRIMARY), OnlineSummary, Pickable::IGNORE),
                        MenuPage::Online,
                    ),
                    tile(
                        ButtonKind::Tile,
                        1.0,
                        "Host",
                        "Friends join a game running on this PC.",
                        (label("Port and password on the next step", 14.0, DIM), Pickable::IGNORE),
                        MenuPage::Host,
                    ),
                    tile(
                        ButtonKind::Tile,
                        1.0,
                        "Join a friend",
                        "Paste the address they sent you.",
                        (label(last, 14.0, DIM), Pickable::IGNORE),
                        MenuPage::Join,
                    ),
                ],
            ),
            (
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![
                    (
                        Node {
                            column_gap: px(10),
                            ..default()
                        },
                        children![
                            (button("Paint yourself"), observe(on_open_paint)),
                            (button("Settings"), observe(on_open_settings)),
                            (danger_button("Quit"), observe(on_quit)),
                        ],
                    ),
                    label(format!("version {}", crate::VERSION), 13.0, FAINT),
                ],
            ),
        ],
    )
}

/// One of the three ways to play: a big card that is a button. `grow` is how
/// much of the row it takes compared to the others.
fn tile(kind: ButtonKind, grow: f32, title: &str, text: &str, footer: impl Bundle, page: MenuPage) -> impl Bundle {
    let (title_color, text_color) = if kind == ButtonKind::Primary { (ON_PRIMARY, ON_PRIMARY) } else { (TEXT, DIM) };
    let title_size = if kind == ButtonKind::Primary { 36.0 } else { 26.0 };
    (
        Button,
        kind,
        Hovered::default(),
        Node {
            flex_grow: grow,
            flex_basis: px(0),
            height: px(320),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            row_gap: px(12),
            padding: UiRect::all(px(30)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(24)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(kind.edge()),
        kind.shadow(),
        observe(go(page)),
        children![
            (heading(title, title_size, title_color), Pickable::IGNORE),
            (label(text, 17.0, text_color), Pickable::IGNORE),
            (
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ),
            footer,
        ],
    )
}

fn on_quit(_: On<Activate>, mut exit: MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
}

// ---------------------------------------------------------------- host

fn host_page(prefs: &MenuPrefs) -> impl Bundle {
    let port = prefs.port.parse::<u16>().unwrap_or(DEFAULT_PORT);
    (
        page_column(),
        children![
            title_bar("Host on this PC", Node::default()),
            status_banner(),
            (
                Node {
                    column_gap: px(48),
                    align_items: AlignItems::FlexStart,
                    ..default()
                },
                children![
                    (
                        Node {
                            width: px(520),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        children![(
                            panel(),
                            children![
                                (
                                    form_column(),
                                    children![
                                        field_sized("UDP port", Field::Port, &prefs.port, 5, 1, 5.0),
                                        label("Leave it at 7777 unless that port is taken.", 14.0, DIM),
                                    ],
                                ),
                                (
                                    form_column(),
                                    children![field_sized(
                                        "Password (optional)",
                                        Field::Password,
                                        &prefs.password,
                                        32,
                                        2,
                                        22.0,
                                    )],
                                ),
                                (
                                    form_column(),
                                    children![(primary_button("Start hosting"), observe(on_host))],
                                ),
                            ],
                        )],
                    ),
                    (
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(14),
                            ..default()
                        },
                        children![
                            section("How friends get in"),
                            step(
                                "1",
                                "On the same Wi-Fi",
                                "They pick Join a friend and type this PC's address. Your lobby shows it, with a Copy button.",
                            ),
                            step(
                                "2",
                                "Over the internet",
                                &format!(
                                    "Forward UDP port {port} on your router to this PC, then send them your public IP."
                                ),
                            ),
                            (
                                Node {
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::SpaceBetween,
                                    column_gap: px(16),
                                    padding: UiRect::new(px(20), px(14), px(14), px(14)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(18)),
                                    ..default()
                                },
                                BorderColor::all(PANEL_EDGE),
                                children![
                                    label("No router setup with a lobby on the server.", 15.0, DIM),
                                    (small_button("Play online"), observe(go(MenuPage::Online))),
                                ],
                            ),
                        ],
                    ),
                ],
            ),
        ],
    )
}

/// A column of form parts, as wide as the panel it is in.
fn form_column() -> Node {
    Node {
        width: percent(100),
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        ..default()
    }
}

/// A numbered step of instructions.
fn step(number: &str, title: &str, text: &str) -> impl Bundle + use<> {
    (
        Node {
            column_gap: px(16),
            padding: UiRect::axes(px(20), px(18)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(18)),
            ..default()
        },
        BackgroundColor(PANEL.with_alpha(0.7)),
        BorderColor::all(PANEL_EDGE),
        children![
            (
                Node {
                    width: px(36),
                    height: px(36),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(ACCENT.with_alpha(0.14)),
                children![weighted(number.to_string(), 16.0, 800, ACCENT)],
            ),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    flex_shrink: 1.0,
                    ..default()
                },
                children![weighted(title.to_string(), 17.0, 700, TEXT), label(text.to_string(), 15.0, DIM)],
            ),
        ],
    )
}

// ---------------------------------------------------------------- join

fn join_page(prefs: &MenuPrefs) -> impl Bundle {
    (
        page_column(),
        children![
            title_bar("Join a friend", Node::default()),
            status_banner(),
            (
                Node {
                    width: px(720),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                children![(
                    panel(),
                    children![
                        (
                            Node {
                                width: percent(100),
                                align_items: AlignItems::FlexEnd,
                                column_gap: px(10),
                                ..default()
                            },
                            children![
                                (
                                    Node {
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        ..default()
                                    },
                                    children![field_sized(
                                        "Address",
                                        Field::Address,
                                        &prefs.address,
                                        ADDRESS_CHARS,
                                        1,
                                        30.0,
                                    )],
                                ),
                                (button("Paste"), observe(on_paste_address)),
                            ],
                        ),
                        (
                            Node {
                                width: percent(100),
                                align_items: AlignItems::FlexEnd,
                                column_gap: px(20),
                                ..default()
                            },
                            children![
                                (
                                    Node {
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        ..default()
                                    },
                                    children![field_sized(
                                        "Password (if they set one)",
                                        Field::Password,
                                        &prefs.password,
                                        32,
                                        2,
                                        18.0,
                                    )],
                                ),
                                (primary_button("Join"), observe(on_join)),
                            ],
                        ),
                    ],
                )],
            ),
            (
                Node::default(),
                children![label(
                    "An address looks like 192.168.0.23 (on your Wi-Fi) or mypc.duckdns.org. Add :port if it isn't 7777.",
                    14.0,
                    DIM,
                )],
            ),
        ],
    )
}

/// Replaces what is in the Address field with the first line of the clipboard.
fn on_paste_address(
    _: On<Activate>,
    mut clipboard: ResMut<Clipboard>,
    mut fields: Query<(Entity, &Field, &mut EditableText)>,
    mut focus: ResMut<InputFocus>,
    mut status: ResMut<MenuStatus>,
) {
    let pasted = match clipboard.fetch_text().poll_result() {
        Some(Ok(text)) => text.lines().next().unwrap_or("").trim().chars().take(ADDRESS_CHARS).collect::<String>(),
        _ => String::new(),
    };
    if pasted.is_empty() {
        status.0 = "There is no text on the clipboard to paste.".to_string();
        return;
    }
    let Some((entity, _, mut text)) = fields.iter_mut().find(|(_, field, _)| **field == Field::Address) else {
        return;
    };
    text.clear();
    text.queue_edit(TextEdit::Insert(pasted.into()));
    focus.set(entity, FocusCause::Navigated);
    status.0.clear();
}

// ---------------------------------------------------------------- starting a game

/// Takes what is in the text fields on screen, and remembers it for the next
/// time the game is started (all but the password, which is not written to disk).
pub(crate) fn read_fields(fields: &Query<(&Field, &EditableText)>, prefs: &mut MenuPrefs, settings: &mut Settings) {
    for (field, text) in fields {
        let value = text.value().to_string().trim().to_string();
        match field {
            Field::Name => prefs.name = value,
            Field::Address => prefs.address = value,
            Field::Port => prefs.port = value,
            Field::Password => prefs.password = value,
            Field::Server => prefs.server = value,
        }
    }
    settings.name = prefs.name.clone();
    settings.address = prefs.address.clone();
    settings.port = prefs.port.clone();
    settings.lobby_server = prefs.server.clone();
    settings.save();
}

fn start_host(
    commands: &mut Commands,
    prefs: &MenuPrefs,
    args: &LaunchArgs,
    status: &mut MenuStatus,
    next_state: &mut NextState<AppState>,
) {
    let Ok(port) = prefs.port.parse::<u16>() else {
        status.0 = format!("'{}' is not a valid port.", prefs.port);
        return;
    };
    let settings = ServerSettings {
        port,
        password: prefs.password.clone(),
        ..args.server.clone()
    };
    finish_connect(net::host(commands, settings, &prefs.name), status, next_state);
}

fn start_join(
    commands: &mut Commands,
    prefs: &MenuPrefs,
    status: &mut MenuStatus,
    next_state: &mut NextState<AppState>,
) {
    if prefs.address.is_empty() {
        status.0 = "Type in the address your friend sent you first.".to_string();
        return;
    }
    let result = net::join(commands, &prefs.address, &prefs.password, &prefs.name);
    finish_connect(result, status, next_state);
}

pub(crate) fn finish_connect(result: Result<(), String>, status: &mut MenuStatus, next_state: &mut NextState<AppState>) {
    match result {
        Ok(()) => {
            status.0.clear();
            next_state.set(AppState::Connecting);
        }
        Err(message) => status.0 = message,
    }
}

fn on_host(
    _: On<Activate>,
    mut commands: Commands,
    fields: Query<(&Field, &EditableText)>,
    args: Res<LaunchArgs>,
    mut prefs: ResMut<MenuPrefs>,
    mut settings: ResMut<Settings>,
    mut status: ResMut<MenuStatus>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    read_fields(&fields, &mut prefs, &mut settings);
    start_host(&mut commands, &prefs, &args, &mut status, &mut next_state);
}

fn on_join(
    _: On<Activate>,
    mut commands: Commands,
    fields: Query<(&Field, &EditableText)>,
    mut prefs: ResMut<MenuPrefs>,
    mut settings: ResMut<Settings>,
    mut status: ResMut<MenuStatus>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    read_fields(&fields, &mut prefs, &mut settings);
    start_join(&mut commands, &prefs, &mut status, &mut next_state);
}

/// `--host` / `--join` on the command line skip the menu.
fn auto_launch(
    mut commands: Commands,
    args: Res<LaunchArgs>,
    prefs: Res<MenuPrefs>,
    mut status: ResMut<MenuStatus>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if args.host {
        start_host(&mut commands, &prefs, &args, &mut status, &mut next_state);
    } else if args.join.is_some() {
        start_join(&mut commands, &prefs, &mut status, &mut next_state);
    }
}
