//! The weapon panel at the bottom right of the HUD: the grenades carried, a
//! picture of the weapon in hand, and its ammo, with a ring that fills
//! around the seconds left while reloading (or, loading round by round, for
//! each round, around the rounds still to load).
//!
//! Only the weapon in hand shows. On a switch its picture slides in, and a
//! strip with all three slots shows over the panel for a moment. The panel
//! fades after a few quiet seconds, but never while reloading or low on ammo.
//!
//! Bevy UI has no opacity for a whole group, so every part's colour is
//! written here each frame with the fade worked in.

use bevy::prelude::*;

use crate::{
    AppState,
    loadout::{Preview, THUMB_SIZE, Thumbnails},
    player::{KNIFE_SLOT, LocalPlayer},
    protocol::{MOST_GRENADES, grenade},
    settings::{Action, Settings},
    ui::{ACCENT, DIM, HUD_EDGE, HUD_PANEL, TEXT, TRACK, hud_shadow, label, mask_image, set_shown, set_text},
    weapons::{IRON_SIGHTS, KNIFE},
};

/// Seconds without anything happening before the panel fades...
const IDLE_AFTER: f32 = 3.0;
/// ...down to this much of itself.
const IDLE_ALPHA: f32 = 0.4;
/// Seconds the strip of slots shows after a switch.
const STRIP_TIME: f32 = 1.6;
/// How many steps the reload ring fills in.
const RING_STEPS: usize = 48;
/// Size of a grenade icon on the panel, in pixels.
const GRENADE_ICON: Vec2 = Vec2::new(20.0, 22.0);

pub struct WeaponPanelPlugin;

impl Plugin for WeaponPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_ring)
            .add_systems(OnEnter(AppState::Playing), spawn_panel)
            .add_systems(Update, update_panel.run_if(in_state(AppState::Playing)));
    }
}

/// The reload ring: its track, and the fill at each step from empty to full.
#[derive(Resource)]
struct Ring {
    track: Handle<Image>,
    fills: Vec<Handle<Image>>,
}

/// Radius and half the thickness of the ring, with the image running from
/// -1 to 1 across.
const RING_RADIUS: f32 = 0.81;
const RING_HALF_WIDTH: f32 = 0.095;

fn make_ring(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let track = images.add(mask_image(80, [255, 255, 255], |u, v, pixel| {
        let r = (u * u + v * v).sqrt();
        (RING_HALF_WIDTH - (r - RING_RADIUS).abs()) / pixel + 0.5
    }));
    let fills = (0..=RING_STEPS)
        .map(|step| {
            let sweep = step as f32 / RING_STEPS as f32 * std::f32::consts::TAU;
            // The ends of the arc: the top, and `sweep` clockwise from it.
            let end = Vec2::new(sweep.sin(), -sweep.cos()) * RING_RADIUS;
            let start = Vec2::new(0.0, -RING_RADIUS);
            images.add(mask_image(80, [255, 255, 255], move |u, v, pixel| {
                if step == 0 {
                    return 0.0;
                }
                let r = (u * u + v * v).sqrt();
                let angle = u.atan2(-v).rem_euclid(std::f32::consts::TAU);
                let point = Vec2::new(u, v);
                // On the arc it is a band; past its ends, round caps.
                let distance = if angle <= sweep {
                    (r - RING_RADIUS).abs()
                } else {
                    point.distance(start).min(point.distance(end))
                };
                (RING_HALF_WIDTH - distance) / pixel + 0.5
            }))
        })
        .collect();
    commands.insert_resource(Ring { track, fills });
}

/// The parts of the panel, each coloured and shown by `update_panel`.
#[derive(Component, Clone, Copy, PartialEq)]
enum Part {
    Panel,
    GrenadeLabel,
    /// One grenade: its box (hidden past what the grenade kind carries), the
    /// grey picture under it, the part that fills from the bottom while it
    /// recharges, and the coloured picture in that.
    GrenadeIcon(usize),
    GrenadeBase(usize),
    GrenadeClip(usize),
    GrenadeFill(usize),
    Separator,
    Gun,
    /// The ammo left, and the magazine size after it.
    Ammo,
    Magazine,
    /// The weapon's name, for the knife, which has no ammo.
    Title,
    /// While reloading, in place of the ammo: the ring, and the seconds left.
    Ring,
    RingTrack,
    RingFill,
    RingText,
    BarTrack,
    BarFill,
    /// The strip over the panel after a switch, and one slot in it.
    Strip,
    StripChip(usize),
    StripKey(usize),
    StripImage(usize),
}

/// Text on the panel, with a shadow so it reads against anything.
fn shadowed(text: &'static str, size: f32, part: Part) -> impl Bundle {
    (label(text, size, TEXT), hud_shadow(), part)
}

fn spawn_panel(mut commands: Commands) {
    let grenade_icon = |index: usize| {
        let picture = |part: Part| {
            (
                part,
                ImageNode {
                    rect: Some(grenade_rect()),
                    ..default()
                }
                .with_color(Color::NONE),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    bottom: px(0),
                    width: px(GRENADE_ICON.x),
                    height: px(GRENADE_ICON.y),
                    ..default()
                },
            )
        };
        (
            Part::GrenadeIcon(index),
            Node {
                width: px(GRENADE_ICON.x),
                height: px(GRENADE_ICON.y),
                ..default()
            },
            UiTransform::default(),
            children![
                picture(Part::GrenadeBase(index)),
                (
                    Part::GrenadeClip(index),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        bottom: px(0),
                        width: percent(100),
                        height: percent(100),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    children![picture(Part::GrenadeFill(index))],
                ),
            ],
        )
    };
    let strip_chip = |slot: usize| {
        (
            Part::StripChip(slot),
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                padding: UiRect::axes(px(8), px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
            children![
                shadowed("", 12.0, Part::StripKey(slot)),
                (
                    Part::StripImage(slot),
                    ImageNode::default().with_color(Color::NONE),
                    Node {
                        width: px(59),
                        height: px(22),
                        ..default()
                    },
                ),
            ],
        )
    };
    let ring_image = |part: Part| {
        (
            part,
            ImageNode::default().with_color(Color::NONE),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        )
    };

    commands.spawn((
        crate::ui::HudScaled,
        crate::ui::HiddenWhenDead,
        crate::ui::ArmedOnly,
        Node {
            position_type: PositionType::Absolute,
            right: px(20),
            bottom: px(18),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::End,
            row_gap: px(6),
            ..default()
        },
        Pickable::IGNORE,
        DespawnOnExit(AppState::Playing),
        children![
            (
                Part::Strip,
                Node {
                    column_gap: px(4),
                    padding: UiRect::all(px(4)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::all(Color::NONE),
                children![strip_chip(0), strip_chip(1), strip_chip(2)],
            ),
            (
                Part::Panel,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    padding: UiRect::axes(px(12), px(7)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(8)),
                    ..default()
                },
                BackgroundColor(HUD_PANEL),
                BorderColor::all(HUD_EDGE),
                children![
                    (
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            ..default()
                        },
                        children![
                            (
                                Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(3),
                                    ..default()
                                },
                                children![
                                    (shadowed("GRENADE", 11.0, Part::GrenadeLabel), Node {
                                        margin: UiRect::right(px(2)),
                                        ..default()
                                    }),
                                    grenade_icon(0),
                                    grenade_icon(1),
                                ],
                            ),
                            (
                                Part::Separator,
                                Node {
                                    width: px(1),
                                    height: px(26),
                                    ..default()
                                },
                                BackgroundColor(HUD_EDGE),
                            ),
                            (
                                Part::Gun,
                                ImageNode::default().with_color(Color::NONE),
                                Node {
                                    width: px(88),
                                    height: px(33),
                                    ..default()
                                },
                                UiTransform::default(),
                            ),
                            // The ammo, the knife's name, or the reload ring,
                            // pushed to the right.
                            (
                                Node {
                                    min_width: px(92),
                                    height: px(40),
                                    justify_content: JustifyContent::End,
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                children![
                                    (
                                        Node {
                                            align_items: AlignItems::Baseline,
                                            column_gap: px(6),
                                            ..default()
                                        },
                                        children![
                                            (shadowed("", 32.0, Part::Ammo), UiTransform::default()),
                                            shadowed("", 16.0, Part::Magazine),
                                        ],
                                    ),
                                    shadowed("", 13.0, Part::Title),
                                    (
                                        Part::Ring,
                                        Node {
                                            width: px(40),
                                            height: px(40),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        children![
                                            ring_image(Part::RingTrack),
                                            ring_image(Part::RingFill),
                                            shadowed("", 12.0, Part::RingText),
                                        ],
                                    ),
                                ],
                            ),
                        ],
                    ),
                    (
                        Part::BarTrack,
                        Node {
                            width: percent(100),
                            height: px(6),
                            border_radius: BorderRadius::all(px(3)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(TRACK),
                        children![(
                            Part::BarFill,
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(0),
                                top: px(0),
                                width: percent(0),
                                height: percent(100),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(TEXT),
                        )],
                    ),
                ],
            ),
        ],
    ));
}

/// What the panel remembers from frame to frame, to notice what changed.
#[derive(Default)]
struct PanelState {
    /// What was in hand, and the ammo in it, last frame.
    slot: usize,
    weapon: Option<(u8, u32)>,
    grenades_ready: u32,
    /// Seconds since: anything happened, the last switch, the last shot, and
    /// each grenade became ready.
    since_action: f32,
    since_switch: f32,
    since_shot: f32,
    since_ready: [f32; MOST_GRENADES],
    /// How much of itself the panel shows (it fades when idle).
    alpha: f32,
}

/// A thumbnail of a grenade is wide with the grenade in the middle: this is
/// the middle, the shape of an icon.
fn grenade_rect() -> Rect {
    let (width, height) = (THUMB_SIZE.x as f32, THUMB_SIZE.y as f32);
    let across = height * GRENADE_ICON.x / GRENADE_ICON.y;
    Rect::new((width - across) / 2.0, 0.0, (width + across) / 2.0, height)
}

fn update_panel(
    time: Res<Time>,
    player: Res<LocalPlayer>,
    settings: Res<Settings>,
    thumbnails: Res<Thumbnails>,
    ring: Res<Ring>,
    mut state: Local<PanelState>,
    mut parts: Query<(
        &Part,
        &mut Node,
        Option<&mut Text>,
        Option<&mut TextColor>,
        Option<&mut ImageNode>,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
        Option<&mut UiTransform>,
    )>,
) {
    let dt = time.delta_secs();
    let state = &mut *state;
    let gun = player.guns.get(player.slot);
    let stats = player.weapon();
    let ammo = gun.map(|gun| gun.ammo);
    let low = gun.is_some_and(|gun| gun.ammo * 4 <= stats.magazine);
    let reloading = gun.is_some() && player.reloading();
    let carried = grenade(player.grenade);
    let ready = player.grenades.floor() as u32;

    // What happened since last frame.
    state.since_action += dt;
    state.since_switch += dt;
    state.since_shot += dt;
    for since in &mut state.since_ready {
        *since += dt;
    }
    let in_hand = gun.map(|gun| (gun.weapon, gun.ammo));
    if player.slot != state.slot {
        state.slot = player.slot;
        state.since_switch = 0.0;
        state.since_action = 0.0;
    } else if let (Some((weapon, now)), Some((was_weapon, was))) = (in_hand, state.weapon)
        && weapon == was_weapon
        && now < was
    {
        state.since_shot = 0.0;
    }
    state.weapon = in_hand;
    if ready > state.grenades_ready
        && let Some(since) = state.since_ready.get_mut(state.grenades_ready as usize)
    {
        *since = 0.0;
    }
    if ready < state.grenades_ready {
        state.since_action = 0.0;
    }
    state.grenades_ready = ready;
    if player.recoil > 0.0 || reloading || player.hand.is_some() {
        state.since_action = 0.0;
    }
    let idle = state.since_action > IDLE_AFTER && !reloading && !low;
    let target = if idle { IDLE_ALPHA } else { 1.0 };
    state.alpha = if state.alpha == 0.0 {
        1.0
    } else {
        state.alpha + (target - state.alpha) * (dt * 4.0).min(1.0)
    };
    let alpha = state.alpha;
    let fade = |color: Color| color.with_alpha(color.alpha() * alpha);

    // The strip comes in quickly, stays, and fades out.
    let t = state.since_switch;
    let strip = if t < STRIP_TIME * 0.12 {
        t / (STRIP_TIME * 0.12)
    } else if t < STRIP_TIME * 0.75 {
        1.0
    } else {
        ((STRIP_TIME - t) / (STRIP_TIME * 0.25)).max(0.0)
    };
    // The picture slides in from the right on a switch; the ammo kicks on a shot.
    let slide = (state.since_switch / 0.2).min(1.0);
    let kick = (1.0 - state.since_shot / 0.14).max(0.0);

    let in_hand_preview = |slot: usize| match player.guns.get(slot) {
        Some(gun) => Preview::gun(gun.weapon, gun.sight),
        None => Preview::Gun(KNIFE, IRON_SIGHTS),
    };
    // Loading round by round, the ring fills for each round.
    let reload_part = player.reload_progress();
    let ammo_color = if low {
        ACCENT
    } else if ammo == Some(stats.magazine) {
        // Full: a little dim.
        TEXT.with_alpha(0.75)
    } else {
        TEXT
    };

    for (part, mut node, text, text_color, image, background, border, transform) in &mut parts {
        let set_image = |handle: Option<Handle<Image>>, color: Color| {
            if let Some(mut image) = image {
                if let Some(handle) = handle
                    && image.image != handle
                {
                    image.image = handle;
                }
                if image.color != color {
                    image.color = color;
                }
            }
        };
        let set_color = |color: Color| {
            if let Some(mut text_color) = text_color
                && text_color.0 != color
            {
                text_color.0 = color;
            }
        };
        let write = |value: String| {
            if let Some(mut text) = text {
                set_text(&mut text, value);
            }
        };
        match *part {
            Part::Panel => {
                set_background(background, fade(HUD_PANEL));
                set_border(border, fade(HUD_EDGE));
            }
            Part::Strip => {
                let shown = strip > 0.0;
                set_shown(&mut node, shown);
                set_background(background, HUD_PANEL.with_alpha(HUD_PANEL.alpha() * strip));
                set_border(border, HUD_EDGE.with_alpha(HUD_EDGE.alpha() * strip));
            }
            Part::StripChip(slot) => {
                let on = slot == player.slot;
                set_background(background, Color::srgba(1.0, 1.0, 1.0, if on { 0.1 * strip } else { 0.0 }));
                set_border(border, ACCENT.with_alpha(if on { 0.7 * strip } else { 0.0 }));
            }
            Part::StripKey(slot) => {
                let action = [Action::Primary, Action::Secondary, Action::Knife][slot];
                write(settings.key_name(action));
                let color = if slot == player.slot { ACCENT } else { DIM };
                set_color(color.with_alpha(strip));
            }
            Part::StripImage(slot) => {
                let on = if slot == player.slot { 1.0 } else { 0.7 };
                set_image(thumbnails.get(in_hand_preview(slot)), Color::WHITE.with_alpha(on * strip));
            }
            Part::GrenadeLabel => set_color(fade(DIM)),
            Part::GrenadeIcon(index) => {
                set_shown(&mut node, (index as f32) < carried.max);
                // A pop when one is ready again.
                let pulse = (1.0 - state.since_ready.get(index).copied().unwrap_or(1.0) / 0.32).max(0.0);
                if let Some(mut transform) = transform {
                    let scale = Vec2::splat(1.0 + 0.5 * pulse * pulse);
                    if transform.scale != scale {
                        transform.scale = scale;
                    }
                }
            }
            Part::GrenadeBase(_) => {
                set_image(thumbnails.get(Preview::Grenade(player.grenade)), fade(Color::srgba(0.6, 0.6, 0.6, 0.3)));
            }
            Part::GrenadeClip(index) => {
                let fill = (player.grenades - index as f32).clamp(0.0, 1.0);
                let height = percent(fill * 100.0);
                if node.height != height {
                    node.height = height;
                }
            }
            Part::GrenadeFill(_) => {
                set_image(thumbnails.get(Preview::Grenade(player.grenade)), fade(Color::WHITE));
            }
            Part::Separator => set_background(background, fade(HUD_EDGE)),
            Part::Gun => {
                set_image(thumbnails.get(in_hand_preview(player.slot)), fade(Color::WHITE.with_alpha(slide)));
                if let Some(mut transform) = transform {
                    let translation = Val2::px(28.0 * (1.0 - slide).powi(2), 0.0);
                    if transform.translation != translation {
                        transform.translation = translation;
                    }
                }
            }
            Part::Ammo => {
                set_shown(&mut node, ammo.is_some() && !reloading);
                write(ammo.map_or(String::new(), |ammo| ammo.to_string()));
                set_color(fade(ammo_color));
                if let Some(mut transform) = transform {
                    let scale = Vec2::splat(1.0 + 0.22 * kick * kick);
                    if transform.scale != scale {
                        transform.scale = scale;
                    }
                }
            }
            Part::Magazine => {
                set_shown(&mut node, ammo.is_some() && !reloading);
                write(format!("/ {}", stats.magazine));
                set_color(fade(DIM));
            }
            Part::Title => {
                set_shown(&mut node, player.slot == KNIFE_SLOT);
                write(stats.name.to_uppercase());
                set_color(fade(DIM));
            }
            Part::Ring => set_shown(&mut node, reloading),
            Part::RingTrack => set_image(Some(ring.track.clone()), fade(TRACK)),
            Part::RingFill => {
                let step = (reload_part * RING_STEPS as f32).round() as usize;
                set_image(ring.fills.get(step.min(RING_STEPS)).cloned(), fade(ACCENT));
            }
            Part::RingText => {
                // The seconds left, or the rounds still to load.
                write(match ammo {
                    _ if player.racking => String::new(),
                    Some(ammo) if stats.reload_per_round => (stats.magazine - ammo).to_string(),
                    _ => format!("{:.1}", player.reload.max(0.0)),
                });
                set_color(fade(ACCENT));
            }
            Part::BarTrack => set_background(background, fade(TRACK)),
            Part::BarFill => {
                // Empty while reloading: the ring shows that.
                let part = match ammo {
                    Some(ammo) if !reloading => ammo as f32 / stats.magazine.max(1) as f32,
                    _ => 0.0,
                };
                let width = percent(part * 100.0);
                if node.width != width {
                    node.width = width;
                }
                set_background(background, fade(if low { ACCENT } else { TEXT }));
            }
        }
    }
}

fn set_background(background: Option<Mut<BackgroundColor>>, color: Color) {
    if let Some(mut background) = background
        && background.0 != color
    {
        background.0 = color;
    }
}

fn set_border(border: Option<Mut<BorderColor>>, color: Color) {
    if let Some(mut border) = border
        && border.top != color
    {
        *border = BorderColor::all(color);
    }
}
