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
    course::format_time,
    race::{BACK_TO_START, ordinal},
    grenade::SmokeFog,
    gun_model::FittedModels,
    menu::{Field, MenuPage},
    net::{self, Connection, Session},
    paint::{Skins, palette_color},
    loadout::{DeathLoadout, DeathLoadoutOpen, Preview, Thumbnails, death_loadout, loadout_summary, on_open_loadout, summary_button},
    paint_editor::on_open_paint,
    player::{Controls, DAMAGE_ARC_TIME, LocalPlayer, Racing},
    remote::Spectate,
    modes,
    protocol::*,
    settings::{Action, ActionInput, Settings, on_open_settings},
    weapons::{GRENADE, IRON_SIGHTS},
    world::{GameMap, MainCamera},
};

mod hud;
mod lobby;
mod scoreboard;

pub use hud::*;
pub use lobby::*;
use scoreboard::*;

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
/// The two colours, seen from the player's side: their own team is always
/// blue and the enemy red, whichever team number they are. See `team_color`.
pub const BLUE: Color = Color::srgb(0.30, 0.86, 1.0);
pub const RED: Color = Color::srgb(1.0, 0.42, 0.38);

/// The colour of team `team` as seen by a player on team `mine`: blue for
/// their own team, red for the other. Used wherever it shows: the outline
/// around teammates, their names, the score, the flags they hold.
pub fn team_color(mine: u8, team: u8) -> Color {
    if (mine ^ team) & 1 == 0 { BLUE } else { RED }
}

/// The name of team `team` as seen by a player on team `mine`: "Blue" or "Red".
pub fn team_name(mine: u8, team: u8) -> &'static str {
    if (mine ^ team) & 1 == 0 { "Blue" } else { "Red" }
}
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
            .insert(AssetId::default(), Font::from_bytes(include_bytes!("../fonts/Archivo.ttf").to_vec()))
            .expect("the default font's slot");
        app.add_plugins(TabNavigationPlugin)
            .init_resource::<PickerOpen>()
            .init_resource::<FinalScoresOpen>()
            .add_systems(OnEnter(AppState::Connecting), spawn_connecting)
            .add_systems(OnEnter(AppState::Lobby), spawn_lobby)
            .add_systems(OnEnter(AppState::Playing), spawn_hud)
            .add_systems(
                Update,
                (
                    scale_ui,
                    scale_hud,
                    style_buttons,
                    style_fields,
                    wheel_scroll,
                    update_connecting.run_if(in_state(AppState::Connecting)),
                    (update_lobby, style_match_choices, update_lobby_teams, style_join_slots)
                        .run_if(in_state(AppState::Lobby)),
                    update_rosters,
                    update_key_hints,
                    (
                        update_hud,
                        show_stats,
                        update_hud_shapes,
                        update_overlays,
                        update_crosshair,
                        update_scope_view,
                        update_damage_arcs,
                        update_choke_ring,
                        update_death_screen,
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

pub fn button_row() -> Node {
    Node {
        column_gap: px(10),
        align_items: AlignItems::Center,
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

/// The window size every screen is laid out for. They are all drawn bigger or
/// smaller to take the same share of any window.
pub const DESIGN_SIZE: Vec2 = Vec2::new(1280.0, 720.0);

/// Scales the whole interface to the window, by whichever of its width and
/// height is the tighter fit so nothing ever runs off a screen of another
/// shape, and then by the player's own "Menu and HUD size".
fn scale_ui(window: Single<&Window, With<PrimaryWindow>>, mut scale: ResMut<UiScale>) {
    let fit = (window.width() / DESIGN_SIZE.x).min(window.height() / DESIGN_SIZE.y);
    // A minimised window is 0 by 0.
    let wanted = fit.max(0.25);
    if (scale.0 - wanted).abs() > 1e-3 {
        scale.0 = wanted;
    }
}

/// A HUD element that the player's "HUD size" makes bigger or smaller.
#[derive(Component)]
pub struct HudScaled;

/// Sizes each HUD element by the player's "HUD size", about the corner or
/// edge of the screen it is pinned to: one in the bottom left grows up and to
/// the right, and elements stacked from the same corner keep clear of each
/// other. (The menus are never sized this way: they are laid out to fill the
/// window, see `scale_ui`.)
fn scale_hud(settings: Res<Settings>, mut elements: Query<(&Node, &ComputedNode, &mut UiTransform), With<HudScaled>>) {
    let scale = settings.ui_scale;
    // How far an edge is from the screen's edge, if the element is pinned by it.
    let offset = |edge: Val| match edge {
        Val::Px(px) => Some(px),
        _ => None,
    };
    for (node, computed, mut transform) in &mut elements {
        let half = computed.size() * computed.inverse_scale_factor / 2.0;
        // Its centre moves away from the corner as it grows.
        let shift = |near: Option<f32>, far: Option<f32>, half: f32| match (near, far) {
            (Some(near), _) => (scale - 1.0) * (near + half),
            (None, Some(far)) => -(scale - 1.0) * (far + half),
            (None, None) => 0.0,
        };
        let wanted = UiTransform {
            translation: Val2::px(
                shift(offset(node.left), offset(node.right), half.x),
                shift(offset(node.top), offset(node.bottom), half.y),
            ),
            scale: Vec2::splat(scale),
            rotation: Rot2::IDENTITY,
        };
        if *transform != wanted {
            *transform = wanted;
        }
    }
}

/// The window's size in the interface's own units (window pixels divided by
/// `UiScale`), for placing things over the 3D view.
pub fn ui_size(window: &Window, scale: &UiScale) -> Vec2 {
    Vec2::new(window.width(), window.height()) / scale.0
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
