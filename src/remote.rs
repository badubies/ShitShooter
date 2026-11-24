//! Other players: spawned from server snapshots and smoothly interpolated.

use std::collections::{HashMap, VecDeque};

use bevy::{light::NotShadowCaster, prelude::*, window::PrimaryWindow};

use crate::{
    AppState,
    map::{CROUCH_HALF, PLAYER_HALF},
    net::{NetMsg, Session, SnapshotMsg},
    paint::{BODY_HEIGHT, BODY_RADIUS, BODY_Y, HEAD_RADIUS, HEAD_Y, NECK, PlayerModel, Skins},
    player::{GRAVITY, LocalPlayer, PlayerSystems, magazine_out},
    gun_model::{FittedModels, GunHolder, GunModelSystems, GunPart, PartKind},
    protocol::*,
    ui::ALLY,
    weapons::{SightId, WeaponId, weapon},
    world::{GameMap, MainCamera, SharedAssets},
};

/// Remote players are drawn this far in the past, so there are always two
/// snapshots to blend between (snapshots arrive every ~33 ms).
const INTERP_DELAY: f64 = 0.075;

/// How far the outline around a teammate stands out from them.
const OUTLINE: f32 = 0.035;
/// A teammate's name floats this far above the middle of their box.
const TAG_HEIGHT: f32 = 1.3;

pub struct RemotePlugin;

impl Plugin for RemotePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RemoteIndex>()
            .add_systems(OnExit(AppState::Playing), clear_index)
            .add_systems(
                Update,
                (apply_snapshots, reset_on_spawn, interpolate)
                    .chain()
                    .in_set(RemoteSystems)
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                Update,
                (
                    size_gun_boxes.after(GunModelSystems::Show),
                    settle_corpses,
                    (show_outlines, draw_ally_tags)
                        .chain()
                        .after(RemoteSystems)
                        .after(PlayerSystems::Camera),
                )
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RemoteSystems;

#[derive(Component)]
pub struct RemotePlayer {
    pub id: PlayerId,
    pub alive: bool,
    /// Their collision box is the short one (crouching or sliding).
    pub crouching: bool,
    reloading: bool,
    /// The weapon in their hands, and the sight on it.
    weapon: WeaponId,
    sight: SightId,
    pitch: f32,
    /// 0 = standing, 1 = crouched; eased so the model does not snap.
    crouch_pose: f32,
    /// 0 = gun up, 1 = gun lowered for a reload.
    reload_pose: f32,
    /// Seconds since they started reloading (0 when they are not).
    reload_for: f32,
    samples: VecDeque<Sample>,
}

struct Sample {
    time: f64,
    pos: Vec3,
    yaw: f32,
    pitch: f32,
}

/// The part of a remote player that tilts up and down with their aim: head and gun.
#[derive(Component)]
struct HeadPivot;

#[derive(Component)]
struct Body;

/// Holds the gun: its model, or `GunBox` for weapons without one.
#[derive(Component)]
struct Gun;

#[derive(Component)]
struct GunBox;

/// The rim drawn around a teammate (see `SharedAssets::ally_outline`), one
/// on the body and one on the head. Shown only when they are on our team.
#[derive(Component)]
struct AllyOutline(PlayerId);

/// A teammate's name over their head, seen through walls.
#[derive(Component)]
struct AllyTag(PlayerId);

#[derive(Resource, Default)]
struct RemoteIndex(HashMap<PlayerId, Entity>);

fn clear_index(mut index: ResMut<RemoteIndex>) {
    index.0.clear();
}

fn apply_snapshots(
    mut commands: Commands,
    mut snapshots: MessageReader<SnapshotMsg>,
    time: Res<Time<Real>>,
    session: Res<Session>,
    assets: Res<SharedAssets>,
    model: Res<PlayerModel>,
    mut skins: ResMut<Skins>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut index: ResMut<RemoteIndex>,
    mut local: ResMut<LocalPlayer>,
    mut remotes: Query<(&mut RemotePlayer, &mut Visibility, &Transform)>,
) {
    let Some(SnapshotMsg(snapshot)) = snapshots.read().last() else {
        return;
    };
    let now = time.elapsed_secs_f64();

    for snap in &snapshot.players {
        if snap.id == session.my_id {
            // The server owns our health and armor.
            if local.alive && local.health != snap.health {
                local.health = snap.health;
            }
            if local.alive && local.armor != snap.armor {
                local.armor = snap.armor;
            }
            continue;
        }
        let sample = Sample {
            time: now,
            pos: Vec3::from(snap.pos),
            yaw: snap.yaw,
            pitch: snap.pitch,
        };
        if let Some((mut remote, mut visibility, transform)) = index
            .0
            .get(&snap.id)
            .and_then(|entity| remotes.get_mut(*entity).ok())
        {
            // They just died: their body stays behind where they fell, apart
            // from them, so it stays put when they respawn.
            if remote.alive && !snap.alive
                && let Some(info) = session.roster.iter().find(|p| p.id == snap.id)
            {
                let material = skins.material(snap.id, info.color, &model, &mut images, &mut materials);
                spawn_corpse(&mut commands, &model, &assets, material, transform, &remote);
            }
            if !snap.alive {
                remote.samples.clear();
            } else {
                remote.samples.push_back(sample);
                if remote.samples.len() > 16 {
                    remote.samples.pop_front();
                }
            }
            remote.alive = snap.alive;
            remote.crouching = snap.flags & FLAG_CROUCH != 0;
            remote.reloading = snap.flags & FLAG_RELOAD != 0;
            remote.weapon = snap.weapon;
            remote.sight = snap.sight;
            let wanted = if snap.alive {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *visibility != wanted {
                *visibility = wanted;
            }
        } else if !index.0.contains_key(&snap.id)
            && let Some(info) = session.roster.iter().find(|p| p.id == snap.id)
        {
            // Their paint job (or a plain coat until it arrives).
            let material = skins.material(snap.id, info.color, &model, &mut images, &mut materials);
            // The outline sits OUTLINE outside the surface all round.
            let body_outline = Vec3::new(
                (BODY_RADIUS + OUTLINE) / BODY_RADIUS,
                (BODY_HEIGHT / 2.0 + OUTLINE) / (BODY_HEIGHT / 2.0),
                (BODY_RADIUS + OUTLINE) / BODY_RADIUS,
            );
            let outline = |mesh: &Handle<Mesh>, scale: Vec3| {
                (
                    AllyOutline(snap.id),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(assets.ally_outline.clone()),
                    Transform::from_scale(scale),
                    NotShadowCaster,
                    Visibility::Hidden,
                )
            };
            let entity = commands
                .spawn((
                    RemotePlayer {
                        id: snap.id,
                        alive: snap.alive,
                        crouching: snap.flags & FLAG_CROUCH != 0,
                        reloading: snap.flags & FLAG_RELOAD != 0,
                        weapon: snap.weapon,
                        sight: snap.sight,
                        pitch: snap.pitch,
                        crouch_pose: 0.0,
                        reload_pose: 0.0,
                        reload_for: 0.0,
                        samples: VecDeque::from([sample]),
                    },
                    Transform::from_translation(Vec3::from(snap.pos)),
                    if snap.alive {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                    DespawnOnExit(AppState::Playing),
                    children![
                        (
                            Body,
                            Mesh3d(model.body.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_xyz(0.0, BODY_Y, 0.0),
                            children![outline(&model.body, body_outline)],
                        ),
                        (
                            HeadPivot,
                            Transform::from_xyz(0.0, HEAD_Y, 0.0),
                            Visibility::default(),
                            children![
                                (
                                    Mesh3d(model.head.clone()),
                                    MeshMaterial3d(material),
                                    children![outline(
                                        &model.head,
                                        Vec3::splat((HEAD_RADIUS + OUTLINE) / HEAD_RADIUS)
                                    )],
                                ),
                                // Visor: shows which way they are facing.
                                (
                                    Mesh3d(assets.cube.clone()),
                                    MeshMaterial3d(assets.dark.clone()),
                                    Transform::from_xyz(0.0, 0.03, -0.17)
                                        .with_scale(Vec3::new(0.36, 0.11, 0.2)),
                                ),
                                (
                                    Gun,
                                    GunHolder {
                                        weapon: snap.weapon,
                                        sight: snap.sight,
                                        shadows: true,
                                    },
                                    Transform::from_translation(GUN_REST)
                                        .with_scale(Vec3::splat(REMOTE_GUN_SCALE)),
                                    Visibility::default(),
                                    children![(
                                        GunBox,
                                        Mesh3d(assets.cube.clone()),
                                        MeshMaterial3d(assets.dark.clone()),
                                        Transform::default(),
                                        Visibility::Hidden,
                                    )],
                                ),
                            ],
                        ),
                    ],
                ))
                .id();
            index.0.insert(snap.id, entity);
        }
    }

    // Players that left.
    index.0.retain(|id, entity| {
        let present = snapshot.players.iter().any(|snap| snap.id == *id);
        if !present {
            commands.entity(*entity).despawn();
        }
        present
    });
}

/// Seconds a body lies where it fell before it sinks into the floor...
const CORPSE_TIME: f32 = 8.0;
/// ...which takes this long.
const CORPSE_SINK: f32 = 1.5;
/// Seconds it takes to tip over onto the floor.
const CORPSE_FALL: f32 = 0.45;

/// A dead player's body. Only for show: it blocks nobody and no bullets.
#[derive(Component)]
pub struct Corpse {
    /// Seconds since they died.
    pub age: f32,
    /// Their collision box when they died, which drops until it lands: shot in
    /// the air, the body falls to the floor instead of lying on nothing.
    half: Vec3,
    center: Vec3,
    vel: Vec3,
    landed: bool,
}

/// The part of a body that tips over, about its feet.
#[derive(Component)]
struct CorpsePivot;

/// A copy of a remote player's body and head, standing where they died, that
/// `settle_corpses` then tips over onto its back.
fn spawn_corpse(
    commands: &mut Commands,
    model: &PlayerModel,
    assets: &SharedAssets,
    material: Handle<StandardMaterial>,
    transform: &Transform,
    remote: &RemotePlayer,
) {
    // The reported position is the middle of their box; the body stands on its floor.
    let half = if remote.crouching { CROUCH_HALF } else { PLAYER_HALF };
    let feet = transform.translation - Vec3::Y * half.y;
    commands.spawn((
        Corpse {
            age: 0.0,
            half,
            center: transform.translation,
            vel: Vec3::ZERO,
            landed: false,
        },
        Transform::from_translation(feet).with_rotation(transform.rotation),
        Visibility::default(),
        DespawnOnExit(AppState::Playing),
        children![(
            CorpsePivot,
            Transform::default(),
            Visibility::default(),
            children![
                (
                    Mesh3d(model.body.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_xyz(0.0, BODY_HEIGHT / 2.0, 0.0),
                ),
                (
                    Transform::from_xyz(0.0, BODY_HEIGHT + NECK, 0.0),
                    Visibility::default(),
                    children![
                        (Mesh3d(model.head.clone()), MeshMaterial3d(material)),
                        (
                            Mesh3d(assets.cube.clone()),
                            MeshMaterial3d(assets.dark.clone()),
                            Transform::from_xyz(0.0, 0.03, -0.17).with_scale(Vec3::new(0.36, 0.11, 0.2)),
                        ),
                    ],
                ),
            ],
        )],
    ));
}

/// Bodies tip over backwards, as if falling, then lie there a while, then
/// sink into the floor and are gone.
fn settle_corpses(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<GameMap>,
    mut corpses: Query<(Entity, &mut Corpse, &mut Transform, &Children)>,
    mut pivots: Query<&mut Transform, (With<CorpsePivot>, Without<Corpse>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut corpse, mut transform, children) in &mut corpses {
        let before = corpse.age;
        corpse.age += dt;
        if corpse.age >= CORPSE_TIME + CORPSE_SINK {
            commands.entity(entity).despawn();
            continue;
        }
        // Falls with the same gravity and collision as a player, until it
        // lands (or gives up when it starts sinking, e.g. off the map).
        if !corpse.landed && corpse.age < CORPSE_TIME {
            let Corpse { half, center, vel, landed, .. } = &mut *corpse;
            vel.y -= GRAVITY * dt;
            *landed = map.0.move_and_slide(center, vel, *half, dt);
            transform.translation.y = center.y - half.y;
        }
        if before <= CORPSE_FALL {
            // Speeds up as it goes, like a fall, and lies on its back at the end,
            // lifted by the body's thickness so it rests on the floor.
            let fall = (corpse.age / CORPSE_FALL).min(1.0).powi(2);
            for child in children.iter() {
                if let Ok(mut pivot) = pivots.get_mut(child) {
                    pivot.rotation = Quat::from_rotation_x(fall * std::f32::consts::FRAC_PI_2);
                    pivot.translation.y = BODY_RADIUS * fall;
                }
            }
        }
        let sinking = (corpse.age - CORPSE_TIME).max(0.0) - (before - CORPSE_TIME).max(0.0);
        transform.translation.y -= sinking / CORPSE_SINK * (BODY_RADIUS * 2.0 + 0.1);
    }
}

/// A respawn is a teleport: do not blend from the place they died.
fn reset_on_spawn(
    mut messages: MessageReader<NetMsg>,
    index: Res<RemoteIndex>,
    mut remotes: Query<&mut RemotePlayer>,
) {
    for NetMsg(msg) in messages.read() {
        if let ServerMsg::Spawn { id, .. } = msg
            && let Some(mut remote) = index.0.get(id).and_then(|e| remotes.get_mut(*e).ok())
        {
            remote.samples.clear();
        }
    }
}

/// Where the gun is held, relative to the head.
const GUN_REST: Vec3 = Vec3::new(0.3, -0.3, -0.45);
/// Twice the size it has in first person, so it reads at a distance.
const REMOTE_GUN_SCALE: f32 = 2.1;

type Part<T> = (With<T>, Without<RemotePlayer>);

fn interpolate(
    time: Res<Time<Real>>,
    mut remotes: Query<(&mut RemotePlayer, &mut Transform, &Children)>,
    mut pivots: Query<(&mut Transform, &Children), (Part<HeadPivot>, Without<Body>, Without<Gun>)>,
    mut bodies: Query<&mut Transform, (Part<Body>, Without<HeadPivot>, Without<Gun>)>,
    mut guns: Query<(&mut Transform, &mut GunHolder), (Part<Gun>, Without<HeadPivot>, Without<Body>)>,
    mut gun_parts: Query<
        (&GunPart, &mut Transform, &mut Visibility),
        (Part<GunPart>, Without<HeadPivot>, Without<Body>, Without<Gun>),
    >,
    // How far out each remote gun's magazine is: (gun, weapon, 0 -> 1).
    mut magazines: Local<Vec<(Entity, WeaponId, f32)>>,
) {
    magazines.clear();
    let render_time = time.elapsed_secs_f64() - INTERP_DELAY;
    let ease = (time.delta_secs() * 12.0).min(1.0);
    let crouch_drop = PLAYER_HALF.y - CROUCH_HALF.y;
    // (body height, head height) when fully crouched: both shrink by twice the
    // drop of the box centre, because the feet stay where they are.
    let crouched_body = (BODY_HEIGHT - 2.0 * crouch_drop) / BODY_HEIGHT;
    for (mut remote, mut transform, children) in &mut remotes {
        let samples = &remote.samples;
        let Some(newest) = samples.back() else {
            continue;
        };
        // Find the pair of samples around the render time.
        let after = samples.iter().position(|s| s.time >= render_time);
        let (pos, yaw, pitch) = match after {
            Some(0) => (samples[0].pos, samples[0].yaw, samples[0].pitch),
            Some(i) => {
                let (a, b) = (&samples[i - 1], &samples[i]);
                let t = ((render_time - a.time) / (b.time - a.time).max(1e-6)) as f32;
                (
                    a.pos.lerp(b.pos, t),
                    lerp_angle(a.yaw, b.yaw, t),
                    a.pitch + (b.pitch - a.pitch) * t,
                )
            }
            // Everything is older than the render time: hold the newest sample.
            None => (newest.pos, newest.yaw, newest.pitch),
        };
        transform.translation = pos;
        transform.rotation = Quat::from_rotation_y(yaw);
        remote.pitch = pitch;
        let (crouch, reload) = (remote.crouching as u8 as f32, remote.reloading as u8 as f32);
        remote.crouch_pose += (crouch - remote.crouch_pose) * ease;
        remote.reload_pose += (reload - remote.reload_pose) * ease;
        remote.reload_for = if remote.reloading { remote.reload_for + time.delta_secs() } else { 0.0 };
        // Their magazine comes out and goes back in, as ours does (we are only
        // told that they are reloading, so it is timed from when that started).
        let stats = weapon(remote.weapon);
        let progress = if stats.reload_per_round { 0.0 } else { remote.reload_for / stats.reload.max(0.01) };
        let magazine_out = magazine_out(progress);

        // The reported position is the centre of their collision box, which is
        // the short one while they crouch. Pose the model around it.
        let pose = remote.crouch_pose;
        // How far the model's feet sit below the reported centre.
        let feet = -(PLAYER_HALF.y - crouch_drop * remote.crouching as u8 as f32);
        let body_scale = 1.0 + (crouched_body - 1.0) * pose;
        let body_height = BODY_HEIGHT * body_scale;
        for child in children.iter() {
            if let Ok(mut body) = bodies.get_mut(child) {
                body.translation.y = feet + body_height / 2.0;
                body.scale.y = body_scale;
            } else if let Ok((mut pivot, parts)) = pivots.get_mut(child) {
                pivot.translation.y = feet + body_height + NECK;
                pivot.rotation = Quat::from_rotation_x(remote.pitch);
                for part in parts.iter() {
                    if let Ok((mut gun, mut holder)) = guns.get_mut(part) {
                        magazines.push((part, remote.weapon, magazine_out));
                        holder.weapon = remote.weapon;
                        holder.sight = remote.sight;
                        gun.translation = GUN_REST + Vec3::new(0.0, -0.12, 0.1) * remote.reload_pose;
                        gun.rotation = Quat::from_rotation_x(-0.9 * remote.reload_pose);
                    }
                }
            }
        }
    }
    for (part, mut transform, mut visibility) in &mut gun_parts {
        if part.kind != PartKind::Magazine {
            continue;
        }
        let Some(&(_, _, out)) = magazines.iter().find(|(gun, weapon, _)| *gun == part.holder && *weapon == part.weapon)
        else {
            continue;
        };
        part.pose_magazine(&mut transform, &mut visibility, out);
    }
}

/// Teammates get an outline; nobody else does.
fn show_outlines(session: Res<Session>, mut outlines: Query<(&AllyOutline, &mut Visibility)>) {
    for (outline, mut visibility) in &mut outlines {
        let ally = session.team_mode() && !session.enemies(session.my_id, outline.0);
        visibility.set_if_neq(if ally {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Puts every living teammate's name over their head, through walls.
fn draw_ally_tags(
    mut commands: Commands,
    session: Res<Session>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Transform, &Projection), With<MainCamera>>,
    remotes: Query<(&RemotePlayer, &Transform), Without<MainCamera>>,
    mut tags: Query<(Entity, &AllyTag, &mut Node, &mut Visibility)>,
) {
    const WIDTH: f32 = 200.0;
    let (camera, projection) = camera.into_inner();
    let fov = match projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => 1.0,
    };
    let size = crate::ui::ui_size(&window, &ui_scale);

    let mut shown = Vec::new();
    for (remote, transform) in &remotes {
        if !remote.alive || !session.team_mode() || session.enemies(session.my_id, remote.id) {
            continue;
        }
        let above = transform.translation + Vec3::Y * TAG_HEIGHT;
        let Some(at) = crate::ping::on_screen(camera, fov, size, above) else {
            continue;
        };
        shown.push(remote.id);
        let (left, top) = (px(at.x - WIDTH / 2.0), px(at.y - 20.0));
        if let Some((_, _, mut node, mut visibility)) = tags.iter_mut().find(|(_, tag, ..)| tag.0 == remote.id) {
            if node.left != left || node.top != top {
                node.left = left;
                node.top = top;
            }
            visibility.set_if_neq(Visibility::Inherited);
            continue;
        }
        let name = session.name_of(remote.id).trim_start_matches("[bot] ").to_string();
        commands.spawn((
            AllyTag(remote.id),
            Node {
                position_type: PositionType::Absolute,
                left,
                top,
                width: px(WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(3),
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(AppState::Playing),
            children![
                (
                    crate::ui::label(name, 14.0, ALLY),
                    TextLayout::justify(Justify::Center),
                    TextShadow {
                        offset: Vec2::splat(1.5),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.8),
                    },
                ),
                // A small diamond just over their head.
                (
                    Node {
                        width: px(7),
                        height: px(7),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                    BackgroundColor(ALLY.with_alpha(0.8)),
                ),
            ],
        ));
    }
    for (entity, tag, _, mut visibility) in &mut tags {
        if shown.contains(&tag.0) {
            continue;
        }
        if session.roster.iter().any(|p| p.id == tag.0) {
            visibility.set_if_neq(Visibility::Hidden);
        } else {
            commands.entity(entity).despawn();
        }
    }
}

/// Weapons without a model are a single box the size of the gun.
fn size_gun_boxes(
    fitted_models: Res<FittedModels>,
    guns: Query<(&GunHolder, &Children), With<Gun>>,
    mut boxes: Query<(&mut Transform, &mut Visibility), With<GunBox>>,
) {
    for (holder, children) in &guns {
        let modelled = fitted_models.get(holder.weapon).is_some();
        for child in children.iter() {
            if let Ok((mut transform, mut visibility)) = boxes.get_mut(child) {
                transform.scale = Vec3::from(weapon(holder.weapon).size);
                visibility.set_if_neq(if modelled {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                });
            }
        }
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let diff = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + diff * t
}
