//! Grenades as the client sees them. The server owns the real ones (and the
//! damage); it tells us when one is thrown and when it goes off. In between we
//! run the same bounce simulation, so the grenade on screen is where the
//! server's is.
//!
//! A frag explodes. A flashbang blinds whoever sees it go off: we work out our
//! own blindness here, the server does it for the bots. A smoke grenade leaves
//! a cloud that hides everything behind it for a while.

use std::f32::consts::TAU;

use bevy::{light::NotShadowCaster, prelude::*};

use crate::{
    AppState,
    audio::{Sound, SoundCue, surface_under},
    map::{self, BOUNCE_SOUND, GRENADE_HALF},
    net::NetMsg,
    player::LocalPlayer,
    protocol::*,
    world::{GameMap, SharedAssets},
};

/// Seconds the fireball lasts.
const BLAST_TIME: f32 = 0.45;
/// Widest the fireball gets, in metres across.
const BLAST_SIZE: f32 = 5.5;
const BLAST_LIGHT: f32 = 6_000_000.0;
/// A flashbang's flash is shorter, smaller and much brighter.
const FLASH_TIME: f32 = 0.3;
const FLASH_SIZE: f32 = 1.6;
const FLASH_LIGHT: f32 = 25_000_000.0;
/// Seconds a smoke cloud takes to billow out, and to thin away at the end.
const SMOKE_GROW: f32 = 1.2;
const SMOKE_FADE: f32 = 2.5;
/// How opaque each puff of smoke is. Two layers are as good as solid.
const SMOKE_ALPHA: f32 = 0.93;
/// Flashed for less than this, the ears do not ring.
const RINGING_BLIND: f32 = 1.0;
/// Thrown grenade models are drawn this much bigger than life, to be seen
/// across the arena.
const THROWN_SCALE: f32 = 1.6;
/// How far below the middle of its bounce box a thrown grenade's model sits,
/// so it lies on the floor rather than hovering over it.
const THROWN_DROP: f32 = GRENADE_HALF - 0.05;
/// Slower than this, in metres a second, a grenade has come to rest and rolls
/// over onto its side.
const RESTING_SPEED: f32 = 0.5;
/// Slower than this along the ground when it stops bouncing, a grenade is
/// not heard rolling.
const ROLL_SOUND: f32 = 1.0;

pub struct GrenadePlugin;

impl Plugin for GrenadePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SmokeFog>()
            .add_systems(OnExit(AppState::Playing), |mut fog: ResMut<SmokeFog>| fog.0 = 0.0)
            .add_systems(
                Update,
                (
                    (handle_net, simulate, update_blasts, update_smoke)
                        .chain()
                        .run_if(in_state(AppState::Playing)),
                    show_held,
                    held_without_shadows,
                ),
            );
    }
}

/// How deep in smoke our eyes are: 0 = clear air, 1 = can't see a thing.
/// Clouds are drawn from the outside only, so from the inside this fogs the
/// whole screen instead.
#[derive(Resource, Default)]
pub struct SmokeFog(pub f32);

/// The grenade in our hand. A child of the camera.
#[derive(Component)]
pub struct HeldGrenade;

/// One kind's model under `HeldGrenade`; the kind in hand is the one shown.
#[derive(Component)]
pub struct HeldGrenadeModel(pub GrenadeKind);

/// Where the held grenade is: `hold` 0 is low out of sight, 1 is up in view.
pub fn held_grenade_transform(hold: f32) -> Transform {
    Transform::from_translation(Vec3::new(-0.24, -0.42, -0.5).lerp(Vec3::new(-0.2, -0.2, -0.5), hold))
        .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.5, 0.15, 0.2))
        .with_scale(Vec3::splat(0.8))
}

/// A thrown grenade's model, a child of its `Grenade`, which turns it as it flies.
#[derive(Component)]
struct ThrownModel;

#[derive(Component)]
struct Grenade {
    id: u32,
    pos: Vec3,
    /// Where it was one simulation step ago, to draw it smoothly in between.
    previous: Vec3,
    vel: Vec3,
    /// Time not yet consumed by whole simulation steps.
    clock: f32,
    age: f32,
    fuse: f32,
    /// Its roll along the ground since it last bounced has been heard.
    rolled: bool,
}

/// A fireball or flash: a glowing sphere that swells and shrinks away, or the
/// light that goes with it.
#[derive(Component)]
struct Blast {
    age: f32,
    life: f32,
    /// Widest the sphere gets, in metres across.
    size: f32,
    /// Brightest the light gets.
    light: f32,
}

/// A smoke cloud: a handful of overlapping puffs sharing one material, which
/// thins out at the end.
#[derive(Component)]
struct SmokeCloud {
    age: f32,
    material: Handle<StandardMaterial>,
}

fn handle_net(
    mut commands: Commands,
    mut messages: MessageReader<NetMsg>,
    assets: Res<SharedAssets>,
    map: Res<GameMap>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut player: ResMut<LocalPlayer>,
    grenades: Query<(Entity, &Grenade)>,
    mut sounds: MessageWriter<SoundCue>,
) {
    for NetMsg(msg) in messages.read() {
        match msg {
            ServerMsg::Grenade { id, kind, pos, vel, fuse } => {
                let pos = Vec3::from(*pos);
                let vel = Vec3::from(*vel);
                // Leaves the hand fuse up, leaning the way it was thrown.
                let lean = Quat::from_rotation_arc(Vec3::Y, (vel.normalize_or(Vec3::NEG_Z) + Vec3::Y * 2.0).normalize());
                commands.spawn((
                    Grenade {
                        id: *id,
                        pos,
                        previous: pos,
                        vel,
                        clock: 0.0,
                        age: 0.0,
                        fuse: *fuse,
                        rolled: false,
                    },
                    Transform::from_translation(pos),
                    Visibility::default(),
                    DespawnOnExit(AppState::Playing),
                    children![(
                        ThrownModel,
                        WorldAssetRoot(assets.grenades[*kind as usize % GRENADE_KINDS].clone()),
                        Transform::from_xyz(0.0, -THROWN_DROP, 0.0)
                            .with_rotation(lean)
                            .with_scale(Vec3::splat(THROWN_SCALE)),
                    )],
                ));
            }
            ServerMsg::Explosion { id, kind, pos } => {
                for (entity, grenade) in &grenades {
                    if grenade.id == *id {
                        commands.entity(entity).despawn();
                    }
                }
                let pos = Vec3::from(*pos);
                match *kind {
                    FLASH => {
                        sounds.write(SoundCue {
                            sound: Sound::Flashbang,
                            at: Some(pos),
                        });
                        spawn_blast(&mut commands, &assets, &assets.flash, pos, FLASH_TIME, FLASH_SIZE, FLASH_LIGHT, Color::WHITE);
                        if player.alive {
                            let look = map::look_dir(player.yaw, player.pitch);
                            let blind = map.0.flash_blindness(pos, player.eye(), look);
                            if blind >= RINGING_BLIND && player.blind < RINGING_BLIND {
                                sounds.write(SoundCue {
                                    sound: Sound::Ringing,
                                    at: None,
                                });
                            }
                            player.blind = player.blind.max(blind);
                        }
                    }
                    SMOKE => {
                        sounds.write(SoundCue {
                            sound: Sound::SmokePop,
                            at: Some(pos),
                        });
                        spawn_smoke(&mut commands, &assets, &mut materials, &map, pos);
                    }
                    _ => {
                        sounds.write(SoundCue {
                            sound: Sound::Explosion,
                            at: Some(pos),
                        });
                        let color = Color::srgb(1.0, 0.6, 0.25);
                        spawn_blast(&mut commands, &assets, &assets.explosion, pos, BLAST_TIME, BLAST_SIZE, BLAST_LIGHT, color);
                    }
                }
            }
            _ => {}
        }
    }
}

fn spawn_blast(
    commands: &mut Commands,
    assets: &SharedAssets,
    material: &Handle<StandardMaterial>,
    pos: Vec3,
    life: f32,
    size: f32,
    light: f32,
    color: Color,
) {
    let blast = || Blast {
        age: 0.0,
        life,
        size,
        light,
    };
    commands.spawn((
        blast(),
        Mesh3d(assets.sphere.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(pos).with_scale(Vec3::splat(0.01)),
        NotShadowCaster,
        DespawnOnExit(AppState::Playing),
    ));
    // The light. Not a child of the sphere, whose scale would shrink it.
    commands.spawn((
        blast(),
        PointLight {
            color,
            intensity: light,
            range: 40.0,
            ..default()
        },
        Transform::from_translation(pos + Vec3::Y * 0.8),
        DespawnOnExit(AppState::Playing),
    ));
}

/// The puffs a cloud is made of: offset from its middle and radius, both as
/// parts of `SMOKE_RADIUS`. A big one in the middle, a ring around it at
/// head height, and a few on top.
fn puffs() -> impl Iterator<Item = (Vec3, f32)> {
    let ring = (0..6).map(|i| {
        let angle = i as f32 * TAU / 6.0;
        (Vec3::new(angle.cos() * 0.5, -0.05, angle.sin() * 0.5), 0.55)
    });
    let top = (0..3).map(|i| {
        let angle = (i as f32 + 0.5) * TAU / 3.0;
        (Vec3::new(angle.cos() * 0.35, 0.4, angle.sin() * 0.35), 0.45)
    });
    std::iter::once((Vec3::ZERO, 0.75)).chain(ring).chain(top)
}

fn spawn_smoke(
    commands: &mut Commands,
    assets: &SharedAssets,
    materials: &mut Assets<StandardMaterial>,
    map: &GameMap,
    pos: Vec3,
) {
    let center = map::smoke_center(pos);
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.62, 0.62, SMOKE_ALPHA),
        emissive: LinearRgba::rgb(0.06, 0.06, 0.06),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    commands
        .spawn((
            SmokeCloud {
                age: 0.0,
                material: material.clone(),
            },
            Transform::from_translation(center).with_scale(Vec3::splat(0.2)),
            Visibility::default(),
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|cloud| {
            for (offset, radius) in puffs() {
                let radius = radius * SMOKE_RADIUS;
                let mut offset = offset * SMOKE_RADIUS;
                // Pulled in from walls, so the smoke does not show through them.
                let distance = offset.length();
                if distance > 0.0
                    && let Some(wall) = map.0.raycast(center, offset / distance, distance + radius * 0.5)
                {
                    offset *= (wall - radius * 0.5).max(0.0) / distance;
                }
                cloud.spawn((
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(offset).with_scale(Vec3::splat(radius * 2.0)),
                    NotShadowCaster,
                ));
            }
        });
}

fn simulate(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<GameMap>,
    mut grenades: Query<(Entity, &mut Grenade, &mut Transform, &Children), Without<ThrownModel>>,
    mut models: Query<&mut Transform, With<ThrownModel>>,
    mut sounds: MessageWriter<SoundCue>,
) {
    let dt = time.delta_secs().min(0.25);
    for (entity, mut grenade, mut transform, children) in &mut grenades {
        let grenade = &mut *grenade;
        grenade.clock += dt;
        while grenade.clock >= GRENADE_STEP {
            grenade.clock -= GRENADE_STEP;
            grenade.age += GRENADE_STEP;
            grenade.previous = grenade.pos;
            let hit = map.0.step_grenade(&mut grenade.pos, &mut grenade.vel, GRENADE_STEP);
            let below = grenade.pos - Vec3::Y * GRENADE_HALF;
            if hit > BOUNCE_SOUND {
                sounds.write(SoundCue {
                    sound: Sound::Bounce(surface_under(&map.0, below), hit),
                    at: Some(grenade.pos),
                });
                grenade.rolled = false;
            } else if hit > 0.0 && grenade.vel.y.abs() < 0.5 && !grenade.rolled {
                // Down for good, but still moving along: it rolls and skids.
                let speed = Vec2::new(grenade.vel.x, grenade.vel.z).length();
                if speed > ROLL_SOUND {
                    sounds.write(SoundCue {
                        sound: Sound::Roll(surface_under(&map.0, below), speed),
                        at: Some(grenade.pos),
                    });
                }
                grenade.rolled = true;
            }
        }
        transform.translation = grenade.previous.lerp(grenade.pos, grenade.clock / GRENADE_STEP);
        for &child in children {
            if let Ok(mut model) = models.get_mut(child) {
                tumble(&mut model, grenade.vel, dt);
            }
        }
        // The server's explosion normally removes it. This only tidies up one
        // whose explosion message never came.
        if grenade.age > grenade.fuse + 2.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// A flying grenade tumbles end over end about the axis across its path. Once
/// it has stopped it rolls over onto its side, fuse along the floor.
fn tumble(model: &mut Transform, vel: Vec3, dt: f32) {
    let speed = vel.length();
    if speed > RESTING_SPEED {
        let axis = vel.cross(Vec3::Y).normalize_or_zero();
        if axis != Vec3::ZERO {
            // About two turns a second at throwing speed.
            let spin = -speed * 0.6;
            model.rotation = (Quat::from_axis_angle(axis, spin * dt) * model.rotation).normalize();
        }
        // Rolling along the floor: the fuse stays across the way it rolls.
        return;
    }
    let up = model.rotation * Vec3::Y;
    let flat = Vec3::new(up.x, 0.0, up.z).normalize_or(Vec3::X);
    let lying = Quat::from_rotation_arc(up, flat) * model.rotation;
    model.rotation = model.rotation.slerp(lying, 1.0 - (-10.0 * dt).exp());
}

/// The sphere swells fast and then shrinks away; the light fades with it.
fn update_blasts(
    mut commands: Commands,
    time: Res<Time>,
    mut blasts: Query<(Entity, &mut Blast, &mut Transform, Option<&mut PointLight>)>,
) {
    for (entity, mut blast, mut transform, light) in &mut blasts {
        blast.age += time.delta_secs();
        let t = blast.age / blast.life;
        if t >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(mut light) = light {
            light.intensity = blast.light * (1.0 - t) * (1.0 - t);
        } else {
            let swell = (t / 0.3).min(1.0);
            let size = if t < 0.3 {
                1.0 - (1.0 - swell).powi(3)
            } else {
                1.0 - ((t - 0.3) / 0.7).powi(2)
            };
            transform.scale = Vec3::splat((blast.size * size).max(0.01));
        }
    }
}

/// Clouds billow out, hang there, then thin away. Works out how deep in
/// smoke our eyes are.
fn update_smoke(
    mut commands: Commands,
    time: Res<Time>,
    player: Res<LocalPlayer>,
    mut fog: ResMut<SmokeFog>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut clouds: Query<(Entity, &mut SmokeCloud, &mut Transform)>,
) {
    let eye = player.eye();
    fog.0 = 0.0;
    for (entity, mut cloud, mut transform) in &mut clouds {
        cloud.age += time.delta_secs();
        if cloud.age >= SMOKE_TIME {
            commands.entity(entity).despawn();
            continue;
        }
        let grow = (cloud.age / SMOKE_GROW).min(1.0);
        let scale = 0.2 + 0.8 * (1.0 - (1.0 - grow).powi(3));
        transform.scale = Vec3::splat(scale);
        let left = SMOKE_TIME - cloud.age;
        let thickness = (left / SMOKE_FADE).min(1.0);
        if thickness < 1.0
            && let Some(mut material) = materials.get_mut(&cloud.material)
        {
            material.base_color.set_alpha(SMOKE_ALPHA * thickness);
        }
        // Thickening over the outer metre or so.
        let radius = SMOKE_RADIUS * scale;
        let depth = (radius - eye.distance(transform.translation)) / (radius * 0.25);
        fog.0 = fog.0.max(depth.clamp(0.0, 1.0) * thickness);
    }
}

/// The grenade comes up into view in the other hand.
fn show_held(
    state: Res<State<AppState>>,
    player: Res<LocalPlayer>,
    held: Single<(&mut Transform, &mut Visibility), With<HeldGrenade>>,
    mut models: Query<(&HeldGrenadeModel, &mut Visibility), Without<HeldGrenade>>,
) {
    let (mut transform, mut visibility) = held.into_inner();
    let hand = player.hand.filter(|_| player.alive && *state.get() == AppState::Playing);
    let wanted = if hand.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *visibility != wanted {
        *visibility = wanted;
    }
    let Some(hand) = hand else {
        return;
    };
    for (model, mut visibility) in &mut models {
        let wanted = if model.0 == hand.kind {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    // A cooking frag trembles as the fuse runs down.
    let shake = match hand.fuse {
        Some(fuse) if fuse < 0.8 => (fuse * 90.0).sin() * 0.004,
        _ => 0.0,
    };
    *transform = held_grenade_transform(player.hold);
    transform.translation.x += shake;
}

/// The held grenade's shadow would float in front of us: its meshes cast none.
fn held_without_shadows(
    mut commands: Commands,
    meshes: Query<Entity, (Added<Mesh3d>, Without<NotShadowCaster>)>,
    parents: Query<&ChildOf>,
    held: Query<(), With<HeldGrenade>>,
) {
    for mesh in &meshes {
        if parents.iter_ancestors(mesh).any(|ancestor| held.contains(ancestor)) {
            commands.entity(mesh).insert(NotShadowCaster);
        }
    }
}
