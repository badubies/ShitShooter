//! The outdoor maps' scenery: the ground, roads, water and every model
//! standing on the map, plus each map's sky, sun and fog. Rebuilt whenever
//! the map changes, like the blocks in `world.rs`.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, PrimitiveTopology},
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
};

use crate::{
    props::PROPS,
    terrain::Terrain,
    textures::{Pattern, Textured, flat_uvs},
    world::{GameMap, MainCamera, Sun},
};

pub struct SceneryPlugin;

impl Plugin for SceneryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_models)
            .add_systems(Update, (build_scenery, set_look).run_if(resource_changed::<GameMap>));
    }
}

/// Every model in `props::PROPS`, loaded once.
#[derive(Resource)]
struct PropModels(Vec<Handle<WorldAsset>>);

/// Part of the current map's scenery.
#[derive(Component)]
struct Scenery;

fn load_models(mut commands: Commands, asset_server: Res<AssetServer>) {
    let models = PROPS
        .iter()
        .map(|model| asset_server.load(GltfAssetLabel::Scene(0).from_asset(model.path)))
        .collect();
    commands.insert_resource(PropModels(models));
}

fn build_scenery(
    mut commands: Commands,
    map: Res<GameMap>,
    models: Res<PropModels>,
    old: Query<Entity, With<Scenery>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    (mut images, mut textured): (ResMut<Assets<Image>>, ResMut<Textured>),
) {
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let map = &map.0;
    for prop in &map.props {
        commands.spawn((
            Scenery,
            WorldAssetRoot(models.0[prop.model].clone()),
            Transform::from_translation(prop.pos)
                .with_rotation(Quat::from_rotation_y(prop.yaw))
                .with_scale(Vec3::splat(prop.scale)),
        ));
    }
    let Some(terrain) = &map.terrain else {
        return;
    };
    let mut dressed = |mut material: StandardMaterial, pattern: Pattern, tile: f32| {
        textured.dress(&mut material, pattern, tile, &mut images);
        material
    };

    commands.spawn((
        Scenery,
        Mesh3d(meshes.add(flat_uvs(ground_mesh(terrain)))),
        MeshMaterial3d(materials.add(dressed(
            StandardMaterial {
                perceptual_roughness: 0.95,
                reflectance: 0.2,
                ..default()
            },
            Pattern::Ground,
            4.0,
        ))),
        Transform::IDENTITY,
    ));

    let asphalt = materials.add(dressed(
        StandardMaterial {
            base_color: Color::srgb(0.17, 0.17, 0.18),
            perceptual_roughness: 0.85,
            depth_bias: 50.0,
            ..default()
        },
        Pattern::Asphalt,
        4.0,
    ));
    let paint = materials.add(dressed(
        StandardMaterial {
            base_color: Color::srgb(0.85, 0.82, 0.70),
            perceptual_roughness: 0.7,
            depth_bias: 100.0,
            ..default()
        },
        Pattern::Paint,
        2.0,
    ));
    for road in map.roads.iter().filter(|road| road.paved) {
        let line = sample_line(&road.points, 2.0);
        commands.spawn((
            Scenery,
            Mesh3d(meshes.add(flat_uvs(strip_mesh(&line, road.width, |p| terrain.height(p.x, p.y) + 0.04, None)))),
            MeshMaterial3d(asphalt.clone()),
            NotShadowCaster,
        ));
        // A dashed line down the middle.
        commands.spawn((
            Scenery,
            Mesh3d(meshes.add(flat_uvs(strip_mesh(&line, 0.15, |p| terrain.height(p.x, p.y) + 0.06, Some(2))))),
            MeshMaterial3d(paint.clone()),
            NotShadowCaster,
        ));
    }

    let water = materials.add(dressed(
        StandardMaterial {
            base_color: Color::srgba(0.16, 0.30, 0.34, 0.78),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.08,
            reflectance: 0.6,
            ..default()
        },
        Pattern::Water,
        6.0,
    ));
    for (points, width) in &map.water {
        let line: Vec<Vec2> = points.iter().map(|p| Vec2::new(p.x, p.z)).collect();
        let heights: Vec<f32> = points.iter().map(|p| p.y).collect();
        let mut index = 0;
        let mesh = strip_mesh(&line, *width, |_| {
            let h = heights[index.min(heights.len() - 1)];
            index += 1;
            h
        }, None);
        commands.spawn((Scenery, Mesh3d(meshes.add(flat_uvs(mesh))), MeshMaterial3d(water.clone()), NotShadowCaster));
    }
}

/// The sky, the sun, the ambient light and the fog of the map.
fn set_look(
    mut commands: Commands,
    map: Res<GameMap>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    sun: Single<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    camera: Single<Entity, With<MainCamera>>,
) {
    let look = &map.0.look;
    let [r, g, b] = look.sky;
    let sky = Color::srgb(r, g, b);
    clear.0 = sky;
    ambient.brightness = look.ambient;
    let (mut light, mut transform) = sun.into_inner();
    light.illuminance = look.sun;
    *transform = Transform::from_translation(look.sun_from).looking_at(Vec3::ZERO, Vec3::Y);
    match look.fog {
        Some((start, end)) => {
            commands.entity(*camera).insert(DistanceFog {
                color: sky,
                directional_light_color: Color::srgba(1.0, 0.95, 0.85, 0.4),
                directional_light_exponent: 24.0,
                falloff: FogFalloff::Linear { start, end },
            });
        }
        None => {
            commands.entity(*camera).remove::<DistanceFog>();
        }
    }
}

/// The ground as one mesh: the same triangles the collision uses, smooth
/// normals, and each vertex coloured, with a little grain so it does not
/// look painted on.
fn ground_mesh(terrain: &Terrain) -> Mesh {
    let n = terrain.size;
    let mut positions = Vec::with_capacity(n * n);
    let mut normals = Vec::with_capacity(n * n);
    let mut colors = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let v = terrain.vertex(i, j);
            positions.push(v.to_array());
            // Central differences, for smooth shading.
            let h = |a: usize, b: usize| terrain.at(a.min(n - 1), b.min(n - 1));
            let dx = h(i + 1, j) - h(i.saturating_sub(1), j);
            let dz = h(i, j + 1) - h(i, j.saturating_sub(1));
            normals.push(Vec3::new(-dx, 2.0 * terrain.cell, -dz).normalize().to_array());
            let [r, g, b] = terrain.colors.get(j * n + i).copied().unwrap_or([0.33, 0.45, 0.19]);
            let grain = 0.9 + 0.2 * hash01(i as u32, j as u32);
            let color = Color::srgb(r * grain, g * grain, b * grain).to_linear();
            colors.push([color.red, color.green, color.blue, 1.0]);
        }
    }
    let mut indices = Vec::with_capacity((n - 1) * (n - 1) * 6);
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let v00 = (j * n + i) as u32;
            let v10 = v00 + 1;
            let v01 = v00 + n as u32;
            let v11 = v01 + 1;
            // Split along the same diagonal as `Terrain::height`.
            indices.extend_from_slice(&[v00, v11, v10, v00, v01, v11]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

/// Points along a line every `step` metres (and its corners).
fn sample_line(points: &[Vec2], step: f32) -> Vec<Vec2> {
    let mut out = vec![points[0]];
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let pieces = (a.distance(b) / step).ceil().max(1.0) as usize;
        out.extend((1..=pieces).map(|k| a.lerp(b, k as f32 / pieces as f32)));
    }
    out
}

/// A flat ribbon `width` wide along `line`, each point raised to `height`.
/// With `dashes`, only every other run of that many segments is kept.
fn strip_mesh(line: &[Vec2], width: f32, mut height: impl FnMut(Vec2) -> f32, dashes: Option<usize>) -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (k, p) in line.iter().enumerate() {
        let before = line[k.saturating_sub(1)];
        let after = line[(k + 1).min(line.len() - 1)];
        let dir = (after - before).normalize_or_zero();
        let side = Vec2::new(-dir.y, dir.x) * width / 2.0;
        let y = height(*p);
        for edge in [*p - side, *p + side] {
            positions.push([edge.x, y, edge.y]);
        }
        if k == 0 || dashes.is_some_and(|run| (k - 1) / run % 2 == 1) {
            continue;
        }
        let (a, b) = ((2 * k - 2) as u32, (2 * k) as u32);
        // Wound to face up.
        indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
    }
    let count = positions.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
        .with_inserted_indices(Indices::U32(indices))
}

fn hash01(x: u32, z: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ z.wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}
