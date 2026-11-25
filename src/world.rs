//! The static scene: map geometry, lighting, the camera and shared assets.

use bevy::{
    light::{CascadeShadowConfigBuilder, NotShadowCaster},
    prelude::*,
    render::render_resource::Face,
};

use crate::{
    grenade::{HeldGrenade, HeldGrenadeModel, held_grenade_transform},
    map::Map,
    net::Session,
    protocol::{GRENADE_KINDS, HILL_CONTESTED, HILL_EMPTY, HILL_HELD, HILL_RADIUS, MODE_HILL},
    ui::{BLUE, RED},
};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GameMap(Map::arena()))
            .insert_resource(ClearColor(Color::srgb(0.03, 0.04, 0.07)))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(0.75, 0.82, 1.0),
                brightness: 350.0,
                ..default()
            })
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (build_map.run_if(resource_changed::<GameMap>), (show_hill, show_armor)).chain(),
            );
    }
}

/// The map being shown and played. Replacing it rebuilds the scenery.
#[derive(Resource)]
pub struct GameMap(pub Map);

/// A block or jump pad of the current map.
#[derive(Component)]
struct MapGeometry;

/// The one light that casts shadows.
#[derive(Component)]
pub struct Sun;

/// The one camera. It orbits the arena in menus and is the player's eyes in a match.
#[derive(Component)]
pub struct MainCamera;

/// The gun in the corner of the screen.
#[derive(Component)]
pub struct ViewModel;

/// The boxes the weapon in hand is made of, for weapons without a model.
/// `player.rs` sizes and places them for whichever weapon it is.
#[derive(Component)]
pub enum ViewModelPart {
    /// Receiver and barrel, or the knife's blade.
    Body,
    Grip,
    Sight,
    /// Drops out during a reload.
    Magazine,
}

/// The plate floating over `map.armor_spots[index]`, shown while it is there to take.
#[derive(Component)]
struct ArmorPickup(usize);

/// The glowing column that marks the hill in king of the hill.
#[derive(Component)]
struct HillZone;

/// Meshes and materials that are reused all over the place.
#[derive(Resource)]
pub struct SharedAssets {
    pub cube: Handle<Mesh>,
    /// A sphere 1 m across; scale it to size.
    pub sphere: Handle<Mesh>,
    pub tracer: Handle<StandardMaterial>,
    pub enemy_tracer: Handle<StandardMaterial>,
    pub explosion: Handle<StandardMaterial>,
    /// The white-hot flash of a flashbang.
    pub flash: Handle<StandardMaterial>,
    /// The model of each kind of grenade, indexed by `GrenadeKind`.
    pub grenades: [Handle<WorldAsset>; GRENADE_KINDS],
    pad: Handle<StandardMaterial>,
    armor: Handle<StandardMaterial>,
    /// The faint mark under an armor pickup, there even while it is taken.
    armor_base: Handle<StandardMaterial>,
    hill_mesh: Handle<Mesh>,
    /// Empty, held by our team, held by the enemy, contested.
    hill_materials: [Handle<StandardMaterial>; 4],
    pub dark: Handle<StandardMaterial>,
    /// The outline around teammates: drawn on the inside of a copy of the
    /// model a little larger than it, so only a rim shows around the edge.
    pub ally_outline: Handle<StandardMaterial>,
}

/// The grenade models, indexed by `GrenadeKind`. Made by `art/grenades.py`.
pub const GRENADE_MODELS: [&str; GRENADE_KINDS] = [
    "models/grenades/frag.glb",
    "models/grenades/flash.glb",
    "models/grenades/smoke.glb",
];

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let sphere = meshes.add(Sphere::new(0.5).mesh().ico(4).expect("valid sphere"));
    let mut glowing = |color: LinearRgba| {
        materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: color,
            unlit: false,
            ..default()
        })
    };
    let tracer = glowing(LinearRgba::rgb(12.0, 9.0, 2.0));
    let enemy_tracer = glowing(LinearRgba::rgb(12.0, 2.5, 1.5));
    let explosion = glowing(LinearRgba::rgb(30.0, 13.0, 3.0));
    let pad = glowing(LinearRgba::rgb(0.4, 5.0, 3.0));
    let armor = glowing(LinearRgba::rgb(0.8, 2.6, 9.0));
    let armor_base = glowing(LinearRgba::rgb(0.05, 0.25, 0.9));
    let flash = glowing(LinearRgba::rgb(40.0, 40.0, 36.0));
    let grenades = GRENADE_MODELS.map(|path| asset_server.load(GltfAssetLabel::Scene(0).from_asset(path)));
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.06),
        perceptual_roughness: 0.4,
        ..default()
    });
    let gun_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.17, 0.2),
        perceptual_roughness: 0.5,
        ..default()
    });
    let ally_outline = materials.add(StandardMaterial {
        base_color: BLUE,
        unlit: true,
        cull_mode: Some(Face::Front),
        ..default()
    });

    // Where it shines from and how brightly is up to the map (`scenery.rs`).
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        // Shadows reach far enough for the outdoor maps.
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            first_cascade_far_bound: 14.0,
            maximum_distance: 220.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(30.0, 60.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((
        MainCamera,
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            fov: crate::player::BASE_FOV,
            near: 0.01,
            ..default()
        }),
        Transform::from_xyz(0.0, 30.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        children![(
            ViewModel,
            crate::gun_model::GunHolder {
                weapon: crate::weapons::RIFLE,
                sight: crate::weapons::IRON_SIGHTS,
                shadows: false,
            },
            Transform::from_translation(crate::player::hip_position(
                crate::weapons::weapon(crate::weapons::RIFLE).size[2],
            )),
            Visibility::Hidden,
            children![
                gun_part(ViewModelPart::Body, &cube, &gun_material),
                gun_part(ViewModelPart::Grip, &cube, &dark),
                gun_part(ViewModelPart::Sight, &cube, &dark),
                gun_part(ViewModelPart::Magazine, &cube, &dark),
            ],
        ), (
            // The grenade in the other hand: one model of each kind, the one
            // in the loadout shown.
            HeldGrenade,
            held_grenade_transform(0.0),
            Visibility::Hidden,
            Children::spawn(SpawnIter(grenades.clone().into_iter().enumerate().map(|(kind, scene)| {
                (HeldGrenadeModel(kind as u8), WorldAssetRoot(scene), Visibility::Hidden)
            }))),
        )],
    ));

    let hill_materials = [
        Color::srgba(0.9, 0.9, 0.9, 0.04),
        BLUE.with_alpha(0.11),
        RED.with_alpha(0.11),
        Color::srgba(1.0, 0.8, 0.1, 0.11),
    ]
    .map(|color| {
        materials.add(StandardMaterial {
            base_color: color,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            // Visible from the inside too.
            cull_mode: None,
            double_sided: true,
            ..default()
        })
    });

    commands.insert_resource(SharedAssets {
        hill_mesh: meshes.add(Cylinder::new(HILL_RADIUS, HILL_HEIGHT)),
        hill_materials,
        sphere: sphere.clone(),
        cube,
        tracer,
        enemy_tracer,
        explosion,
        flash,
        grenades: grenades.clone(),
        pad,
        armor,
        armor_base,
        dark,
        ally_outline,
    });
}

const HILL_HEIGHT: f32 = 12.0;

fn gun_part(part: ViewModelPart, cube: &Handle<Mesh>, material: &Handle<StandardMaterial>) -> impl Bundle {
    (
        part,
        Mesh3d(cube.clone()),
        MeshMaterial3d(material.clone()),
        NotShadowCaster,
    )
}

/// The hill's column is shown in king of the hill, in the colour of whoever
/// holds it: us or the enemy.
fn show_hill(
    session: Res<Session>,
    assets: Res<SharedAssets>,
    mut zone: Query<(&mut Visibility, &mut MeshMaterial3d<StandardMaterial>), With<HillZone>>,
) {
    for (mut visibility, mut material) in &mut zone {
        let wanted = if session.mode == MODE_HILL {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let shade = match session.hill {
            HILL_EMPTY => 0,
            HILL_CONTESTED => 3,
            held => 1 + ((held - HILL_HELD) ^ session.team_of(session.my_id)) as usize & 1,
        };
        let holder = &assets.hill_materials[shade];
        if material.0 != *holder {
            material.0 = holder.clone();
        }
    }
}

/// Armor pickups spin and bob while they are there, and vanish while taken.
fn show_armor(
    time: Res<Time>,
    session: Res<Session>,
    map: Res<GameMap>,
    mut pickups: Query<(&ArmorPickup, &mut Transform, &mut Visibility)>,
) {
    let t = time.elapsed_secs();
    for (pickup, mut transform, mut visibility) in &mut pickups {
        let taken = session.armor_taken & (1 << (pickup.0 % 32)) != 0;
        let wanted = if taken { Visibility::Hidden } else { Visibility::Inherited };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let Some(spot) = map.0.armor_spots.get(pickup.0) else {
            continue;
        };
        // Each one a little out of step with the others.
        let phase = t * 2.0 + pickup.0 as f32;
        transform.translation = *spot + Vec3::Y * (0.75 + 0.12 * phase.sin());
        transform.rotation = Quat::from_rotation_y(t * 1.8 + pickup.0 as f32);
    }
}

/// Replaces the scenery whenever the map changes (and builds it the first time).
fn build_map(
    mut commands: Commands,
    map: Res<GameMap>,
    assets: Res<SharedAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, With<MapGeometry>>,
) {
    for entity in &old {
        commands.entity(entity).despawn();
    }

    // One material per distinct colour.
    let mut block_materials: Vec<([f32; 3], Handle<StandardMaterial>)> = Vec::new();
    for block in map.0.blocks.iter().filter(|b| b.visible) {
        let material = match block_materials.iter().find(|(c, _)| *c == block.color) {
            Some((_, handle)) => handle.clone(),
            None => {
                let [r, g, b] = block.color;
                let handle = materials.add(StandardMaterial {
                    base_color: Color::srgb(r, g, b),
                    perceptual_roughness: 0.9,
                    ..default()
                });
                block_materials.push((block.color, handle.clone()));
                handle
            }
        };
        commands.spawn((
            MapGeometry,
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(material),
            Transform::from_translation((block.min + block.max) / 2.0)
                .with_scale(block.max - block.min),
        ));
    }

    for pad in &map.0.pads {
        commands.spawn((
            MapGeometry,
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.pad.clone()),
            Transform::from_translation(pad.center + Vec3::Y * 0.03).with_scale(Vec3::new(
                pad.half_size * 2.0,
                0.06,
                pad.half_size * 2.0,
            )),
            NotShadowCaster,
        ));

        // Arrow pads get two dark chevrons pointing where they throw you.
        let Some(dir) = pad.direction() else {
            continue;
        };
        const ARM: f32 = 0.7;
        for tip in [0.5, -0.1] {
            let tip = pad.center + dir * tip * pad.half_size + Vec3::Y * 0.07;
            for angle in [std::f32::consts::FRAC_PI_4, -std::f32::consts::FRAC_PI_4] {
                let arm = Quat::from_rotation_y(angle) * dir;
                commands.spawn((
                    MapGeometry,
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.dark.clone()),
                    Transform::from_translation(tip - arm * ARM / 2.0)
                        .looking_to(arm, Vec3::Y)
                        .with_scale(Vec3::new(0.18, 0.02, ARM)),
                    NotShadowCaster,
                ));
            }
        }
    }

    for (index, spot) in map.0.armor_spots.iter().enumerate() {
        commands.spawn((
            MapGeometry,
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(assets.armor_base.clone()),
            Transform::from_translation(*spot + Vec3::Y * 0.02).with_scale(Vec3::new(1.1, 0.04, 1.1)),
            NotShadowCaster,
        ));
        // A chest plate: a thick slab with a raised strip down the middle.
        commands.spawn((
            MapGeometry,
            ArmorPickup(index),
            Transform::from_translation(*spot + Vec3::Y * 0.75),
            Visibility::Inherited,
            children![
                (
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.armor.clone()),
                    Transform::from_scale(Vec3::new(0.6, 0.7, 0.14)),
                    NotShadowCaster,
                ),
                (
                    Mesh3d(assets.cube.clone()),
                    MeshMaterial3d(assets.dark.clone()),
                    Transform::from_scale(Vec3::new(0.12, 0.72, 0.18)),
                    NotShadowCaster,
                ),
            ],
        ));
    }

    commands.spawn((
        MapGeometry,
        HillZone,
        Mesh3d(assets.hill_mesh.clone()),
        MeshMaterial3d(assets.hill_materials[0].clone()),
        Transform::from_translation(map.0.hill + Vec3::Y * HILL_HEIGHT / 2.0),
        NotShadowCaster,
        Visibility::Hidden,
    ));
}
