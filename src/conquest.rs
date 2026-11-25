//! Conquest on the client: the flags on their poles and the rings round
//! them, a row of flags under the clock, a bar while you stand on one,
//! markers over them in the world, and a word when one changes hands.
//! The server decides who holds what (`Session::flags`).

use bevy::{light::NotShadowCaster, prelude::*, window::PrimaryWindow};

use crate::{
    AppState,
    audio::{Sound, SoundCue},
    combat::Notices,
    net::Session,
    ping::on_screen,
    player::{LocalPlayer, PlayerSystems},
    protocol::{FlagSnap, MODE_CONQUEST},
    remote::RemotePlayer,
    ui::{BLUE, DIM, RED, TEXT, label, set_text},
    world::{GameMap, MainCamera},
};

/// Where the cloth hangs at the top of a pole (`flagpole` in `props.rs`).
const POLE_TOP: f32 = 8.6;
const CLOTH_LOWEST: f32 = 1.4;
const NEUTRAL: Color = Color::srgb(0.86, 0.86, 0.82);
const BADGE: f32 = 34.0;

pub struct ConquestPlugin;

impl Plugin for ConquestPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_materials)
            .add_systems(Update, (build_flags.run_if(resource_changed::<GameMap>), show_flags).chain())
            .add_systems(OnEnter(AppState::Playing), spawn_hud)
            .add_systems(
                Update,
                (update_badges, update_capture_bar, update_markers.after(PlayerSystems::Camera), announce)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(Resource)]
struct FlagMaterials {
    /// Neutral, ours, theirs: the cloth, then the ring on the ground.
    cloth: [Handle<StandardMaterial>; 3],
    ring: [Handle<StandardMaterial>; 3],
}

#[derive(Component)]
struct FlagCloth(usize);

#[derive(Component)]
struct FlagRing(usize);

#[derive(Component)]
struct FlagBadge(usize);

/// The strip along the bottom of a badge: how far the flag leans one way.
#[derive(Component)]
struct BadgeFill(usize);

#[derive(Component)]
struct CaptureBar;

#[derive(Component)]
struct CaptureText;

#[derive(Component)]
struct CaptureFill;

#[derive(Component)]
struct Marker(usize);

#[derive(Component)]
struct MarkerLetter(usize);

#[derive(Component)]
struct MarkerDistance(usize);

fn make_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let cloth = [NEUTRAL, BLUE, RED].map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 0.25,
            cull_mode: None,
            double_sided: true,
            perceptual_roughness: 0.9,
            ..default()
        })
    });
    let ring = [NEUTRAL, BLUE, RED].map(|color| {
        materials.add(StandardMaterial {
            base_color: color.with_alpha(0.55),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })
    });
    commands.insert_resource(FlagMaterials { cloth, ring });
}

/// A cloth on every flagpole and a ring round every flag.
fn build_flags(
    mut commands: Commands,
    map: Res<GameMap>,
    materials: Res<FlagMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    old: Query<Entity, Or<(With<FlagCloth>, With<FlagRing>)>>,
) {
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let cloth = meshes.add(Cuboid::new(1.5, 0.95, 0.03));
    for (index, flag) in map.0.flags.iter().enumerate() {
        commands.spawn((
            FlagCloth(index),
            Mesh3d(cloth.clone()),
            MeshMaterial3d(materials.cloth[0].clone()),
            Transform::from_translation(flag.pos + Vec3::new(0.8, CLOTH_LOWEST, 0.0)),
            Visibility::Hidden,
        ));
        let ring = Extrusion::new(Annulus::new(flag.radius - 0.3, flag.radius), 0.04);
        commands.spawn((
            FlagRing(index),
            Mesh3d(meshes.add(ring)),
            MeshMaterial3d(materials.ring[0].clone()),
            Transform::from_translation(flag.pos + Vec3::Y * 0.06)
                .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            NotShadowCaster,
            Visibility::Hidden,
        ));
    }
}

/// Which of `FlagMaterials` a flag wears (0 neutral, 1 ours and blue, 2 the enemy's and red), and
/// how high its cloth is up the pole (0 to 1): a held flag at the top, one
/// being taken rising in its taker's colour, a neutral one at the bottom.
fn flag_look(flag: &FlagSnap, mine: u8) -> (usize, f32) {
    let control = flag.control();
    let leaning = if control < -0.01 {
        Some(0)
    } else if control > 0.01 {
        Some(1)
    } else {
        None
    };
    (team_shade(flag.owner.or(leaning), mine), control.abs())
}

/// The shade of a flag held by `owner`, seen from team `mine`: 0 neutral,
/// 1 ours (blue), 2 the enemy's (red).
fn team_shade(owner: Option<u8>, mine: u8) -> usize {
    owner.map_or(0, |team| 1 + ((team ^ mine) & 1) as usize)
}

fn show_flags(
    time: Res<Time>,
    session: Res<Session>,
    map: Res<GameMap>,
    materials: Res<FlagMaterials>,
    mut cloths: Query<(&FlagCloth, &mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>), Without<FlagRing>>,
    mut rings: Query<(&FlagRing, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>), Without<FlagCloth>>,
) {
    let mine = session.team_of(session.my_id);
    let on = session.mode == MODE_CONQUEST && !session.flags.is_empty();
    let wanted = if on { Visibility::Inherited } else { Visibility::Hidden };
    let t = time.elapsed_secs();
    for (cloth, mut transform, mut visibility, mut material) in &mut cloths {
        visibility.set_if_neq(wanted);
        let (Some(flag), Some(point)) = (session.flags.get(cloth.0), map.0.flags.get(cloth.0)) else {
            continue;
        };
        let (shade, height) = flag_look(flag, mine);
        if material.0 != materials.cloth[shade] {
            material.0 = materials.cloth[shade].clone();
        }
        let y = CLOTH_LOWEST + (POLE_TOP - CLOTH_LOWEST) * height;
        // Flapping a little in the wind.
        let flap = (t * 2.3 + cloth.0 as f32).sin() * 0.25;
        let turn = Quat::from_rotation_y(flap);
        transform.translation = point.pos + Vec3::Y * y + turn * Vec3::X * 0.8;
        transform.rotation = turn;
    }
    for (ring, mut visibility, mut material) in &mut rings {
        visibility.set_if_neq(wanted);
        let Some(flag) = session.flags.get(ring.0) else {
            continue;
        };
        let shade = team_shade(flag.owner, mine);
        if material.0 != materials.ring[shade] {
            material.0 = materials.ring[shade].clone();
        }
    }
}

/// A thin dark edge, so the words read against the sky or the ground.
fn shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::splat(1.5),
        color: Color::srgba(0.0, 0.0, 0.0, 0.8),
    }
}

fn shade_color(shade: usize) -> Color {
    match shade {
        0 => NEUTRAL,
        1 => BLUE,
        _ => RED,
    }
}

fn spawn_hud(mut commands: Commands, session: Res<Session>, map: Res<GameMap>) {
    if session.mode != MODE_CONQUEST {
        return;
    }
    let names: Vec<char> = map.0.flags.iter().map(|flag| flag.name).collect();
    let badges = names.into_iter().enumerate().map(|(index, name)| {
        (
            FlagBadge(index),
            Node {
                width: px(BADGE),
                height: px(BADGE),
                border: UiRect::all(px(2)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(NEUTRAL),
            BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.6)),
            children![
                label(name.to_string(), 18.0, TEXT),
                (
                    BadgeFill(index),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        bottom: px(0),
                        height: px(4),
                        width: percent(0),
                        ..default()
                    },
                    BackgroundColor(NEUTRAL),
                ),
            ],
        )
    });
    commands.spawn((
        crate::ui::HudScaled,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            top: px(102),
            justify_content: JustifyContent::Center,
            column_gap: px(8),
            ..default()
        },
        Pickable::IGNORE,
        DespawnOnExit(AppState::Playing),
        Children::spawn(SpawnIter(badges)),
    ));

    // The capture bar, under the crosshair.
    commands.spawn((
        CaptureBar,
        crate::ui::HudScaled,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            top: percent(70),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            ..default()
        },
        Visibility::Hidden,
        Pickable::IGNORE,
        DespawnOnExit(AppState::Playing),
        children![
            (CaptureText, label("", 18.0, TEXT), shadow()),
            (
                Node {
                    width: px(260),
                    height: px(8),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.6)),
                children![(
                    CaptureFill,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(NEUTRAL),
                )],
            ),
        ],
    ));

    // A marker over each flag in the world.
    for (index, flag) in map.0.flags.iter().enumerate() {
        commands.spawn((
            Marker(index),
            Node {
                position_type: PositionType::Absolute,
                width: px(60),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            DespawnOnExit(AppState::Playing),
            children![
                (label(flag.name.to_string(), 20.0, NEUTRAL), shadow(), MarkerLetter(index)),
                (label("", 12.0, DIM), shadow(), MarkerDistance(index)),
            ],
        ));
    }
}

fn update_badges(
    session: Res<Session>,
    mut badges: Query<(&FlagBadge, &mut BorderColor, &mut BackgroundColor), Without<BadgeFill>>,
    mut fills: Query<(&BadgeFill, &mut Node, &mut BackgroundColor), Without<FlagBadge>>,
) {
    let mine = session.team_of(session.my_id);
    for (badge, mut border, mut background) in &mut badges {
        let Some(flag) = session.flags.get(badge.0) else {
            continue;
        };
        let owner = team_shade(flag.owner, mine);
        let color = shade_color(owner);
        border.set_if_neq(BorderColor::all(color));
        let fill = if owner == 0 { Color::srgba(0.05, 0.06, 0.08, 0.6) } else { color.with_alpha(0.45) };
        background.set_if_neq(BackgroundColor(fill));
    }
    for (fill, mut node, mut background) in &mut fills {
        let Some(flag) = session.flags.get(fill.0) else {
            continue;
        };
        let (shade, amount) = flag_look(flag, mine);
        let width = percent(amount * 100.0);
        if node.width != width {
            node.width = width;
        }
        background.set_if_neq(BackgroundColor(shade_color(shade)));
    }
}

/// While we stand on a flag: what is happening to it, and how far along.
fn update_capture_bar(
    session: Res<Session>,
    map: Res<GameMap>,
    player: Res<LocalPlayer>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    mut bar: Query<&mut Visibility, With<CaptureBar>>,
    mut text: Query<(&mut Text, &mut TextColor), With<CaptureText>>,
    mut fill: Query<(&mut Node, &mut BackgroundColor), With<CaptureFill>>,
) {
    let on = player.alive && session.mode == MODE_CONQUEST;
    let here = map.0.flags.iter().position(|flag| on && flag.holds(player.pos));
    for mut visibility in &mut bar {
        visibility.set_if_neq(if here.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    }
    let (Some(index), Some(state)) = (here, here.and_then(|i| session.flags.get(i))) else {
        return;
    };
    let point = &map.0.flags[index];
    let mine = session.team_of(session.my_id);
    let enemies_here = remotes
        .iter()
        .any(|(remote, transform)| remote.alive && session.enemies(session.my_id, remote.id) && point.holds(transform.translation));
    let ours = if mine == 0 { -state.control() } else { state.control() };
    let (words, color) = if enemies_here {
        ("CONTESTED", TEXT)
    } else if state.owner == Some(mine) && ours >= 0.999 {
        ("SECURED", BLUE)
    } else if state.owner.is_some_and(|team| team != mine) {
        ("NEUTRALIZING", BLUE)
    } else {
        ("CAPTURING", BLUE)
    };
    for (mut text, mut text_color) in &mut text {
        set_text(&mut text, format!("{words} {}", point.name));
        text_color.set_if_neq(TextColor(color));
    }
    let (shade, amount) = flag_look(state, mine);
    for (mut node, mut background) in &mut fill {
        let width = percent(amount * 100.0);
        if node.width != width {
            node.width = width;
        }
        background.set_if_neq(BackgroundColor(shade_color(shade)));
    }
}

fn update_markers(
    session: Res<Session>,
    map: Res<GameMap>,
    player: Res<LocalPlayer>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Transform, &Projection), With<MainCamera>>,
    mut markers: Query<(&Marker, &mut Node, &mut Visibility)>,
    mut letters: Query<(&MarkerLetter, &mut TextColor)>,
    mut distances: Query<(&MarkerDistance, &mut Text)>,
) {
    let (camera, projection) = camera.into_inner();
    let fov = match projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => 1.0,
    };
    let size = crate::ui::ui_size(&window, &ui_scale);
    for (marker, mut node, mut visibility) in &mut markers {
        let Some(point) = map.0.flags.get(marker.0) else {
            continue;
        };
        let above = point.pos + Vec3::Y * (POLE_TOP + 1.5);
        // Not while standing on it (the bar says it all), nor while dead.
        let shown = player.alive && !point.holds(player.pos);
        let at = shown.then(|| on_screen(camera, fov, size, above)).flatten();
        let Some(at) = at else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let (left, top) = (px(at.x - 30.0), px(at.y - 24.0));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
    for (letter, mut color) in &mut letters {
        if let Some(state) = session.flags.get(letter.0) {
            color.set_if_neq(TextColor(shade_color(flag_look(state, session.team_of(session.my_id)).0)));
        }
    }
    for (text, mut value) in &mut distances {
        if let Some(point) = map.0.flags.get(text.0) {
            set_text(&mut value, format!("{:.0} m", point.pos.distance(player.pos)));
        }
    }
}

/// A line under the kill notice, and a sound, when a flag changes hands.
fn announce(
    session: Res<Session>,
    map: Res<GameMap>,
    mut last: Local<Vec<Option<u8>>>,
    mut notices: ResMut<Notices>,
    mut sounds: MessageWriter<SoundCue>,
) {
    let owners: Vec<Option<u8>> = session.flags.iter().map(|flag| flag.owner).collect();
    if last.len() != owners.len() {
        *last = owners;
        return;
    }
    let mine = session.team_of(session.my_id);
    for (index, (before, now)) in last.iter().zip(&owners).enumerate() {
        if before == now {
            continue;
        }
        let name = map.0.flags.get(index).map_or('?', |flag| flag.name);
        let line = match (before, now) {
            (_, Some(team)) if *team == mine => format!("We took {name}"),
            (_, Some(_)) => format!("The enemy took {name}"),
            (Some(team), None) if *team == mine => format!("We lost {name}"),
            _ => format!("{name} is neutral"),
        };
        notices.banner = Some((line, 3.0));
        sounds.write(SoundCue {
            sound: if *now == Some(mine) { Sound::Go } else { Sound::Ping },
            at: None,
        });
    }
    *last = owners;
}
