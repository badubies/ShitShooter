//! The map from above: small in the top left corner while playing, with the
//! flags, teammates and an arrow for us; and in conquest, bigger on the
//! death screen, where a click on the base or a flag we hold picks where to
//! come back in.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    ui_widgets::{Activate, Button, observe},
};

use crate::{
    AppState,
    map::Map,
    net::{Connection, Session},
    player::LocalPlayer,
    props::{self, PROPS},
    protocol::{ClientMsg, FlagSnap, MODE_CONQUEST},
    remote::RemotePlayer,
    settings::{Action, ActionInput, Settings},
    ui::{ACCENT, ALLY, DIM, ENEMY, TEXT, label, mask_image, set_text},
    world::GameMap,
};

/// Pixels across the picture of the map.
const RES: usize = 256;
const MINIMAP: f32 = 180.0;
const DEPLOY: f32 = 320.0;
const NEUTRAL: Color = Color::srgb(0.86, 0.86, 0.82);

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SpawnChoice>()
            .add_systems(Startup, make_arrow)
            .add_systems(Update, draw_map.run_if(resource_changed::<GameMap>))
            .add_systems(OnEnter(AppState::Playing), (spawn_minimap, reset_choice))
            .add_systems(
                Update,
                (toggle_minimap, update_minimap, update_deploy_map, update_deploy_mates, send_choice)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

/// The picture of the current map, and the square of the world it shows
/// (its north-west corner and side, in metres).
#[derive(Resource)]
pub struct MapPicture {
    pub image: Handle<Image>,
    corner: Vec2,
    side: f32,
}

impl MapPicture {
    /// Where on a picture `size` pixels across a point in the world is.
    fn place(&self, at: Vec3, size: f32) -> Vec2 {
        (Vec2::new(at.x, at.z) - self.corner) / self.side * size
    }
}

/// Conquest: where we asked to come back in. `None`: wherever the server
/// thinks best; 0: our base; `1 + i`: flag `i`.
#[derive(Resource, Default)]
pub struct SpawnChoice {
    pub point: Option<u8>,
    sent: Option<Option<u8>>,
}

#[derive(Resource)]
struct Arrow(Handle<Image>);

#[derive(Component)]
struct Minimap;

#[derive(Component)]
struct MinimapFlag(usize);

#[derive(Component)]
struct MinimapMate;

#[derive(Component)]
struct MinimapSelf;

/// The death screen's map (see `deploy_map`).
#[derive(Component)]
pub struct DeployMap;

#[derive(Component)]
struct DeployPicture;

/// A place to come back in on the death screen's map (see `SpawnChoice`).
#[derive(Component)]
struct DeployPoint(u8);

#[derive(Component)]
struct DeployCaption;

#[derive(Component)]
struct DeployMate;

fn make_arrow(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // A triangle pointing up.
    let arrow = mask_image(64, [255, 255, 255], |u, v, _| {
        let inside = v > -0.9 && v < 0.75 && u.abs() < (v + 0.9) * 0.55;
        let notch = v > 0.35 && u.abs() < (v - 0.35) * 1.6;
        if inside && !notch { 1.0 } else { 0.0 }
    });
    commands.insert_resource(Arrow(images.add(arrow)));
}

/// Paints the map from above: the ground and its roads and water, then the
/// trees, rocks and buildings on it (or, on the indoor maps, the blocks).
fn draw_map(mut commands: Commands, map: Res<GameMap>, mut images: ResMut<Assets<Image>>) {
    let map = &map.0;
    let [min, max] = map.bounds;
    let margin = if map.terrain.is_some() { 12.0 } else { 4.0 };
    let half = ((max - min).max_element() / 2.0) + margin;
    let center = (min + max) / 2.0;
    let corner = center - Vec2::splat(half);
    let side = half * 2.0;
    let mut pixels = vec![[0.0_f32; 3]; RES * RES];
    let world = |i: usize, j: usize| corner + Vec2::new(i as f32 + 0.5, j as f32 + 0.5) / RES as f32 * side;
    let light = Vec3::new(-0.5, 0.75, -0.45).normalize();
    for j in 0..RES {
        for i in 0..RES {
            let at = world(i, j);
            pixels[j * RES + i] = ground_color(map, at, light);
        }
    }
    let mut paint = |center: Vec2, half: Vec2, round: bool, color: [f32; 3], blend: f32| {
        let lo = ((center - half - corner) / side * RES as f32).floor();
        let hi = ((center + half - corner) / side * RES as f32).ceil();
        for j in lo.y.max(0.0) as usize..(hi.y.max(0.0) as usize).min(RES) {
            for i in lo.x.max(0.0) as usize..(hi.x.max(0.0) as usize).min(RES) {
                let at = world(i, j);
                let inside = if round {
                    ((at - center) / half).length() <= 1.0
                } else {
                    ((at - center).abs() - half).max_element() <= 0.0
                };
                if inside {
                    let p = &mut pixels[j * RES + i];
                    for k in 0..3 {
                        p[k] += (color[k] - p[k]) * blend;
                    }
                }
            }
        }
    };
    for prop in &map.props {
        let at = Vec2::new(prop.pos.x, prop.pos.z);
        let name = PROPS[prop.model].name;
        if [props::PINE, props::PINE_SMALL, props::OAK, props::BIRCH].contains(&prop.model) {
            let r = if prop.model == props::PINE_SMALL { 1.6 } else { 2.6 } * prop.scale;
            paint(at, Vec2::splat(r), true, [0.13, 0.24, 0.11], 0.8);
        } else if prop.model == props::BUSH {
            paint(at, Vec2::splat(1.0 * prop.scale), true, [0.2, 0.32, 0.14], 0.5);
        } else if name.starts_with("rock") || prop.model == props::CLIFF {
            let r = if prop.model == props::CLIFF { 4.5 } else { 1.6 } * prop.scale;
            paint(at, Vec2::splat(r), true, [0.5, 0.49, 0.46], 0.9);
        } else {
            // Buildings and props: their footprint, darker the taller it is.
            let turn = Quat::from_rotation_y(prop.yaw);
            for [x0, y0, z0, x1, y1, z1] in PROPS[prop.model].colliders {
                if y1 - y0.max(0.0) < 0.6 {
                    continue;
                }
                let a = turn * Vec3::new(*x0, 0.0, *z0) * prop.scale;
                let b = turn * Vec3::new(*x1, 0.0, *z1) * prop.scale;
                let (lo, hi) = (a.min(b), a.max(b));
                let mid = at + Vec2::new(lo.x + hi.x, lo.z + hi.z) / 2.0;
                let half = Vec2::new(hi.x - lo.x, hi.z - lo.z) / 2.0;
                let shade = (0.62 - y1 * 0.025).clamp(0.3, 0.6);
                paint(mid, half, false, [shade, shade * 0.95, shade * 0.9], 1.0);
            }
        }
    }
    let data = pixels
        .iter()
        .flat_map(|c| {
            let srgb = Color::srgb(c[0], c[1], c[2]).to_srgba();
            [srgb.red, srgb.green, srgb.blue].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).into_iter().chain([255])
        })
        .collect();
    let image = Image::new(
        Extent3d {
            width: RES as u32,
            height: RES as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    commands.insert_resource(MapPicture {
        image: images.add(image),
        corner,
        side,
    });
}

/// The ground's colour at a point, lit from the north-west so hills show.
fn ground_color(map: &Map, at: Vec2, light: Vec3) -> [f32; 3] {
    let Some(terrain) = &map.terrain else {
        // Indoor maps: the top of the highest block there, lighter the higher.
        let mut top: Option<(f32, [f32; 3])> = None;
        for block in map.blocks.iter().filter(|b| b.visible) {
            let over = at.x >= block.min.x && at.x <= block.max.x && at.y >= block.min.z && at.y <= block.max.z;
            if over && block.max.y < 20.0 && top.is_none_or(|(y, _)| block.max.y > y) {
                top = Some((block.max.y, block.color));
            }
        }
        let (height, [r, g, b]) = top.unwrap_or((0.0, [0.1, 0.1, 0.12]));
        let lift = 0.8 + 0.06 * height.max(0.0);
        return [r * lift, g * lift, b * lift];
    };
    let n = terrain.size;
    let gx = (((at.x - terrain.origin.x) / terrain.cell).round().max(0.0) as usize).min(n - 1);
    let gz = (((at.y - terrain.origin.y) / terrain.cell).round().max(0.0) as usize).min(n - 1);
    let [r, g, b] = terrain.colors.get(gz * n + gx).copied().unwrap_or([0.33, 0.45, 0.19]);
    let shade = 0.55 + 0.6 * terrain.normal(at.x, at.y).dot(light).max(0.0);
    let mut color = [r * shade, g * shade, b * shade];
    for road in map.roads.iter().filter(|road| road.paved) {
        if distance_to_line(&road.points, at) < road.width / 2.0 {
            color = [0.2, 0.2, 0.21];
        }
    }
    for (points, width) in &map.water {
        let line: Vec<Vec2> = points.iter().map(|p| Vec2::new(p.x, p.z)).collect();
        if distance_to_line(&line, at) < width / 2.0 {
            color = [0.2, 0.36, 0.45];
        }
    }
    color
}

fn distance_to_line(points: &[Vec2], at: Vec2) -> f32 {
    points
        .windows(2)
        .map(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let ab = b - a;
            let t = ((at - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            at.distance(a + ab * t)
        })
        .fold(f32::INFINITY, f32::min)
}

fn reset_choice(mut choice: ResMut<SpawnChoice>) {
    *choice = SpawnChoice::default();
}

/// The colour a flag is shown in, from our side: neutral, ours or theirs.
fn flag_color(session: &Session, flag: Option<&FlagSnap>) -> Color {
    match flag.and_then(|flag| flag.owner) {
        None => NEUTRAL,
        Some(team) if team == session.team_of(session.my_id) => ALLY,
        Some(_) => ENEMY,
    }
}

fn spawn_minimap(mut commands: Commands, map: Res<GameMap>, picture: Option<Res<MapPicture>>, arrow: Res<Arrow>) {
    let Some(picture) = picture else {
        return;
    };
    let flags: Vec<(usize, char)> = map.0.flags.iter().enumerate().map(|(i, flag)| (i, flag.name)).collect();
    commands
        .spawn((
            Minimap,
            crate::ui::HudScaled,
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                // Under the frame rate and ping box.
                top: px(72),
                width: px(MINIMAP),
                height: px(MINIMAP),
                border: UiRect::all(px(2)),
                overflow: Overflow::clip(),
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ImageNode::new(picture.image.clone()).with_color(Color::srgba(1.0, 1.0, 1.0, 0.85)),
            Pickable::IGNORE,
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|parent| {
            for (index, name) in flags {
                parent.spawn((MinimapFlag(index), dot(16.0), BorderColor::all(NEUTRAL), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45))))
                    .with_child((label(name.to_string(), 10.0, TEXT), Pickable::IGNORE));
            }
            parent.spawn((
                MinimapSelf,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(14),
                    height: px(14),
                    ..default()
                },
                ImageNode::new(arrow.0.clone()).with_color(ACCENT),
                UiTransform::default(),
                Pickable::IGNORE,
            ));
        });
}

/// A small round marker, placed by its centre.
fn dot(size: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: px(size),
        height: px(size),
        border: UiRect::all(px(2)),
        border_radius: BorderRadius::MAX,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// The minimap key shows or hides the minimap, and remembers it.
/// `ActionInput` reads the settings too, so the two take turns.
fn toggle_minimap(mut input: ParamSet<(ActionInput, ResMut<Settings>)>) {
    if input.p0().just_pressed(Action::Minimap) {
        let mut settings = input.p1();
        settings.show_minimap = !settings.show_minimap;
        settings.save();
    }
}

#[allow(clippy::too_many_arguments)]
fn update_minimap(
    mut commands: Commands,
    settings: Res<Settings>,
    session: Res<Session>,
    map: Res<GameMap>,
    player: Res<LocalPlayer>,
    picture: Option<Res<MapPicture>>,
    mut minimap: Query<(Entity, &mut Visibility), (With<Minimap>, Without<MinimapFlag>)>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    mut flags: Query<(&MinimapFlag, &mut Node, &mut BorderColor, &mut Visibility), Without<MinimapSelf>>,
    mut me: Query<(&mut Node, &mut UiTransform), (With<MinimapSelf>, Without<MinimapFlag>)>,
    mates: Query<Entity, With<MinimapMate>>,
) {
    let (Some(picture), Ok((minimap, mut shown))) = (picture, minimap.single_mut()) else {
        return;
    };
    // Out of the way of the death screen (and, in conquest, its own map), and
    // away when the player has turned it off.
    let hidden = !settings.show_minimap || player.on_death_screen();
    shown.set_if_neq(if hidden { Visibility::Hidden } else { Visibility::Inherited });
    let conquest = session.mode == MODE_CONQUEST;
    let inner = MINIMAP - 4.0;
    let place = |at: Vec3, size: f32| {
        let p = picture.place(at, inner).clamp(Vec2::ZERO, Vec2::splat(inner));
        (px(p.x - size / 2.0), px(p.y - size / 2.0))
    };
    for (flag, mut node, mut border, mut visibility) in &mut flags {
        visibility.set_if_neq(if conquest { Visibility::Inherited } else { Visibility::Hidden });
        let Some(point) = map.0.flags.get(flag.0) else {
            continue;
        };
        let (left, top) = place(point.pos, 16.0);
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        border.set_if_neq(BorderColor::all(flag_color(&session, session.flags.get(flag.0))));
    }
    for (mut node, mut transform) in &mut me {
        let (left, top) = place(player.pos, 14.0);
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        transform.rotation = Rot2::radians(-player.yaw);
    }
    // Teammates: rebuilt every frame, there are only a few.
    for entity in &mates {
        commands.entity(entity).despawn();
    }
    if session.team_mode() {
        for (remote, transform) in &remotes {
            if !remote.alive || session.enemies(session.my_id, remote.id) {
                continue;
            }
            let (left, top) = place(transform.translation, 7.0);
            commands.spawn((
                MinimapMate,
                Node {
                    position_type: PositionType::Absolute,
                    left,
                    top,
                    width: px(7),
                    height: px(7),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(ALLY),
                Pickable::IGNORE,
                ChildOf(minimap),
            ));
        }
    }
}

/// The death screen's map, for the death screen to put beside the loadout:
/// filled in once the map's picture is drawn.
pub fn deploy_map() -> impl Bundle {
    (
        DeployMap,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6),
            display: Display::None,
            ..default()
        },
        children![
            (DeployCaption, label("", 15.0, DIM)),
            (
                DeployPicture,
                Node {
                    width: px(DEPLOY),
                    height: px(DEPLOY),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                ImageNode::default(),
            ),
        ],
    )
}

/// Shows the death screen's map in conquest, with a button on our base
/// and on every flag: lit up where we can come back in, the chosen one
/// ringed.
#[allow(clippy::too_many_arguments)]
fn update_deploy_map(
    mut commands: Commands,
    session: Res<Session>,
    map: Res<GameMap>,
    picture: Option<Res<MapPicture>>,
    choice: Res<SpawnChoice>,
    mut shell: Query<&mut Node, With<DeployMap>>,
    mut image: Query<(Entity, &mut ImageNode), With<DeployPicture>>,
    mut points: Query<(&DeployPoint, &mut BorderColor, &mut BackgroundColor)>,
    mut caption: Query<&mut Text, With<DeployCaption>>,
) {
    let (Some(picture), Ok((holder, mut image))) = (picture, image.single_mut()) else {
        return;
    };
    let shown = session.mode == MODE_CONQUEST && !map.0.flags.is_empty();
    for mut node in &mut shell {
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    if !shown {
        return;
    }
    if image.image != picture.image {
        image.image = picture.image.clone();
    }
    let mine = session.team_of(session.my_id);
    // The base sits on the middle of the team's spawns.
    let base = {
        let spawns = &map.0.bases[mine as usize & 1];
        spawns.iter().map(|s| s.pos).sum::<Vec3>() / spawns.len().max(1) as f32
    };
    let held = |point: u8| match point {
        0 => true,
        _ => session.flags.get(point as usize - 1).is_some_and(|flag| flag.owner == Some(mine)),
    };
    if points.is_empty() {
        let inner = DEPLOY - 4.0;
        let mut spots = vec![(0u8, "HQ".to_string(), base)];
        spots.extend(map.0.flags.iter().enumerate().map(|(i, flag)| (i as u8 + 1, flag.name.to_string(), flag.pos)));
        for (point, name, at) in spots {
            let size = if point == 0 { 34.0 } else { 26.0 };
            let p = picture.place(at, inner).clamp(Vec2::splat(size / 2.0), Vec2::splat(inner - size / 2.0));
            commands.spawn((
                DeployPoint(point),
                Button,
                Node {
                    left: px(p.x - size / 2.0),
                    top: px(p.y - size / 2.0),
                    ..dot(size)
                },
                BorderColor::all(NEUTRAL),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                observe(move |_: On<Activate>, mut choice: ResMut<SpawnChoice>, session: Res<Session>| {
                    let mine = session.team_of(session.my_id);
                    let ours = point == 0
                        || session.flags.get(point as usize - 1).is_some_and(|flag| flag.owner == Some(mine));
                    if ours {
                        // Clicking the chosen one again leaves it to the server.
                        choice.point = if choice.point == Some(point) { None } else { Some(point) };
                    }
                }),
                children![(label(name, 13.0, TEXT), Pickable::IGNORE)],
                ChildOf(holder),
            ));
        }
        return;
    }
    for (point, mut border, mut background) in &mut points {
        let color = if point.0 == 0 {
            ALLY
        } else {
            flag_color(&session, session.flags.get(point.0 as usize - 1))
        };
        let chosen = choice.point == Some(point.0);
        border.set_if_neq(BorderColor::all(if chosen { ACCENT } else { color }));
        let fill = if chosen {
            ACCENT.with_alpha(0.75)
        } else if held(point.0) {
            color.with_alpha(0.55)
        } else {
            Color::srgba(0.0, 0.0, 0.0, 0.5)
        };
        background.set_if_neq(BackgroundColor(fill));
    }
    let words = match choice.point {
        None => "Click your base or a flag you hold to spawn there".to_string(),
        Some(0) => "Spawn: at your base".to_string(),
        Some(point) => {
            let name = map.0.flags.get(point as usize - 1).map_or('?', |flag| flag.name);
            if held(point) {
                format!("Spawn: at {name}")
            } else {
                format!("Spawn: {name} is lost, at your base")
            }
        }
    };
    for mut text in &mut caption {
        set_text(&mut text, words.clone());
    }
}

/// Our living teammates on the death screen's map, as on the minimap:
/// rebuilt every frame, over the flags but letting clicks through to them.
fn update_deploy_mates(
    mut commands: Commands,
    session: Res<Session>,
    player: Res<LocalPlayer>,
    picture: Option<Res<MapPicture>>,
    holder: Query<Entity, With<DeployPicture>>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    mates: Query<Entity, With<DeployMate>>,
) {
    for entity in &mates {
        commands.entity(entity).despawn();
    }
    let (Some(picture), Ok(holder)) = (picture, holder.single()) else {
        return;
    };
    if session.mode != MODE_CONQUEST || !player.on_death_screen() {
        return;
    }
    let inner = DEPLOY - 4.0;
    let size = 11.0;
    for (remote, transform) in &remotes {
        if !remote.alive || session.enemies(session.my_id, remote.id) {
            continue;
        }
        let p = picture.place(transform.translation, inner).clamp(Vec2::ZERO, Vec2::splat(inner));
        commands.spawn((
            DeployMate,
            Node {
                position_type: PositionType::Absolute,
                left: px(p.x - size / 2.0),
                top: px(p.y - size / 2.0),
                width: px(size),
                height: px(size),
                border: UiRect::all(px(1.5)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(ALLY),
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            Pickable::IGNORE,
            ChildOf(holder),
        ));
    }
}

/// Tells the server where we want to come back in, when it changes.
fn send_choice(mut choice: ResMut<SpawnChoice>, connection: Option<ResMut<Connection>>) {
    let Some(mut connection) = connection else {
        return;
    };
    if choice.sent != Some(choice.point) {
        choice.sent = Some(choice.point);
        connection.send(&ClientMsg::SpawnAt { point: choice.point });
    }
}
