//! The 2D interface's building blocks (the look of buttons, fields and
//! panels), and the connecting screen, lobby, in-game HUD, scoreboard and pause
//! menu. The main menu is in `menu.rs`.

use bevy::{
    clipboard::Clipboard,
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    input_focus::{
        InputFocus,
        tab_navigation::{TabIndex, TabNavigationPlugin},
    },
    picking::{hover::Hovered, pointer::PointerButton},
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::Pressed,
    ui_widgets::{Activate, Button, TextInput, observe},
    window::PrimaryWindow,
};

use crate::{
    AppState,
    combat::{HIT_MARKER_TIME, KillFeed, Notices},
    grenade::SmokeFog,
    gun_model::FittedModels,
    map::MAP_NAMES,
    menu::{Field, MenuPage},
    net::{self, Connection, Session},
    paint::{Skins, palette_color},
    loadout::{LoadoutCard, loadout_card, loadout_summary, on_open_loadout},
    paint_editor::on_open_paint,
    player::{Controls, DAMAGE_ARC_TIME, DASH_COOLDOWN, LocalPlayer, ask_respawn},
    protocol::*,
    settings::{Action, ActionInput, Settings, on_open_settings},
    world::{GameMap, MainCamera},
};

// The menus are warm-dark see-through panels over the live map, with thin
// light edges and soft shadows. Orange marks the one thing to press next, and
// whatever is picked.

/// Over the map behind a menu, so the panels read.
pub const SCRIM: Color = Color::srgba(0.047, 0.039, 0.035, 0.74);
pub const PANEL: Color = Color::srgba(0.094, 0.078, 0.067, 0.92);
/// The edge of panels and cards: the text colour, faint.
pub const PANEL_EDGE: Color = Color::srgba(1.0, 0.925, 0.84, 0.09);
/// A box inside a panel, a shade lighter than it.
pub const CARD: Color = Color::srgba(1.0, 0.925, 0.84, 0.04);
pub const BUTTON: Color = Color::srgba(1.0, 0.925, 0.84, 0.07);
const BUTTON_HOVER: Color = Color::srgba(1.0, 0.925, 0.84, 0.13);
const BUTTON_PRESS: Color = Color::srgba(1.0, 0.925, 0.84, 0.19);
pub const BUTTON_EDGE: Color = Color::srgba(1.0, 0.925, 0.84, 0.12);
/// The one button on a screen that moves things along.
const PRIMARY: Color = Color::srgb(1.0, 0.541, 0.122);
const PRIMARY_HOVER: Color = Color::srgb(1.0, 0.62, 0.27);
const PRIMARY_PRESS: Color = Color::srgb(0.91, 0.47, 0.07);
/// Text on a primary button.
pub const ON_PRIMARY: Color = Color::srgb(0.102, 0.055, 0.016);
/// A big card that is a button (the main menu's choices, a lobby in the list).
const TILE: Color = Color::srgba(0.094, 0.078, 0.067, 0.88);
const TILE_HOVER: Color = Color::srgba(0.15, 0.125, 0.105, 0.92);
const TILE_PRESS: Color = Color::srgba(0.19, 0.16, 0.135, 0.94);
/// Whatever is picked: a mode, a map, a lobby in the list.
const PICKED: Color = Color::srgba(1.0, 0.541, 0.122, 0.13);
const PICKED_HOVER: Color = Color::srgba(1.0, 0.541, 0.122, 0.18);
const PICKED_PRESS: Color = Color::srgba(1.0, 0.541, 0.122, 0.24);
const DANGER_HOVER: Color = Color::srgba(1.0, 0.42, 0.38, 0.20);
pub const FIELD: Color = Color::srgba(0.047, 0.039, 0.035, 0.72);
const FIELD_EDGE: Color = Color::srgba(1.0, 0.925, 0.84, 0.14);
pub const TEXT: Color = Color::srgb(0.965, 0.933, 0.894);
pub const DIM: Color = Color::srgb(0.70, 0.647, 0.584);
/// Footnotes, and what can't be picked.
pub const FAINT: Color = Color::srgb(0.557, 0.51, 0.447);
pub const ACCENT: Color = Color::srgb(1.0, 0.541, 0.122);
/// Errors, and the killer's name.
pub const DANGER: Color = Color::srgb(1.0, 0.541, 0.502);
/// The text of Leave and Quit.
pub const DANGER_TEXT: Color = Color::srgb(1.0, 0.706, 0.659);
pub const GOOD: Color = Color::srgb(0.373, 0.824, 0.498);
/// Behind the HUD's panels: see-through, so the game shows past them.
pub const HUD_PANEL: Color = Color::srgba(0.03, 0.04, 0.07, 0.6);
pub const HUD_EDGE: Color = Color::srgba(1.0, 1.0, 1.0, 0.09);
/// The empty part of a bar.
pub const TRACK: Color = Color::srgba(1.0, 1.0, 1.0, 0.12);
/// Our team, wherever it shows: the outline around teammates, their names,
/// our score. Everybody else is the enemy.
pub const ALLY: Color = Color::srgb(0.30, 0.86, 1.0);
pub const ENEMY: Color = Color::srgb(1.0, 0.42, 0.38);
/// At or below this, health is shown in red and the bar pulses.
const LOW_HEALTH: i32 = 30;
/// Armor on the HUD, the same blue as the pickups.
const ARMOR: Color = Color::srgb(0.40, 0.68, 1.0);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // Archivo in place of Bevy's built-in font, for every piece of text.
        // It is a variable font: `TextFont::weight` and `width` pick its
        // boldness and how wide it runs (see `heading`).
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(AssetId::default(), Font::from_bytes(include_bytes!("fonts/Archivo.ttf").to_vec()))
            .expect("the default font's slot");
        app.add_plugins(TabNavigationPlugin)
            .add_systems(OnEnter(AppState::Connecting), spawn_connecting)
            .add_systems(OnEnter(AppState::Lobby), spawn_lobby)
            .add_systems(OnEnter(AppState::Playing), spawn_hud)
            .add_systems(
                Update,
                (
                    style_buttons,
                    style_fields,
                    wheel_scroll,
                    update_connecting.run_if(in_state(AppState::Connecting)),
                    (update_lobby, style_match_choices).run_if(in_state(AppState::Lobby)),
                    update_rosters,
                    update_key_hints,
                    (
                        update_hud,
                        update_hud_shapes,
                        update_overlays,
                        update_crosshair,
                        update_scope_view,
                        update_damage_arcs,
                        update_choke_ring,
                        show_respawn_button,
                    )
                        .run_if(in_state(AppState::Playing)),
                ),
            );
    }
}

// ---------------------------------------------------------------- building blocks

pub fn label(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// Text in a given weight (see `FontWeight`), in the normal width.
pub fn weighted(text: impl Into<String>, size: f32, weight: u16, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            ..default()
        },
        TextColor(color),
    )
}

/// A title: extra bold, wide capitals.
pub fn heading(text: &str, size: f32, color: Color) -> impl Bundle + use<> {
    (
        Text::new(text.to_uppercase()),
        TextFont {
            font_size: FontSize::Px(size),
            weight: FontWeight::EXTRA_BOLD,
            width: FontWidth::EXPANDED,
            ..default()
        },
        TextColor(color),
    )
}

/// The small dim capitals over a group of controls.
pub fn section(text: &str) -> impl Bundle + use<> {
    weighted(text.to_uppercase(), 12.0, 700, DIM)
}

/// How a button is coloured.
#[derive(Component, Clone, Copy, PartialEq, Default)]
pub enum ButtonKind {
    #[default]
    Normal,
    /// The main thing to do on a screen: filled with the accent colour.
    Primary,
    /// Leaving and quitting: red text, turns red under the mouse.
    Danger,
    /// A big card that is a button.
    Tile,
    /// What is picked, among things to pick from.
    Picked,
}

impl ButtonKind {
    /// (at rest, under the mouse, held down)
    fn colors(self) -> [Color; 3] {
        match self {
            ButtonKind::Normal => [BUTTON, BUTTON_HOVER, BUTTON_PRESS],
            ButtonKind::Primary => [PRIMARY, PRIMARY_HOVER, PRIMARY_PRESS],
            ButtonKind::Danger => [BUTTON, DANGER_HOVER, DANGER_HOVER],
            ButtonKind::Tile => [TILE, TILE_HOVER, TILE_PRESS],
            ButtonKind::Picked => [PICKED, PICKED_HOVER, PICKED_PRESS],
        }
    }

    pub fn edge(self) -> Color {
        match self {
            ButtonKind::Primary | ButtonKind::Picked => PRIMARY,
            ButtonKind::Tile => PANEL_EDGE,
            _ => BUTTON_EDGE,
        }
    }

    /// The colour of the text on it.
    pub fn ink(self) -> Color {
        match self {
            ButtonKind::Primary => ON_PRIMARY,
            ButtonKind::Danger => DANGER_TEXT,
            _ => TEXT,
        }
    }

    /// Under it: a glow for the primary button, a soft shadow for tiles.
    pub fn shadow(self) -> BoxShadow {
        match self {
            ButtonKind::Primary => BoxShadow::new(PRIMARY.with_alpha(0.22), px(0), px(6), px(0), px(18)),
            ButtonKind::Tile => BoxShadow::new(Color::BLACK.with_alpha(0.38), px(0), px(16), px(0), px(40)),
            _ => BoxShadow::default(),
        }
    }
}

/// The box of a button, without its text.
pub fn button_box(kind: ButtonKind, small: bool) -> impl Bundle {
    let (across, down, round) = match (small, kind) {
        (true, _) => (12, 6, 9),
        (false, ButtonKind::Primary) => (32, 15, 14),
        (false, _) => (20, 12, 12),
    };
    (
        Button,
        kind,
        Hovered::default(),
        Node {
            padding: UiRect::axes(px(across), px(down)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(round)),
            ..default()
        },
        BackgroundColor(kind.colors()[0]),
        BorderColor::all(kind.edge()),
        kind.shadow(),
    )
}

fn button_of(text: &str, kind: ButtonKind, small: bool) -> impl Bundle + use<> {
    // The primary button's text is like a heading; the rest is plain.
    let (text, font) = if kind == ButtonKind::Primary && !small {
        let font = TextFont {
            font_size: FontSize::Px(19.0),
            weight: FontWeight::EXTRA_BOLD,
            width: FontWidth::EXPANDED,
            ..default()
        };
        (text.to_uppercase(), font)
    } else {
        let font = TextFont {
            font_size: FontSize::Px(if small { 14.0 } else { 16.0 }),
            weight: FontWeight::SEMIBOLD,
            ..default()
        };
        (text.to_string(), font)
    };
    (
        button_box(kind, small),
        children![(Text::new(text), font, TextColor(kind.ink()), Pickable::IGNORE)],
    )
}

pub fn button(text: &str) -> impl Bundle + use<> {
    button_of(text, ButtonKind::Normal, false)
}

pub fn primary_button(text: &str) -> impl Bundle + use<> {
    button_of(text, ButtonKind::Primary, false)
}

pub(crate) fn danger_button(text: &str) -> impl Bundle + use<> {
    button_of(text, ButtonKind::Danger, false)
}

pub(crate) fn small_button(text: &str) -> impl Bundle + use<> {
    button_of(text, ButtonKind::Normal, true)
}

/// A caption with a one-line text field under it, `width` characters wide.
pub(crate) fn field_sized(
    caption: &str,
    field: Field,
    initial: &str,
    max_chars: usize,
    tab: i32,
    width: f32,
) -> impl Bundle + use<> {
    (
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },
        children![section(caption), text_box(field, initial, max_chars, tab, width)],
    )
}

/// A one-line text field, `width` characters wide, with no caption.
pub(crate) fn text_box(field: Field, initial: &str, max_chars: usize, tab: i32, width: f32) -> impl Bundle + use<> {
    (
        field,
        TextInput,
        EditableText {
            visible_width: Some(width),
            max_characters: Some(max_chars),
            ..EditableText::new(initial)
        },
        TextLayout::no_wrap(),
        TextFont {
            font_size: FontSize::Px(18.0),
            weight: FontWeight::SEMIBOLD,
            ..default()
        },
        TextColor(TEXT),
        TextCursorStyle::default(),
        TabIndex(tab),
        Node {
            border: UiRect::all(px(1)),
            padding: UiRect::axes(px(16), px(13)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BorderColor::all(FIELD_EDGE),
        BackgroundColor(FIELD),
    )
}

/// Full-screen container that centres its children.
fn screen(state: AppState) -> impl Bundle {
    (
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        DespawnOnExit(state),
    )
}

pub fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(16),
            padding: UiRect::all(px(28)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(24)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(PANEL_EDGE),
        soft_shadow(),
    )
}

/// The shadow under panels.
pub fn soft_shadow() -> BoxShadow {
    BoxShadow::new(Color::BLACK.with_alpha(0.45), px(0), px(24), px(0), px(48))
}

/// A box inside a panel that groups a few controls under a `section` caption.
pub fn card() -> impl Bundle {
    (card_node(), BackgroundColor(CARD), BorderColor::all(PANEL_EDGE))
}

pub fn card_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(12),
        padding: UiRect::all(px(18)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(16)),
        ..default()
    }
}

pub fn button_row() -> Node {
    Node {
        column_gap: px(10),
        align_items: AlignItems::Center,
        ..default()
    }
}

/// A row with its two ends pushed apart: a caption and what it is about.
pub fn spread_row() -> Node {
    Node {
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        column_gap: px(12),
        ..default()
    }
}

fn style_buttons(
    mut buttons: Query<(&Hovered, Has<Pressed>, Option<&ButtonKind>, &mut BackgroundColor), With<Button>>,
) {
    for (hovered, pressed, kind, mut background) in &mut buttons {
        let [rest, hover, press] = kind.copied().unwrap_or_default().colors();
        let color = match (hovered.get(), pressed) {
            (_, true) => press,
            (true, false) => hover,
            (false, false) => rest,
        };
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// The text field being typed into gets a bright edge.
fn style_fields(focus: Res<InputFocus>, mut fields: Query<(Entity, &mut BorderColor), With<Field>>) {
    for (entity, mut border) in &mut fields {
        let wanted = if focus.get() == Some(entity) { ACCENT } else { FIELD_EDGE };
        if border.top != wanted {
            *border = BorderColor::all(wanted);
        }
    }
}

/// A box whose contents scroll with the mouse wheel while the pointer is on it.
#[derive(Component)]
pub struct WheelScroll;

fn wheel_scroll(
    wheel: Res<AccumulatedMouseScroll>,
    mut boxes: Query<(&Hovered, &ComputedNode, &mut ScrollPosition), With<WheelScroll>>,
) {
    if wheel.delta.y == 0.0 {
        return;
    }
    let step = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * 40.0,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    for (hovered, node, mut scroll) in &mut boxes {
        if !hovered.get() {
            continue;
        }
        // Kept within what there is to see, so scrolling back starts at once.
        let room = ((node.content_size().y - node.size().y) * node.inverse_scale_factor).max(0.0);
        scroll.0.y = (scroll.0.y - step).clamp(0.0, room);
    }
}

pub fn set_text(text: &mut Mut<Text>, value: String) {
    if text.0 != value {
        text.0 = value;
    }
}

pub fn set_shown(node: &mut Mut<Node>, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
}

fn on_leave(
    _: On<Activate>,
    mut commands: Commands,
    mut connection: Option<ResMut<Connection>>,
    mut page: ResMut<MenuPage>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    net::disconnect(&mut commands, connection.as_deref_mut());
    *page = MenuPage::Home;
    next_state.set(AppState::Menu);
}

// ---------------------------------------------------------------- connecting

/// The dots after "Connecting", which come and go to show it is still trying.
#[derive(Component)]
struct ConnectingDots;

fn spawn_connecting(mut commands: Commands, session: Res<Session>) {
    commands.spawn((
        screen(AppState::Connecting),
        BackgroundColor(SCRIM),
        children![
            (
                Node {
                    width: px(480),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                children![(
                    panel(),
                    children![
                        (
                            Node {
                                align_items: AlignItems::Baseline,
                                ..default()
                            },
                            children![
                                heading("Connecting", 28.0, TEXT),
                                // Three characters wide whatever it shows, so the line does not shift.
                                (heading("", 28.0, ACCENT), ConnectingDots, Node { width: px(50), ..default() }),
                            ],
                        ),
                        label(session.server_label.clone(), 16.0, DIM),
                    ],
                )],
            ),
            (
                Node {
                    margin: UiRect::top(px(20)),
                    ..default()
                },
                children![(danger_button("Cancel"), observe(on_leave))],
            ),
        ],
    ));
}

fn update_connecting(time: Res<Time>, mut dots: Single<&mut Text, With<ConnectingDots>>) {
    let count = (time.elapsed_secs() * 2.5) as usize % 4;
    set_text(&mut dots, ".".repeat(count));
}

// ---------------------------------------------------------------- player list

/// A table of the players and their scores, kept up to date by
/// `update_rosters`. Used in the lobby and for the in-game scoreboard.
#[derive(Component, Default)]
struct RosterList {
    /// What the rows were last built from.
    shown: Option<RosterKey>,
}

/// Everything the player list shows; the rows are rebuilt when it changes.
#[derive(Clone, PartialEq)]
struct RosterKey {
    roster: Vec<PlayerInfo>,
    my_id: PlayerId,
    leader: PlayerId,
    team_mode: bool,
    my_team: u8,
    team_scores: [u32; 2],
    /// The dot beside each name, in roster order: their main paint colour.
    colors: Vec<u8>,
}

/// How wide the scoreboard's player list is at least.
const ROSTER_WIDTH: f32 = 430.0;
const STAT_WIDTH: f32 = 56.0;

fn roster_list(min_width: f32) -> impl Bundle {
    (
        RosterList::default(),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            min_width: px(min_width),
            ..default()
        },
    )
}

/// A number in one of the two columns on the right of the player list.
fn roster_stat(text: String, size: f32, color: Color) -> impl Bundle {
    (
        label(text, size, color),
        TextLayout::justify(Justify::Right),
        Node {
            width: px(STAT_WIDTH),
            ..default()
        },
    )
}

fn roster_row_node() -> Node {
    Node {
        align_items: AlignItems::Center,
        column_gap: px(10),
        padding: UiRect::axes(px(10), px(7)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(10)),
        ..default()
    }
}

/// The line over a group of players: a title, and the names of the columns.
fn roster_heading(title: String, color: Color) -> impl Bundle {
    (
        roster_row_node(),
        children![
            (weighted(title, 11.0, 700, color), Node { flex_grow: 1.0, ..default() }),
            roster_stat("KILLS".into(), 11.0, DIM),
            roster_stat("DEATHS".into(), 11.0, DIM),
        ],
    )
}

fn roster_row(player: &PlayerInfo, swatch: Color, key: &RosterKey) -> impl Bundle {
    let me = player.id == key.my_id;
    let (name, tag) = match player.name.strip_prefix("[bot] ") {
        Some(name) if player.is_bot => (name, "BOT"),
        _ if player.id == key.leader => (player.name.as_str(), "LEADER"),
        _ => (player.name.as_str(), ""),
    };
    (
        roster_row_node(),
        // Your own line stands out.
        BackgroundColor(if me { ACCENT.with_alpha(0.10) } else { CARD }),
        BorderColor::all(if me { ACCENT.with_alpha(0.5) } else { Color::NONE }),
        children![
            (
                Node {
                    width: px(14),
                    height: px(14),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(swatch),
            ),
            (weighted(name, 16.0, 600, TEXT), TextLayout::no_wrap()),
            (
                weighted(tag, 10.0, 700, if tag == "LEADER" { ACCENT } else { DIM }),
                Node { flex_grow: 1.0, ..default() },
            ),
            roster_stat(player.kills.to_string(), 16.0, TEXT),
            roster_stat(player.deaths.to_string(), 16.0, DIM),
        ],
    )
}

/// Rebuilds the rows of every player list whose players or scores changed.
/// Best score first; one group per team in the team modes.
fn update_rosters(
    mut commands: Commands,
    session: Res<Session>,
    skins: Res<Skins>,
    my_paint: Option<Res<crate::paint::MyPaint>>,
    mut lists: Query<(Entity, &mut RosterList)>,
) {
    if lists.is_empty() {
        return;
    }
    let mut roster = session.roster.clone();
    roster.sort_by(|a, b| b.kills.cmp(&a.kills).then(a.deaths.cmp(&b.deaths)));
    let colors = roster
        .iter()
        .map(|player| match &my_paint {
            Some(mine) if player.id == session.my_id => mine.canvas.main_color(),
            _ => skins.main_color(player.id).unwrap_or(player.color),
        })
        .collect();
    let key = RosterKey {
        roster,
        my_id: session.my_id,
        leader: session.leader,
        team_mode: session.team_mode(),
        my_team: session.team_of(session.my_id),
        team_scores: session.team_scores,
        colors,
    };
    for (entity, mut list) in &mut lists {
        if list.shown.as_ref() == Some(&key) {
            continue;
        }
        list.shown = Some(key.clone());
        let column = || Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(2),
            ..default()
        };
        commands.entity(entity).despawn_children().with_children(|groups| {
            if !key.team_mode {
                groups.spawn(column()).with_children(|rows| {
                    rows.spawn(roster_heading("PLAYER".into(), DIM));
                    for (player, color) in key.roster.iter().zip(&key.colors) {
                        rows.spawn(roster_row(player, palette_color(*color), &key));
                    }
                });
                return;
            }
            // Our team first.
            for (team, title, color) in [(key.my_team, "YOUR TEAM", ALLY), (key.my_team ^ 1, "ENEMY TEAM", ENEMY)] {
                groups.spawn(column()).with_children(|rows| {
                    let title = format!("{title}  -  {}", key.team_scores[team as usize & 1]);
                    rows.spawn(roster_heading(title, color));
                    for player in key.roster.iter().filter(|player| player.team & 1 == team) {
                        rows.spawn(roster_row(player, color, &key));
                    }
                });
            }
        });
    }
}

// ---------------------------------------------------------------- lobby

/// The pieces of text in the lobby that change.
#[derive(Component, Clone, Copy)]
enum LobbyText {
    /// Whose lobby it is, and who picks the match.
    Caption,
    Map,
    Mode,
    Bots,
    Players,
    /// How the last match ended.
    Result,
    /// Who we are waiting for.
    Waiting,
}

/// Shown only to the lobby leader.
#[derive(Component)]
struct LeaderOnly;

/// Shown to everyone but the lobby leader.
#[derive(Component)]
struct GuestOnly;

/// Shown only in the team modes.
#[derive(Component)]
struct TeamModeOnly;

/// The box with how the last match ended; hidden before the first.
#[derive(Component)]
struct ResultBanner;

/// The text of the button that copies the lobby's address.
#[derive(Component)]
struct CopyLabel;

/// A chip or card with which the leader picks the match.
#[derive(Component, Clone, Copy, PartialEq)]
enum MatchChoice {
    Mode(u8),
    Map(u8),
    AirAccel(u16),
}

/// A map card's picture, dimmed when the mode isn't played on the map.
#[derive(Component)]
struct MapPicture(u8);

/// Under a map's name: picked, or not played in this mode.
#[derive(Component)]
struct MapNote(u8);

/// How each air acceleration is shown, named after where it is played.
fn air_accel_name(value: u16) -> String {
    match value {
        10 => "10 · CS".to_string(),
        100 => "100 · bhop".to_string(),
        other => other.to_string(),
    }
}

fn spawn_lobby(mut commands: Commands, session: Res<Session>, asset_server: Res<AssetServer>) {
    let share = session.share_address.clone();
    let server_label = session.server_label.clone();
    let modes = (0..MODE_NAMES.len() as u8).map(|mode| {
        (
            MatchChoice::Mode(mode),
            small_button(MODE_NAMES[mode as usize]),
            Pickable::default(),
            observe(send_choice),
        )
    });
    let maps = (0..MAP_NAMES.len() as u8).map(|id| map_card(id, crate::browser::map_picture(&asset_server, id)));
    let accels = AIR_ACCEL_CHOICES.map(|value| {
        (
            MatchChoice::AirAccel(value),
            small_button(&air_accel_name(value)),
            Pickable::default(),
            observe(send_choice),
        )
    });

    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                min_height: px(0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SCRIM.with_alpha(0.84)),
            DespawnOnExit(AppState::Lobby),
        ))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    width: percent(100),
                    max_width: px(1240),
                    height: percent(100),
                    // Never taller than the window: the player list gives way.
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(48), px(32)),
                    row_gap: px(18),
                    ..default()
                })
                .with_children(|page| {
                    page.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((danger_button("‹  Leave lobby"), observe(on_leave)));
                        match share {
                            // Hosting: how the others get in.
                            Some(address) => row.spawn(invite_box(address)),
                            None => row.spawn((
                                Node {
                                    padding: UiRect::axes(px(16), px(12)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(12)),
                                    ..default()
                                },
                                BackgroundColor(PANEL),
                                BorderColor::all(PANEL_EDGE),
                                children![label(format!("Connected to {server_label}"), 13.0, DIM)],
                            )),
                        };
                    });
                    page.spawn((
                        ResultBanner,
                        Node {
                            display: Display::None,
                            align_self: AlignSelf::FlexStart,
                            padding: UiRect::axes(px(16), px(10)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(12)),
                            ..default()
                        },
                        BackgroundColor(ALLY.with_alpha(0.12)),
                        BorderColor::all(ALLY.with_alpha(0.4)),
                        children![(weighted("", 15.0, 700, TEXT), LobbyText::Result)],
                    ));
                    page.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6),
                            ..default()
                        },
                        children![
                            (weighted("", 12.0, 700, DIM), LobbyText::Caption),
                            (
                                Node {
                                    column_gap: px(18),
                                    ..default()
                                },
                                children![
                                    (heading("", 44.0, TEXT), LobbyText::Map),
                                    (heading("", 44.0, ACCENT), LobbyText::Mode),
                                ],
                            ),
                        ],
                    ));
                    // The room between the title and the buttons, whatever is in it.
                    page.spawn(Node {
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        column_gap: px(24),
                        min_height: px(0),
                        ..default()
                    })
                    .with_children(|content| {
                        content
                            .spawn((
                                Node {
                                    flex_grow: 1.0,
                                    flex_basis: px(0),
                                    // As wide as there is room for, not as its map pictures.
                                    min_width: px(0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(16),
                                    padding: UiRect::axes(px(22), px(20)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(20)),
                                    ..default()
                                },
                                BackgroundColor(PANEL),
                                BorderColor::all(PANEL_EDGE),
                                soft_shadow(),
                            ))
                            .with_children(|choices| {
                                let group = || Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(8),
                                    min_width: px(0),
                                    ..default()
                                };
                                let row = |gap: i32| Node {
                                    column_gap: px(gap),
                                    align_items: AlignItems::Center,
                                    min_width: px(0),
                                    ..default()
                                };
                                choices.spawn(group()).with_children(|group| {
                                    group.spawn(section("Mode"));
                                    group.spawn(row(8)).with_children(|row| {
                                        for chip in modes {
                                            row.spawn(chip);
                                        }
                                    });
                                });
                                choices.spawn(group()).with_children(|group| {
                                    group.spawn(section("Map"));
                                    group.spawn(row(12)).with_children(|row| {
                                        for card in maps {
                                            row.spawn(card);
                                        }
                                    });
                                });
                                choices.spawn(row(32)).with_children(|row| {
                                    row.spawn(group()).with_children(|group| {
                                        group.spawn(section("Air accel"));
                                        group.spawn(Node { column_gap: px(6), ..default() }).with_children(|chips| {
                                            for chip in accels {
                                                chips.spawn(chip);
                                            }
                                        });
                                    });
                                    row.spawn(group()).with_children(|group| {
                                        group.spawn(section("Bots"));
                                        group.spawn((
                                            Node {
                                                align_items: AlignItems::Center,
                                                column_gap: px(4),
                                                padding: UiRect::all(px(3)),
                                                border: UiRect::all(px(1)),
                                                border_radius: BorderRadius::all(px(12)),
                                                ..default()
                                            },
                                            BackgroundColor(FIELD),
                                            BorderColor::all(PANEL_EDGE),
                                            children![
                                                leader_button("−", ClientMsg::RemoveBot),
                                                (
                                                    weighted("", 16.0, 700, TEXT),
                                                    LobbyText::Bots,
                                                    TextLayout::justify(Justify::Center),
                                                    Node { min_width: px(34), ..default() },
                                                ),
                                                leader_button("+", ClientMsg::AddBot),
                                            ],
                                        ));
                                    });
                                });
                            });
                        content.spawn((
                            Node {
                                width: px(360),
                                flex_shrink: 0.0,
                                // The player list gives way, not the buttons under it.
                                min_height: px(0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(12),
                                ..default()
                            },
                            children![
                                (
                                    Node {
                                        // Whatever room the loadout under it leaves.
                                        flex_grow: 1.0,
                                        flex_basis: px(0),
                                        min_height: px(0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(10),
                                        padding: UiRect::all(px(16)),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(20)),
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                    BackgroundColor(PANEL),
                                    BorderColor::all(PANEL_EDGE),
                                    children![
                                        (
                                            Node {
                                                justify_content: JustifyContent::SpaceBetween,
                                                align_items: AlignItems::Center,
                                                min_height: px(30),
                                                ..default()
                                            },
                                            children![
                                                (weighted("", 12.0, 700, DIM), LobbyText::Players),
                                                (
                                                    Node::default(),
                                                    TeamModeOnly,
                                                    children![(
                                                        small_button("Switch team"),
                                                        observe(send_on_click(ClientMsg::SwitchTeam)),
                                                    )],
                                                ),
                                            ],
                                        ),
                                        (
                                            WheelScroll,
                                            Hovered::default(),
                                            ScrollPosition::default(),
                                            Node {
                                                flex_grow: 1.0,
                                                min_height: px(0),
                                                flex_direction: FlexDirection::Column,
                                                overflow: Overflow::scroll_y(),
                                                ..default()
                                            },
                                            children![roster_list(0.0)],
                                        ),
                                    ],
                                ),
                                loadout_summary(),
                            ],
                        ));
                    });
                    page.spawn((
                        Node {
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        children![
                            (
                                button_row(),
                                children![
                                    (button("Loadout"), observe(on_open_loadout)),
                                    (button("Paint yourself"), observe(on_open_paint)),
                                    (button("Settings"), observe(on_open_settings)),
                                ],
                            ),
                            (
                                Node::default(),
                                children![
                                    (
                                        primary_button("Start match"),
                                        LeaderOnly,
                                        observe(send_on_click(ClientMsg::StartMatch)),
                                    ),
                                    (
                                        Node {
                                            padding: UiRect::axes(px(22), px(16)),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(14)),
                                            ..default()
                                        },
                                        BackgroundColor(PANEL),
                                        BorderColor::all(PANEL_EDGE),
                                        GuestOnly,
                                        children![(weighted("", 16.0, 600, DIM), LobbyText::Waiting)],
                                    ),
                                ],
                            ),
                        ],
                    ));
                });
        });
}

/// The address friends join with, and a button that copies it.
fn invite_box(address: String) -> impl Bundle {
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::new(px(16), px(5), px(5), px(5)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(PANEL),
        BorderColor::all(PANEL_EDGE),
        children![
            label("Friends join with", 13.0, DIM),
            weighted(address.clone(), 16.0, 700, TEXT),
            (
                button_box(ButtonKind::Normal, true),
                children![(weighted("Copy", 14.0, 600, TEXT), Pickable::IGNORE, CopyLabel)],
                observe(
                    move |_: On<Activate>, mut clipboard: ResMut<Clipboard>, mut text: Single<&mut Text, With<CopyLabel>>| {
                        let done = clipboard.set_text(address.as_str()).is_ok();
                        text.0 = if done { "Copied" } else { "Failed" }.to_string();
                    },
                ),
            ),
        ],
    )
}

/// A map to pick: its picture, its name, and a note under it.
fn map_card(id: u8, picture: Option<Handle<Image>>) -> impl Bundle {
    let choice = MatchChoice::Map(id);
    (
        choice,
        Button,
        ButtonKind::Tile,
        Hovered::default(),
        Pickable::default(),
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            // Not held as wide as its picture.
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(14)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(ButtonKind::Tile.edge()),
        observe(send_choice),
        children![
            (
                MapPicture(id),
                picture.map(ImageNode::new).unwrap_or_default(),
                Node {
                    width: percent(100),
                    min_width: px(0),
                    aspect_ratio: Some(16.0 / 9.0),
                    ..default()
                },
                Pickable::IGNORE,
            ),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    padding: UiRect::axes(px(12), px(8)),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (weighted(MAP_NAMES.get(id as usize).copied().unwrap_or("?"), 15.0, 700, TEXT), Pickable::IGNORE),
                    (MapNote(id), label("", 11.0, DIM), Pickable::IGNORE),
                ],
            ),
        ],
    )
}

fn leader_button(text: &str, msg: ClientMsg) -> impl Bundle + use<> {
    (small_button(text), LeaderOnly, observe(send_on_click(msg)))
}

/// Sends the leader's pick of a mode, map or air acceleration.
fn send_choice(
    activate: On<Activate>,
    choices: Query<&MatchChoice>,
    session: Res<Session>,
    connection: Option<ResMut<Connection>>,
) {
    let (Ok(choice), Some(mut connection)) = (choices.get(activate.entity), connection) else {
        return;
    };
    if !session.is_leader() {
        return;
    }
    connection.send(&match *choice {
        MatchChoice::Mode(mode) => ClientMsg::SetMode(mode),
        MatchChoice::Map(id) => ClientMsg::SetMap(id),
        MatchChoice::AirAccel(value) => ClientMsg::SetAirAccel(value),
    });
}

/// An observer that sends `msg` to the server when its button is clicked.
fn send_on_click(msg: ClientMsg) -> impl Fn(On<Activate>, Option<ResMut<Connection>>) {
    move |_, connection| {
        if let Some(mut connection) = connection {
            connection.send(&msg);
        }
    }
}

fn update_lobby(
    session: Res<Session>,
    map: Res<GameMap>,
    mut texts: Query<(&LobbyText, &mut Text)>,
    mut shown: ParamSet<(
        Query<&mut Node, With<LeaderOnly>>,
        Query<&mut Node, With<GuestOnly>>,
        Query<&mut Node, With<TeamModeOnly>>,
        Query<&mut Node, With<ResultBanner>>,
    )>,
) {
    let leader = session.name_of(session.leader).to_string();
    for (kind, mut text) in &mut texts {
        let value = match kind {
            LobbyText::Caption if session.is_leader() => "YOUR LOBBY  ·  YOU PICK THE MATCH".to_string(),
            LobbyText::Caption => format!("{leader}'S LOBBY  ·  {leader} PICKS THE MATCH").to_uppercase(),
            LobbyText::Map => MAP_NAMES.get(map.0.id as usize).copied().unwrap_or("?").to_uppercase(),
            LobbyText::Mode => MODE_NAMES.get(session.mode as usize).copied().unwrap_or("?").to_uppercase(),
            LobbyText::Bots => session.roster.iter().filter(|player| player.is_bot).count().to_string(),
            LobbyText::Players => format!("PLAYERS  ·  {} OF {MAX_PLAYERS}", session.roster.len()),
            LobbyText::Result => session.last_result.clone().unwrap_or_default(),
            LobbyText::Waiting => format!("Waiting for {leader} to start the match"),
        };
        set_text(&mut text, value);
    }
    for mut node in &mut shown.p0() {
        set_shown(&mut node, session.is_leader());
    }
    for mut node in &mut shown.p1() {
        set_shown(&mut node, !session.is_leader());
    }
    for mut node in &mut shown.p2() {
        set_shown(&mut node, session.team_mode());
    }
    for mut node in &mut shown.p3() {
        set_shown(&mut node, session.last_result.is_some());
    }
}

/// Marks the mode, map and air acceleration picked, dims the maps the mode
/// isn't played on, and lets only the leader click any of them.
fn style_match_choices(
    session: Res<Session>,
    map: Res<GameMap>,
    mut choices: Query<(&MatchChoice, &mut ButtonKind, &mut BorderColor, &mut Pickable)>,
    mut pictures: Query<(&MapPicture, &mut ImageNode)>,
    mut notes: Query<(&MapNote, &mut Text)>,
) {
    let air_accel = session.air_accel() as u16;
    for (choice, mut kind, mut border, mut pickable) in &mut choices {
        let (picked, playable, rest) = match *choice {
            MatchChoice::Mode(mode) => (session.mode == mode, true, ButtonKind::Normal),
            MatchChoice::Map(id) => (map.0.id == id, crate::map::plays(id, session.mode), ButtonKind::Tile),
            MatchChoice::AirAccel(value) => (air_accel == value, true, ButtonKind::Normal),
        };
        let wanted = if picked { ButtonKind::Picked } else { rest };
        if *kind != wanted {
            *kind = wanted;
            *border = BorderColor::all(wanted.edge());
        }
        let clickable = session.is_leader() && playable;
        if pickable.is_hoverable != clickable {
            pickable.is_hoverable = clickable;
        }
    }
    for (picture, mut image) in &mut pictures {
        let alpha = if crate::map::plays(picture.0, session.mode) { 1.0 } else { 0.3 };
        if image.color.alpha() != alpha {
            image.color = Color::WHITE.with_alpha(alpha);
        }
    }
    for (note, mut text) in &mut notes {
        let line = if map.0.id == note.0 {
            "Picked"
        } else if !crate::map::plays(note.0, session.mode) {
            "Not for this mode"
        } else {
            ""
        };
        set_text(&mut text, line.to_string());
    }
}

// ---------------------------------------------------------------- in-game HUD

/// The pieces of text on the HUD.
#[derive(Component, Clone, Copy)]
enum Hud {
    Clock,
    /// The caption over one of the two scores beside the clock (0 = left).
    ScoreLabel(usize),
    Score(usize),
    /// The line under the clock: score limit, team, hill.
    Info,
    /// Frames per second, over the ping.
    Fps,
    Ping,
    Speed,
    DashLabel,
    /// One line of the kill feed.
    Feed(usize),
    /// "Get ready" before our first spawn.
    Center,
    /// On the death screen: who killed us.
    Killed,
    /// Under it: how long until we respawn, and how to do it now.
    Respawn,
    /// Under the loadout on the death screen: the keys that change it.
    DeadKeys,
    /// Under the crosshair: the grenade in hand, and what a click does with it
    /// (or a cooking frag's fuse).
    Hand,
    /// "FRAGGED somebody".
    Kill,
    /// Streaks and revenge, under the kill line.
    Banner,
    /// The seconds left before the match starts, then "GO".
    Countdown,
    /// The line over the countdown.
    CountdownLabel,
}

/// The part of a bar that grows and shrinks.
#[derive(Component, Clone, Copy)]
enum Bar {
    Health,
    /// Trails behind the health bar, showing what was just lost.
    HealthLag,
    Armor,
    Dash,
}

/// The box around one line of the kill feed.
#[derive(Component)]
struct FeedRow(usize);

/// The coloured shapes on the HUD.
#[derive(Component, Clone, Copy)]
enum Overlay {
    /// Red flash over the whole screen when hurt.
    DamageFlash,
    /// White over the whole screen while blinded by a flashbang.
    Blind,
    /// Grey over the whole screen while inside a smoke cloud.
    Smoke,
    /// One of the four ticks that appear around the crosshair on a hit.
    HitTick,
}

#[derive(Component)]
struct ScoreboardPanel;
#[derive(Component)]
struct ScoreboardText;
#[derive(Component)]
struct PausePanel;

/// A HUD element pinned to part of the screen. Never blocks mouse clicks.
fn hud_anchor(node: Node) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            ..node
        },
        Pickable::IGNORE,
        DespawnOnExit(AppState::Playing),
    )
}

/// A full-screen HUD layer that centres its content.
fn hud_layer() -> impl Bundle {
    hud_anchor(Node {
        width: percent(100),
        height: percent(100),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    })
}

/// A line across the screen at `top` percent of its height, text centred.
fn hud_line(top: f32) -> impl Bundle {
    hud_anchor(Node {
        width: percent(100),
        top: percent(top),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: px(4),
        ..default()
    })
}

/// HUD text, with a shadow so it reads against any background.
fn hud_text(size: f32, color: Color, hud: Hud) -> impl Bundle {
    (label("", size, color), hud_shadow(), hud)
}

pub fn hud_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::splat(1.5),
        color: Color::srgba(0.0, 0.0, 0.0, 0.8),
    }
}

/// A dark see-through box with a faint edge.
fn hud_panel(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(8)),
            ..node
        },
        BackgroundColor(HUD_PANEL),
        BorderColor::all(HUD_EDGE),
    )
}

/// The empty part of a bar; give it `bar_fill` children.
fn bar_track(width: Val, height: f32) -> impl Bundle {
    (
        Node {
            width,
            height: px(height),
            border_radius: BorderRadius::all(px(height / 2.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(TRACK),
    )
}

fn bar_fill(bar: Bar, color: Color) -> impl Bundle {
    (
        bar,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: px(0),
            width: percent(0),
            height: percent(100),
            border_radius: BorderRadius::all(px(5)),
            ..default()
        },
        BackgroundColor(color),
    )
}

/// One of the two scores beside the clock, under its caption.
fn score_block(side: usize) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            min_width: px(96),
            ..default()
        },
        children![
            hud_text(12.0, DIM, Hud::ScoreLabel(side)),
            hud_text(26.0, TEXT, Hud::Score(side)),
        ],
    )
}

/// One line of the frame rate and ping box: a caption on the left, the value
/// lined up on the right so it does not wobble as the digits change.
fn stat_row(caption: &str, hud: Hud) -> impl Bundle {
    (
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            column_gap: px(14),
            ..default()
        },
        children![
            (label(caption, 11.0, DIM), hud_shadow()),
            (
                Node {
                    min_width: px(48),
                    justify_content: JustifyContent::End,
                    ..default()
                },
                children![hud_text(14.0, TEXT, hud)],
            ),
        ],
    )
}

/// One line of the kill feed (it holds seven at most).
fn feed_row(row: usize) -> impl Bundle {
    (
        FeedRow(row),
        Node {
            padding: UiRect::axes(px(10), px(3)),
            border_radius: BorderRadius::all(px(5)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(HUD_PANEL),
        children![hud_text(16.0, TEXT, Hud::Feed(row))],
    )
}

/// One tick of the hit marker: a short diagonal stroke `(dx, dy)` away from
/// the middle of its 40 x 40 box.
fn hit_tick(dx: f32, dy: f32) -> impl Bundle {
    const LENGTH: f32 = 12.0;
    const THICKNESS: f32 = 3.0;
    // The strokes point at the crosshair: "\" on one diagonal, "/" on the other.
    let angle = if dx * dy > 0.0 { 45.0 } else { -45.0 };
    (
        Overlay::HitTick,
        Node {
            position_type: PositionType::Absolute,
            left: px(20.0 + dx - LENGTH / 2.0),
            top: px(20.0 + dy - THICKNESS / 2.0),
            width: px(LENGTH),
            height: px(THICKNESS),
            ..default()
        },
        UiTransform::from_rotation(Rot2::degrees(angle)),
        BackgroundColor(Color::NONE),
    )
}

fn spawn_hud(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // The view through a scope, under the rest of the HUD: its lens is a
    // square as tall as the screen, black bars fill the sides.
    let lens = images.add(scope_lens_image());
    let side = || {
        (
            ScopeSide,
            Node {
                flex_grow: 1.0,
                height: percent(100),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
        )
    };
    commands.spawn((
        ScopeView,
        hud_anchor(Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Row,
            ..default()
        }),
        Visibility::Hidden,
        children![
            side(),
            (
                ScopeLens,
                Node {
                    height: percent(100),
                    aspect_ratio: Some(1.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                ImageNode::new(lens),
                Pickable::IGNORE,
            ),
            side(),
        ],
    ));

    commands.spawn((hud_layer(), Overlay::DamageFlash, BackgroundColor(Color::NONE)));
    commands.spawn((hud_layer(), Overlay::Smoke, BackgroundColor(Color::NONE)));
    commands.spawn((hud_layer(), Overlay::Blind, BackgroundColor(Color::NONE)));

    // Crosshair: a white dot with a dark rim, so it shows against anything.
    commands.spawn((
        Crosshair,
        hud_layer(),
        children![(
            Node {
                width: px(8),
                height: px(8),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(Color::WHITE),
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        )],
    ));

    // A gun with a choke, aimed: a ring around the crosshair as wide as its spread.
    commands.spawn((
        hud_layer(),
        children![(
            ChokeRing,
            Node {
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::all(percent(50)),
                ..default()
            },
            BorderColor::all(Color::NONE),
            Visibility::Hidden,
        )],
    ));

    // Hit marker, around the crosshair.
    commands.spawn((
        hud_layer(),
        children![(
            Node {
                width: px(40),
                height: px(40),
                ..default()
            },
            children![
                hit_tick(-11.0, -11.0),
                hit_tick(11.0, -11.0),
                hit_tick(-11.0, 11.0),
                hit_tick(11.0, 11.0),
            ],
        )],
    ));

    // Where hits come from: arcs on a ring around the crosshair.
    let arc = images.add(damage_arc_image());
    let arcs = (0..DAMAGE_ARCS).map(move |index| {
        (
            DamageArcNode(index),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ImageNode::new(arc.clone()).with_color(Color::NONE),
            UiTransform::default(),
            Pickable::IGNORE,
        )
    });
    commands.spawn((
        hud_layer(),
        children![(
            Node {
                width: px(DAMAGE_RING),
                height: px(DAMAGE_RING),
                ..default()
            },
            Pickable::IGNORE,
            Children::spawn(SpawnIter(arcs)),
        )],
    ));

    // The grenade in hand, just under the crosshair.
    commands.spawn((hud_line(55.0), children![hud_text(18.0, ACCENT, Hud::Hand)]));

    // The countdown before the match, big, above the crosshair.
    commands.spawn((
        hud_line(26.0),
        children![
            hud_text(20.0, TEXT, Hud::CountdownLabel),
            hud_text(96.0, ACCENT, Hud::Countdown),
        ],
    ));

    // Before our first spawn, a bit above the middle of the screen.
    commands.spawn((hud_line(30.0), children![hud_text(30.0, DANGER, Hud::Center)]));

    // Kill announcements, a bit below it.
    commands.spawn((
        hud_line(60.0),
        children![hud_text(28.0, TEXT, Hud::Kill), hud_text(22.0, ACCENT, Hud::Banner)],
    ));

    // Top middle: the clock between the two scores, and a line of detail under it.
    commands.spawn((
        hud_anchor(Node {
            width: percent(100),
            top: px(12),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        }),
        children![
            (
                hud_panel(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    padding: UiRect::axes(px(14), px(6)),
                    ..default()
                }),
                children![score_block(0), hud_text(32.0, TEXT, Hud::Clock), score_block(1)],
            ),
            hud_text(14.0, DIM, Hud::Info),
        ],
    ));

    commands.spawn((
        hud_anchor(Node {
            left: px(20),
            top: px(14),
            ..default()
        }),
        children![(
            hud_panel(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                padding: UiRect::axes(px(10), px(5)),
                ..default()
            }),
            children![stat_row("FPS", Hud::Fps), stat_row("PING", Hud::Ping)],
        )],
    ));

    // Kill feed, top right.
    commands.spawn((
        hud_anchor(Node {
            right: px(20),
            top: px(14),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(4),
            ..default()
        }),
        children![
            feed_row(0),
            feed_row(1),
            feed_row(2),
            feed_row(3),
            feed_row(4),
            feed_row(5),
            feed_row(6),
        ],
    ));

    // Health, bottom left.
    commands.spawn((
        hud_anchor(Node {
            left: px(20),
            bottom: px(18),
            ..default()
        }),
        children![(
            hud_panel(Node {
                width: px(280),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::axes(px(14), px(10)),
                ..default()
            }),
            children![
                // Armor on top: the bar is full at the most you can carry.
                (bar_track(percent(100), 6.0), children![bar_fill(Bar::Armor, ARMOR)]),
                (
                    bar_track(percent(100), 10.0),
                    children![
                        bar_fill(Bar::HealthLag, Color::srgba(1.0, 1.0, 1.0, 0.75)),
                        bar_fill(Bar::Health, GOOD),
                    ],
                ),
            ],
        )],
    ));

    // Speed and dash, bottom middle. Hidden on the death screen, which
    // reaches down to it.
    commands.spawn((
        SpeedPanel,
        hud_anchor(Node {
            width: percent(100),
            bottom: px(22),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        }),
        children![
            hud_text(20.0, TEXT, Hud::Speed),
            (
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                },
                children![
                    hud_text(13.0, DIM, Hud::DashLabel),
                    (bar_track(px(120), 6.0), children![bar_fill(Bar::Dash, ACCENT)]),
                ],
            ),
        ],
    ));

    // The death screen: who got us, when we respawn, and the loadout we will
    // respawn with. The mouse is free while it shows, so the loadout can be
    // changed; a click anywhere off the card respawns us.
    commands.spawn((
        DeathScreen,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            // Clear of the clock and scores at the top, and conquest's flags.
            padding: UiRect::top(px(140)),
            row_gap: px(8),
            display: Display::None,
            ..default()
        },
        DespawnOnExit(AppState::Playing),
        observe(on_death_screen_click),
        children![
            hud_text(30.0, DANGER, Hud::Killed),
            hud_text(20.0, TEXT, Hud::Respawn),
            (
                Node {
                    margin: UiRect::top(px(6)),
                    column_gap: px(18),
                    align_items: AlignItems::Start,
                    ..default()
                },
                children![
                    crate::minimap::deploy_map(),
                    loadout_card(320.0, PANEL, hud_text(14.0, DIM, Hud::DeadKeys)),
                ],
            ),
            (primary_button("Respawn"), RespawnButton, observe(on_respawn_button)),
        ],
    ));

    commands.spawn((
        hud_layer(),
        ScoreboardPanel,
        children![(
            panel(),
            children![(label("", 16.0, DIM), ScoreboardText), roster_list(ROSTER_WIDTH)],
        )],
    ));

    // Pause menu. Spawned last so it is on top; this one does take clicks.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            PausePanel,
            GlobalZIndex(10),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|screen| {
            screen.spawn(panel()).with_children(|panel| {
                panel.spawn(heading("Paused", 34.0, TEXT));
                panel.spawn((
                    button_row(),
                    children![
                        (primary_button("Resume"), observe(on_resume)),
                        (button("Settings"), observe(on_open_settings)),
                        (button("End match"), LeaderOnly, observe(send_on_click(ClientMsg::EndMatch))),
                        (danger_button("Leave"), observe(on_leave)),
                    ],
                ));
                panel
                    .spawn(Node {
                        column_gap: px(14),
                        margin: UiRect::top(px(4)),
                        ..default()
                    })
                    .with_children(|cards| {
                        cards.spawn(card()).with_children(|card| {
                            card.spawn(section("Controls"));
                            for (actions, what) in KEY_HINTS {
                                card.spawn(Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(6),
                                    ..default()
                                })
                                .with_children(|row| {
                                    for action in actions.iter().copied() {
                                        row.spawn(key_chip(action));
                                    }
                                    row.spawn((
                                        label(*what, 15.0, DIM),
                                        Node {
                                            margin: UiRect::left(px(4)),
                                            ..default()
                                        },
                                    ));
                                });
                            }
                        });
                        // Side by side rather than stacked, so it all fits a 720p window.
                        cards.spawn(loadout_card(
                            300.0,
                            CARD,
                            label("Handed out the next time you spawn.", 14.0, DIM),
                        ));
                        cards.spawn((
                            Node {
                                width: px(260),
                                ..card_node()
                            },
                            BackgroundColor(CARD),
                            BorderColor::all(HUD_EDGE),
                            children![
                                section("Tips"),
                                label("Strafe and turn in the air to build speed.", 14.0, DIM),
                                label("Green pads launch you: arrows throw you their way, plain ones keep your momentum.", 14.0, DIM),
                                label("Bullets take time to arrive: lead moving targets.", 14.0, DIM),
                                label("Team modes: your pings show only to your team.", 14.0, DIM),
                                label(format!("Health returns after {REGEN_DELAY} s unhurt."), 14.0, DIM),
                                label(format!("Armor takes damage before health and never comes back by itself. You spawn with {START_ARMOR}; blue plates give {ARMOR_PICKUP} more, up to {MAX_ARMOR}."), 14.0, DIM),
                            ],
                        ));
                    });
            });
        });
}

/// The controls listed in the pause menu: the actions whose keys are shown,
/// and what they do.
const KEY_HINTS: &[(&[Action], &str)] = &[
    (&[Action::Forward, Action::Left, Action::Back, Action::Right], "Move"),
    (&[Action::Jump], "Jump, and again in the air (hold: bunny hop)"),
    (&[Action::Dash], "Dash"),
    (&[Action::Crouch], "Crouch (while running: slide)"),
    (&[Action::Fire], "Fire"),
    (&[Action::Aim], "Aim down sights"),
    (&[Action::Reload], "Reload"),
    (&[Action::Grenade], "Take out your grenade, click to throw"),
    (&[Action::Ping], "Ping an enemy (or a spot) for your team"),
    (&[Action::Primary, Action::Secondary, Action::Knife], "Weapons"),
    (&[Action::PreviousWeapon, Action::NextWeapon], "Previous / next weapon"),
    (&[Action::Scores], "Scoreboard"),
];

/// The name of the key an action is bound to, in a little box.
#[derive(Component)]
pub struct KeyHint(pub Action);

fn key_chip(action: Action) -> impl Bundle {
    (
        Node {
            min_width: px(28),
            padding: UiRect::axes(px(7), px(2)),
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(BUTTON),
        BorderColor::all(BUTTON_EDGE),
        children![(label("", 14.0, TEXT), KeyHint(action))],
    )
}

/// Keeps the keys in the pause menu in step with the bindings.
fn update_key_hints(
    settings: Res<Settings>,
    added: Query<(), Added<KeyHint>>,
    mut hints: Query<(&KeyHint, &mut Text)>,
) {
    if !settings.is_changed() && added.is_empty() {
        return;
    }
    for (hint, mut text) in &mut hints {
        set_text(&mut text, settings.key_names(hint.0));
    }
}

#[derive(Component)]
struct SpeedPanel;

/// The death screen (see `spawn_hud`).
#[derive(Component)]
struct DeathScreen;

#[derive(Component)]
struct RespawnButton;

fn on_respawn_button(
    _: On<Activate>,
    session: Res<Session>,
    mut player: ResMut<LocalPlayer>,
    connection: Option<ResMut<Connection>>,
) {
    if let Some(mut connection) = connection {
        ask_respawn(&mut player, &mut connection, session.mode);
    }
}

/// No respawn button where there is no respawning early (conquest).
fn show_respawn_button(session: Res<Session>, mut button: Query<&mut Node, With<RespawnButton>>) {
    for mut node in &mut button {
        set_shown(&mut node, early_respawn(session.mode));
    }
}

/// A left click on the death screen respawns us, unless it is on the card
/// (or on the button, which does that itself). The click bubbles up to the
/// screen from whatever was under the pointer.
fn on_death_screen_click(
    click: On<PointerClick>,
    parents: Query<&ChildOf>,
    keep: Query<(), Or<(With<LoadoutCard>, With<RespawnButton>, With<crate::minimap::DeployMap>)>>,
    session: Res<Session>,
    mut player: ResMut<LocalPlayer>,
    connection: Option<ResMut<Connection>>,
) {
    let target = click.original_event_target();
    let on_card = std::iter::once(target).chain(parents.iter_ancestors(target)).any(|entity| keep.contains(entity));
    if click.button == PointerButton::Primary && !on_card {
        if let Some(mut connection) = connection {
            ask_respawn(&mut player, &mut connection, session.mode);
        }
    }
}

fn on_resume(_: On<Activate>, mut controls: ResMut<Controls>) {
    controls.captured = true;
}

fn format_clock(seconds: f32) -> String {
    let total = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", total / 60, total % 60)
}

/// The frame rate on the HUD: frames counted over a quarter second, so the
/// number holds still long enough to read.
#[derive(Default)]
struct FpsCounter {
    frames: u32,
    elapsed: f32,
    shown: f32,
}

impl FpsCounter {
    fn tick(&mut self, delta: f32) {
        self.frames += 1;
        self.elapsed += delta;
        if self.elapsed >= 0.25 {
            self.shown = self.frames as f32 / self.elapsed;
            self.frames = 0;
            self.elapsed = 0.0;
        }
    }
}

/// What goes either side of the clock, and under it.
struct TopBar {
    labels: [String; 2],
    scores: [u32; 2],
    colors: [Color; 2],
    info: String,
}

fn top_bar(session: &Session) -> TopBar {
    if !session.team_mode() {
        let my_kills = session
            .roster
            .iter()
            .find(|p| p.id == session.my_id)
            .map_or(0, |p| p.kills);
        let leader = session.roster.iter().max_by_key(|p| p.kills);
        let lead = match leader {
            Some(p) if p.kills > 0 => format!("{} leads", p.name),
            _ => "no kills yet".to_string(),
        };
        return TopBar {
            labels: ["YOU".to_string(), "BEST".to_string()],
            scores: [my_kills as u32, leader.map_or(0, |p| p.kills) as u32],
            colors: [ACCENT, TEXT],
            info: format!("first to {}   |   {lead}", session.score_limit),
        };
    }
    let mine = session.team_of(session.my_id) as usize;
    let mut info = format!("first to {}", session.score_limit);
    if session.mode == MODE_CONQUEST {
        let held = |team: usize| session.flags.iter().filter(|f| f.owner == Some(team as u8)).count();
        info = match held(mine).cmp(&held(mine ^ 1)) {
            std::cmp::Ordering::Less => "tickets   |   fewer flags: your team is bleeding".to_string(),
            std::cmp::Ordering::Greater => "tickets   |   more flags: the enemy is bleeding".to_string(),
            std::cmp::Ordering::Equal => "tickets   |   hold more flags to bleed the enemy".to_string(),
        };
    }
    if session.mode == MODE_HILL {
        info.push_str(match session.hill {
            HILL_EMPTY => "   |   hill: empty",
            HILL_CONTESTED => "   |   hill: CONTESTED",
            held if (held - HILL_HELD) as usize == mine => "   |   hill: YOURS",
            _ => "   |   hill: ENEMY",
        });
    }
    TopBar {
        labels: ["YOUR TEAM".to_string(), "ENEMY".to_string()],
        scores: [session.team_scores[mine], session.team_scores[mine ^ 1]],
        colors: [ALLY, ENEMY],
        info,
    }
}

fn update_hud(
    session: Res<Session>,
    settings: Res<Settings>,
    player: Res<LocalPlayer>,
    feed: Res<KillFeed>,
    notices: Res<Notices>,
    connection: Option<Res<Connection>>,
    time: Res<Time>,
    mut fps: Local<FpsCounter>,
    mut texts: Query<(&Hud, &mut Text, &mut TextColor)>,
) {
    fps.tick(time.delta_secs());
    let ping = connection.as_ref().map_or(0.0, |c| c.ping_ms());
    let top = top_bar(&session);
    // A notice fades out over its last half second.
    let notice = |notice: &Option<(String, f32)>, color: Color| match notice {
        Some((text, left)) => (text.clone(), color.with_alpha((left / 0.5).min(1.0))),
        None => (String::new(), color),
    };
    for (hud, mut text, mut color) in &mut texts {
        let (value, wanted) = match *hud {
            Hud::Clock => (
                format_clock(session.time_left),
                // The last half minute.
                if session.time_left <= 30.0 { DANGER } else { TEXT },
            ),
            Hud::ScoreLabel(side) => (top.labels[side].clone(), top.colors[side].with_alpha(0.8)),
            Hud::Score(side) => (top.scores[side].to_string(), top.colors[side]),
            Hud::Info => (top.info.clone(), DIM),
            Hud::Fps => (
                format!("{:.0}", fps.shown),
                match fps.shown {
                    f if f < 30.0 => DANGER,
                    f if f < 60.0 => ACCENT,
                    _ => TEXT,
                },
            ),
            Hud::Ping => (
                format!("{ping:.0} ms"),
                match ping {
                    p if p > 150.0 => DANGER,
                    p if p > 80.0 => ACCENT,
                    _ => TEXT,
                },
            ),
            Hud::Speed => (format!("{:.1} m/s", player.horizontal_speed()), TEXT),
            Hud::DashLabel => (
                "DASH".to_string(),
                if player.dash_cooldown <= 0.0 { TEXT } else { DIM },
            ),
            Hud::Feed(row) => match feed.0.get(row) {
                Some((line, left)) => (line.clone(), TEXT.with_alpha((left / 0.5).min(1.0))),
                None => (String::new(), TEXT),
            },
            Hud::Center => {
                let ready = !player.alive && player.killed_by.is_none();
                (if ready { "Get ready..." } else { "" }.to_string(), DANGER)
            }
            Hud::Killed => (format!("Fragged by {}", player.killed_by.as_deref().unwrap_or("?")), DANGER),
            Hud::Respawn => {
                let message = if player.respawn_asked {
                    "Respawning...".to_string()
                } else {
                    let left = (RESPAWN_DELAY - player.dead_for).max(0.0).ceil();
                    if early_respawn(session.mode) {
                        format!("Respawning in {left:.0}  -  click off the card to respawn now")
                    } else {
                        format!("Respawning in {left:.0}")
                    }
                };
                (message, TEXT)
            }
            Hud::DeadKeys => (
                format!(
                    "{} / {} / {} also step through them.",
                    settings.key_name(Action::Primary),
                    settings.key_name(Action::Secondary),
                    settings.key_name(Action::Grenade),
                ),
                DIM,
            ),
            Hud::Hand => match player.hand.filter(|_| player.alive) {
                Some(hand) => {
                    let name = GRENADES[hand.kind as usize].name.to_uppercase();
                    match hand.fuse {
                        // About to go off in hand: red.
                        Some(fuse) => (format!("{name}  {:.1}", fuse.max(0.0)), if fuse < 0.8 { DANGER } else { ACCENT }),
                        None if hand.kind == FRAG => (format!("{name}  -  click to pull the pin"), TEXT),
                        None => (format!("{name}  -  click to throw"), TEXT),
                    }
                }
                None => (String::new(), ACCENT),
            },
            Hud::Kill => notice(&notices.kill, TEXT),
            Hud::Banner => notice(&notices.banner, ACCENT),
            Hud::Countdown => match session.countdown {
                left if left > 0.0 => (format!("{}", left.ceil() as u32), ACCENT),
                // "GO" for a moment once it is over, fading out.
                left if left > -0.8 => ("GO!".to_string(), GOOD.with_alpha(((left + 0.8) / 0.4).min(1.0))),
                _ => (String::new(), ACCENT),
            },
            Hud::CountdownLabel => (
                if session.frozen() { "MATCH STARTS IN" } else { "" }.to_string(),
                TEXT,
            ),
        };
        set_text(&mut text, value);
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// The bars, and the boxes that light up or come and go.
fn update_hud_shapes(
    time: Res<Time>,
    player: Res<LocalPlayer>,
    feed: Res<KillFeed>,
    mut health_lag: Local<f32>,
    mut bars: Query<(&Bar, &mut Node, &mut BackgroundColor)>,
    mut rows: Query<(&FeedRow, &mut Node, &mut BackgroundColor), Without<Bar>>,
) {
    let health = if player.alive { player.health.max(0) } else { 0 };
    let health_part = health as f32 / MAX_HEALTH as f32;
    // What was just lost drains away behind the bar; healing catches up at once.
    *health_lag = (*health_lag - time.delta_secs() * 0.6).max(health_part);
    let health_color = if health <= LOW_HEALTH {
        // Pulses, to be hard to miss.
        DANGER.with_alpha(0.75 + 0.25 * (time.elapsed_secs() * 9.0).sin())
    } else if health <= MAX_HEALTH * 6 / 10 {
        ACCENT
    } else {
        GOOD
    };

    let dash_part = 1.0 - player.dash_cooldown / DASH_COOLDOWN;
    let armor_part = if player.alive { player.armor.max(0) as f32 / MAX_ARMOR as f32 } else { 0.0 };

    for (bar, mut node, mut background) in &mut bars {
        let (part, color) = match *bar {
            Bar::Health => (health_part, health_color),
            Bar::HealthLag => (*health_lag, Color::srgba(1.0, 1.0, 1.0, 0.75)),
            Bar::Armor => (armor_part, ARMOR),
            Bar::Dash => (dash_part, if dash_part >= 1.0 { ACCENT } else { DIM }),
        };
        let width = percent(part.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
        if background.0 != color {
            background.0 = color;
        }
    }

    for (row, mut node, mut background) in &mut rows {
        let left = feed.0.get(row.0).map(|(_, left)| *left);
        set_shown(&mut node, left.is_some());
        // Fades out with its text.
        let color = HUD_PANEL.with_alpha(HUD_PANEL.alpha() * (left.unwrap_or(0.0) / 0.5).min(1.0));
        if background.0 != color {
            background.0 = color;
        }
    }
}

/// The view through a scope (see `LocalPlayer::scope_view`): the lens in the
/// middle, and a black bar on each side of it.
#[derive(Component)]
struct ScopeView;
#[derive(Component)]
struct ScopeLens;
#[derive(Component)]
struct ScopeSide;

/// Pixels across the scope's lens texture.
const SCOPE_LENS_SIZE: u32 = 1024;

/// A square texture of one colour, `size` pixels across, see-through by
/// `alpha(u, v, pixel)`: `u` and `v` run from -1 to 1 across and down, and
/// `pixel` is the width of one pixel in those units.
pub fn mask_image(size: u32, rgb: [u8; 3], alpha: impl Fn(f32, f32, f32) -> f32) -> Image {
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    let pixel = 2.0 / size as f32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) * pixel - 1.0;
            let v = (y as f32 + 0.5) * pixel - 1.0;
            let a = alpha(u, v, pixel).clamp(0.0, 1.0);
            data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (a * 255.0).round() as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// A scope's lens: clear in the middle, with a duplex reticle (thin lines
/// through the centre, thick posts towards the edge), darkening to black at
/// the rim and beyond it.
fn scope_lens_image() -> Image {
    let rim = 0.97;
    let (thin, thick, posts_from) = (0.0026, 0.012, 0.36);
    mask_image(SCOPE_LENS_SIZE, [0, 0, 0], |u, v, pixel| {
        // How much of a pixel lies within `half` of a line `distance` away.
        let cover = |distance: f32, half: f32| ((half - distance) / pixel + 0.5).clamp(0.0, 1.0);
        let r = (u * u + v * v).sqrt();
        let outside = ((r - rim) / pixel + 0.5).clamp(0.0, 1.0);
        let shade = ((r - (rim - 0.16)) / 0.16).clamp(0.0, 1.0).powi(2) * 0.85;
        let posts =
            |along: f32, across: f32| cover(across.abs(), thick) * ((along.abs() - posts_from) / pixel + 0.5).clamp(0.0, 1.0);
        let reticle = cover(u.abs(), thin)
            .max(cover(v.abs(), thin))
            .max(posts(u, v))
            .max(posts(v, u));
        outside.max(shade).max(reticle)
    })
}

/// How many hits from different sides can show at once.
const DAMAGE_ARCS: usize = 6;
/// Pixels across the ring the arcs lie on.
const DAMAGE_RING: f32 = 170.0;

/// One of the arcs around the crosshair that point to where a hit came from
/// (see `LocalPlayer::damage_arcs`).
#[derive(Component)]
struct DamageArcNode(usize);

/// A thin arc at the top of a ring, fading towards its ends. Turned about the
/// middle, it points any way.
fn damage_arc_image() -> Image {
    let (radius, half_width, half_angle) = (0.9, 0.035, 0.38);
    mask_image(256, [255, 255, 255], |u, v, pixel| {
        let r = (u * u + v * v).sqrt();
        // Clockwise from straight up.
        let angle = u.atan2(-v);
        let across = ((half_width - (r - radius).abs()) / pixel + 0.5).clamp(0.0, 1.0);
        let along = (1.0 - angle.abs() / half_angle).clamp(0.0, 1.0);
        across * along.powf(0.6)
    })
}

fn update_damage_arcs(
    player: Res<LocalPlayer>,
    mut arcs: Query<(&DamageArcNode, &mut ImageNode, &mut UiTransform)>,
) {
    for (arc, mut image, mut transform) in &mut arcs {
        let Some(hit) = player.damage_arcs.get(arc.0) else {
            if image.color != Color::NONE {
                image.color = Color::NONE;
            }
            continue;
        };
        // Full for a moment, then it fades.
        let fade = (hit.left / DAMAGE_ARC_TIME * 1.6).min(1.0);
        image.color = Color::srgba(1.0, 0.3, 0.22, 0.85 * fade);
        transform.rotation = Rot2::radians(player.bearing(hit.at));
    }
}

fn update_scope_view(
    player: Res<LocalPlayer>,
    mut view: Single<&mut Visibility, With<ScopeView>>,
    mut lens: Single<&mut ImageNode, With<ScopeLens>>,
    mut sides: Query<&mut BackgroundColor, With<ScopeSide>>,
) {
    let fade = player.scope_view();
    view.set_if_neq(if fade > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    let color = Color::srgba(1.0, 1.0, 1.0, fade);
    if lens.color != color {
        lens.color = color;
    }
    for mut side in &mut sides {
        side.set_if_neq(BackgroundColor(Color::srgba(0.0, 0.0, 0.0, fade)));
    }
}

/// The crosshair's dot, hidden while aiming through a sight that has its own.
#[derive(Component)]
struct Crosshair;

fn update_crosshair(
    player: Res<LocalPlayer>,
    fitted: Res<FittedModels>,
    mut crosshair: Single<&mut Visibility, With<Crosshair>>,
) {
    let sight = fitted
        .get(player.weapon_id())
        .is_some_and(|f| f.aim_point(player.sight_id()).is_some());
    crosshair.set_if_neq(if sight && player.aim > 0.9 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
}

/// Shows how wide a choked gun's shots will spread (see `Weapon::choke`).
#[derive(Component)]
struct ChokeRing;

fn update_choke_ring(
    player: Res<LocalPlayer>,
    window: Single<&Window, With<PrimaryWindow>>,
    projection: Single<&Projection, With<MainCamera>>,
    ring: Single<(&mut Node, &mut BorderColor, &mut Visibility), With<ChokeRing>>,
) {
    let (mut node, mut border, mut visibility) = ring.into_inner();
    let shown = player.alive && player.weapon().choke.is_some() && player.aim > 0.05;
    visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    if !shown {
        return;
    }
    let fov = match *projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => 1.0,
    };
    // Where the edge of the cone lands on the screen.
    let radius = player.spread().tan() / (fov / 2.0).tan() * window.height() / 2.0;
    let size = px((radius * 2.0).round());
    if node.width != size {
        node.width = size;
        node.height = size;
    }
    let color = if player.choke() >= 1.0 {
        ACCENT.with_alpha(0.85 * player.aim)
    } else {
        Color::srgba(1.0, 1.0, 1.0, 0.55 * player.aim)
    };
    if border.top != color {
        *border = BorderColor::all(color);
    }
}

fn update_overlays(
    input: ActionInput,
    session: Res<Session>,
    map: Res<GameMap>,
    player: Res<LocalPlayer>,
    smoke_fog: Res<SmokeFog>,
    controls: Res<Controls>,
    mut overlays: Query<(&Overlay, &mut BackgroundColor)>,
    mut nodes: ParamSet<(
        Single<&mut Node, With<ScoreboardPanel>>,
        Single<&mut Node, With<PausePanel>>,
        Query<&mut Node, With<LeaderOnly>>,
        Single<&mut Node, With<DeathScreen>>,
        Single<&mut Node, With<SpeedPanel>>,
    )>,
    mut scoreboard_title: Single<&mut Text, With<ScoreboardText>>,
) {
    // White ticks for a hit, yellow for a headshot; red, and held a little
    // longer, for a kill.
    let tick_color = if player.kill_marker > 0.0 {
        Color::srgba(1.0, 0.15, 0.1, (player.kill_marker * 3.0).min(1.0))
    } else if player.headshot_marker > 0.0 {
        Color::srgba(1.0, 0.85, 0.1, (player.headshot_marker / HIT_MARKER_TIME * 2.0).clamp(0.0, 1.0))
    } else {
        Color::srgba(1.0, 1.0, 1.0, (player.hit_marker / HIT_MARKER_TIME * 2.0).clamp(0.0, 1.0))
    };
    for (overlay, mut background) in &mut overlays {
        let wanted = match overlay {
            Overlay::DamageFlash => Color::srgba(0.8, 0.0, 0.0, player.damage_flash * 0.35),
            // Pure white until the last second, then it clears.
            Overlay::Blind => Color::srgba(1.0, 1.0, 1.0, player.blind.min(1.0)),
            Overlay::Smoke => Color::srgba(0.6, 0.62, 0.62, smoke_fog.0 * 0.97),
            Overlay::HitTick => tick_color,
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }

    let paused = !controls.captured;
    let show_scores = input.pressed(Action::Scores);
    set_shown(&mut nodes.p3(), player.on_death_screen());
    set_shown(&mut nodes.p4(), !player.on_death_screen());
    set_shown(&mut nodes.p0(), show_scores);
    set_shown(&mut nodes.p1(), paused);
    for mut node in &mut nodes.p2() {
        set_shown(&mut node, session.is_leader());
    }
    if show_scores {
        let mode = MODE_NAMES.get(session.mode as usize).copied().unwrap_or("?");
        let map_name = MAP_NAMES.get(map.0.id as usize).copied().unwrap_or("?");
        set_text(&mut scoreboard_title, format!("{mode}  -  {map_name}"));
    }
}


