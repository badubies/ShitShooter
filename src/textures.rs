//! Surface textures for the maps (blocks, ground, roads, water, props) and
//! the guns, made here rather than downloaded. Each is a tile that repeats: a colour map that multiplies the
//! material's colour, a normal map from the same height field, and a
//! roughness map. A material gets one by its name in the model file
//! ("Brick", "Rifle_Polymer"); names not listed in `look` stay plain. The
//! maps' blocks get one by their colour (`block_look`), and `scenery.rs`
//! picks the ground's, the roads' and the water's.
//!
//! The models have no texture coordinates of their own, so each mesh gets
//! them by box projection: every triangle is laid flat on the axis plane it
//! faces most, in the model's own units, and the material's `uv_transform`
//! sets how many units one tile covers. On a roof the tile's top points
//! uphill, so the rows of roof tiles run along the slope.
//!
//! The guns' steel, receivers and sight bodies are made metallic. Metal
//! looks like metal by what it reflects, and the map has nothing to
//! reflect but its flat ambient light, so a small reflection probe follows
//! the first-person camera with a sky-over-ground surround for the gun in
//! hand. It is too small to reach the map.

use std::collections::{HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    gltf::GltfMaterialName,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{EnvironmentMapLight, GeneratedEnvironmentMapLight, LightProbe},
    math::Affine2,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension},
    transform::TransformSystems,
};

use crate::{
    ramp::Ramp,
    world::{GameMap, MainCamera},
};

pub struct TexturesPlugin;

impl Plugin for TexturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Textured>()
            .add_systems(Startup, spawn_reflections)
            .add_systems(Update, reflections_brightness.run_if(resource_changed::<GameMap>))
            .add_systems(PostUpdate, (texture_models, follow_camera.before(TransformSystems::Propagate)));
    }
}

/// Texels across a tile.
const SIZE: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pattern {
    Plaster,
    Brick,
    Stone,
    /// Planks lying along the tile.
    Wood,
    /// Planks standing up, like a barn's boards.
    Boards,
    Concrete,
    Metal,
    Corrugated,
    CorrugatedRust,
    Rust,
    RoofTiles,
    Cloth,
    Hay,
    Bark,
    BirchBark,
    Leaves,
    Rock,
    Moss,
    Paint,
    Rubber,
    GunSteel,
    Anodized,
    Polymer,
    Grip,
    /// Arena's wall and floor plates, with seams and bolts.
    Panels,
    /// Depot's walls.
    Cinder,
    /// Concrete poured in slabs, a seam round each tile.
    Slab,
    Ground,
    Asphalt,
    Water,
}

#[cfg(test)]
const PATTERNS: [Pattern; 30] = {
    use Pattern::*;
    [
        Plaster, Brick, Stone, Wood, Boards, Concrete, Metal, Corrugated, CorrugatedRust, Rust, RoofTiles, Cloth, Hay,
        Bark, BirchBark, Leaves, Rock, Moss, Paint, Rubber, GunSteel, Anodized, Polymer, Grip, Panels, Cinder, Slab,
        Ground, Asphalt, Water,
    ]
};

/// The texture a material gets, and how many model units one tile covers:
/// metres for the props, the guns' own (shrunk) units for the guns.
fn look(material: &str) -> Option<(Pattern, f32)> {
    use Pattern::*;
    // The guns' and sights' materials are named "Rifle_Polymer",
    // "Sight_Body", "Mat_Steel"; the props' have no underscore.
    if let Some((_, part)) = material.rsplit_once('_') {
        return Some(match part {
            "Steel" | "Barrel" | "Bolt" => (GunSteel, 0.06),
            "Receiver" | "Slide" | "Body" => (Anodized, 0.08),
            "Polymer" | "Frame" | "Chassis" | "Magazine" | "Dark" => (Polymer, 0.04),
            "Grip" | "Rubber" => (Grip, 0.03),
            _ => return None,
        });
    }
    Some(match material {
        "Plaster" | "PlasterWarm" | "PlasterBlue" => (Plaster, 2.5),
        "Trim" | "Olive" | "SignalRed" | "PaintWhite" => (Paint, 2.0),
        "Stone" | "StoneLight" => (Stone, 2.0),
        "Brick" => (Brick, 1.2),
        "Wood" | "WoodLight" | "WoodGrey" => (Wood, 1.0),
        "BarnRed" => (Boards, 2.0),
        "RoofRed" | "RoofDark" => (RoofTiles, 2.0),
        // The shipping container.
        "RoofGreen" | "Sheet" => (Corrugated, 1.2),
        "SheetRust" => (CorrugatedRust, 1.2),
        "Concrete" | "ConcreteDark" => (Concrete, 2.5),
        "Metal" | "MetalDark" => (Metal, 1.0),
        "Rust" => (Rust, 1.5),
        "Canvas" | "Sandbag" => (Cloth, 0.6),
        "Hay" => (Hay, 1.0),
        "Tyre" => (Rubber, 0.4),
        "Bark" => (Bark, 1.0),
        "BarkBirch" => (BirchBark, 1.0),
        "PineNeedles" | "PineDark" | "Leaves" | "LeavesLight" | "LeavesBirch" | "Bush" => (Leaves, 1.5),
        "Rock" | "RockDark" => (Rock, 3.0),
        "Moss" => (Moss, 2.0),
        _ => return None,
    })
}

/// The textures a map made in Blender can name (`Map::textures`), by its
/// material's name, and the metres one tile covers.
const NAMED: [(&str, Pattern, f32); 16] = {
    use Pattern::*;
    [
        ("panels", Panels, 4.0),
        ("slab", Slab, 4.0),
        ("concrete", Concrete, 2.5),
        ("paint", Paint, 2.0),
        ("plaster", Plaster, 2.5),
        ("brick", Brick, 1.6),
        ("cinder", Cinder, 1.6),
        ("stone", Stone, 2.0),
        ("rock", Rock, 3.0),
        ("wood", Wood, 1.0),
        ("boards", Boards, 1.0),
        ("metal", Metal, 2.0),
        ("corrugated", Corrugated, 1.2),
        ("rust", Rust, 2.0),
        ("asphalt", Asphalt, 4.0),
        ("ground", Ground, 4.0),
    ]
};

/// The texture called `name` (by `NAMED`, any case) and its tile, if any.
pub fn named(name: &str) -> Option<(Pattern, f32)> {
    let name = name.to_lowercase();
    NAMED.iter().find(|(known, ..)| *known == name).map(|(_, pattern, tile)| (*pattern, *tile))
}

/// The texture a map block gets, by its colour, and the metres one tile
/// covers. Unknown colours get concrete.
pub fn block_look(color: [f32; 3]) -> (Pattern, f32) {
    use crate::{gauntlet, hamlet, map, surf};
    use Pattern::*;
    let looks = [
        (map::FLOOR, Panels, 4.0),
        (map::WALL, Panels, 4.0),
        (map::COVER, Paint, 2.0),
        (map::PILLAR, Concrete, 2.5),
        (map::PLATFORM, Panels, 4.0),
        (map::LEDGE, Panels, 4.0),
        (map::DEPOT_FLOOR, Slab, 4.0),
        (map::DEPOT_WALL, Cinder, 1.6),
        (map::CONCRETE, Concrete, 2.5),
        (map::CONTAINER_RED, Corrugated, 1.2),
        (map::CONTAINER_BLUE, Corrugated, 1.2),
        (map::CRATE, Wood, 1.0),
        (map::BARRIER, Paint, 2.0),
        (map::ROOF, Corrugated, 1.2),
        (hamlet::STONE, Stone, 2.0),
        (hamlet::STONE_DARK, Stone, 2.0),
        (hamlet::PLASTER, Plaster, 2.5),
        (hamlet::WOOD, Wood, 1.0),
        (surf::RAMP, Panels, 4.0),
        (surf::DECK, Paint, 2.0),
        (gauntlet::CHUTE, Panels, 4.0),
        (gauntlet::RIDGE, Panels, 4.0),
        (gauntlet::DECK, Slab, 4.0),
        (gauntlet::PIT, Concrete, 6.0),
        (gauntlet::PILLAR, Concrete, 2.5),
    ];
    looks
        .into_iter()
        .find(|(known, ..)| *known == color)
        .map_or((Concrete, 2.5), |(_, pattern, tile)| (pattern, tile))
}

// ---------------------------------------------------------------- putting them on

/// One pattern's textures, made the first time a material wants it.
struct Set {
    albedo: Handle<Image>,
    normal: Handle<Image>,
    rough: Handle<Image>,
    /// The colour map's average, which the material's colour is divided by
    /// so the model keeps the colour it had.
    mean: Vec3,
    mean_rough: f32,
}

#[derive(Resource, Default)]
pub struct Textured {
    sets: HashMap<Pattern, Set>,
    meshes: HashSet<AssetId<Mesh>>,
    materials: HashSet<AssetId<StandardMaterial>>,
}

impl Textured {
    /// Puts a pattern on a material, `tile` units to a tile, keeping its
    /// average colour. Its meshes need texture coordinates in those units
    /// (`box_project`, or `flat_uvs` for what lies flat) and tangents.
    pub fn dress(&mut self, material: &mut StandardMaterial, pattern: Pattern, tile: f32, images: &mut Assets<Image>) {
        let set = self.sets.entry(pattern).or_insert_with(|| make_set(pattern, images));
        let mut base = material.base_color.to_linear();
        if let Some(shine) = shine(pattern) {
            // A metal's colour is what it reflects: a few times the dark
            // paint the model was given, which keeps each part's darkness
            // next to the others.
            base = LinearRgba::new(
                (base.red * shine).min(0.8),
                (base.green * shine).min(0.8),
                (base.blue * shine).min(0.8),
                base.alpha,
            );
            material.metallic = 1.0;
            // Glossier than the paint was, so the reflections stay sharp.
            material.perceptual_roughness *= 0.65;
        }
        material.base_color = Color::LinearRgba(LinearRgba::new(
            base.red / set.mean.x,
            base.green / set.mean.y,
            base.blue / set.mean.z,
            base.alpha,
        ));
        material.perceptual_roughness = (material.perceptual_roughness / set.mean_rough).min(1.0);
        material.base_color_texture = Some(set.albedo.clone());
        if bumpy(pattern) {
            material.normal_map_texture = Some(set.normal.clone());
        }
        material.metallic_roughness_texture = Some(set.rough.clone());
        material.uv_transform = Affine2::from_scale(Vec2::splat(1.0 / tile));
    }
}

/// Textures every newly spawned part of a model file whose material is
/// listed in `look`. Meshes and materials are shared by every copy of a
/// model, so each is changed once.
fn texture_models(
    mut textured: ResMut<Textured>,
    parts: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &GltfMaterialName), Added<GltfMaterialName>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let textured = &mut *textured;
    for (mesh, material, name) in &parts {
        let Some((pattern, tile)) = look(&name.0) else {
            continue;
        };
        if textured.meshes.insert(mesh.id())
            && let Some(mut mesh) = meshes.get_mut(mesh)
            && !box_project(&mut mesh, Vec3::ZERO)
        {
            warn!("could not lay a texture on a mesh of {}", name.0);
        }
        if !textured.materials.insert(material.id()) {
            continue;
        }
        let Some(mut material) = materials.get_mut(material) else {
            continue;
        };
        if material.base_color_texture.is_some() {
            continue;
        }
        textured.dress(&mut material, pattern, tile, &mut images);
    }
}

/// Whether a pattern gets its normal map. The guns' rounded parts would
/// show the box projection's seams as lumps, so only their grips do.
fn bumpy(pattern: Pattern) -> bool {
    !matches!(pattern, Pattern::GunSteel | Pattern::Anodized | Pattern::Polymer)
}

/// How many times brighter than its paint a pattern's metal is, for the
/// patterns that are metal.
fn shine(pattern: Pattern) -> Option<f32> {
    match pattern {
        Pattern::GunSteel => Some(2.5),
        Pattern::Anodized => Some(2.5),
        _ => None,
    }
}

// ---------------------------------------------------------------- the gun's reflections

/// Metres across the reflection probe around the first-person camera: big
/// enough for the longest gun held out, small enough to miss the map.
const PROBE_SIZE: f32 = 2.0;

#[derive(Component)]
struct GunReflections;

fn spawn_reflections(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((
        GunReflections,
        LightProbe::new(),
        GeneratedEnvironmentMapLight {
            environment_map: images.add(surroundings()),
            intensity: 400.0,
            ..default()
        },
        Transform::from_scale(Vec3::splat(PROBE_SIZE)),
    ));
}

/// Keeps the probe on the camera. Only its place follows: turned with the
/// camera, the reflections would turn with the view.
fn follow_camera(
    camera: Single<&Transform, (With<MainCamera>, Without<GunReflections>)>,
    mut probe: Single<&mut Transform, With<GunReflections>>,
) {
    probe.translation = camera.translation;
}

/// As bright as the map's ambient light, with room for the highlights, so
/// the gun suits the map.
fn reflections_brightness(
    map: Res<GameMap>,
    mut probe: Single<(&mut GeneratedEnvironmentMapLight, Option<&mut EnvironmentMapLight>), With<GunReflections>>,
) {
    let intensity = map.0.look.ambient * 2.5;
    probe.0.intensity = intensity;
    if let Some(light) = &mut probe.1 {
        light.intensity = intensity;
    }
}

/// Texels across each face of the surround.
const SURROUND_SIZE: u32 = 64;

/// A cube map that is mostly dark, with a bright sky overhead and a thin
/// bright horizon: on dark metal it makes crisp highlights along the top
/// edges and a line along the sides, which is what reads as metal.
fn surroundings() -> Image {
    let n = SURROUND_SIZE as usize;
    let mut data = Vec::with_capacity(n * n * 6 * 4);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let s = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                let dir = match face {
                    0 => Vec3::new(1.0, -t, -s),
                    1 => Vec3::new(-1.0, -t, s),
                    2 => Vec3::new(s, 1.0, t),
                    3 => Vec3::new(s, -1.0, -t),
                    4 => Vec3::new(s, -t, 1.0),
                    _ => Vec3::new(-s, -t, -1.0),
                }
                .normalize();
                let [r, g, b] = encode_srgb(surrounding(dir));
                data.extend([r, g, b, 255]);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: SURROUND_SIZE,
            height: SURROUND_SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

/// The light coming from `dir` (linear, at most 1).
fn surrounding(dir: Vec3) -> Vec3 {
    let y = dir.y;
    let sky = smoothstep(0.35, 0.85, y);
    let horizon = (-(y / 0.07).powi(2)).exp();
    let ground = if y < 0.0 { 0.03 } else { 0.06 };
    Vec3::new(0.85, 0.92, 1.0) * sky + Vec3::new(1.0, 0.95, 0.88) * horizon + Vec3::splat(ground)
}

// ---------------------------------------------------------------- texture coordinates

/// Gives the mesh texture coordinates by box projection, and tangents for
/// the normal map. Its triangles stop sharing corners, so each can face its
/// own plane; the normals stay as they were, so smooth shading stays smooth.
/// `offset` is added to the positions first: where the mesh stands, so
/// that neighbouring meshes' textures meet.
pub fn box_project(mesh: &mut Mesh, offset: Vec3) -> bool {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList || mesh.try_duplicate_vertices().is_err() {
        return false;
    }
    let Ok(VertexAttributeValues::Float32x3(positions)) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION) else {
        return false;
    };
    if positions.len() % 3 != 0 {
        return false;
    }
    let uvs: Vec<[f32; 2]> = positions
        .chunks_exact(3)
        .flat_map(|corners| {
            let [a, b, c] = [corners[0], corners[1], corners[2]].map(|p| Vec3::from(p) + offset);
            let n = (b - a).cross(c - a);
            let (x, y, z) = (n.x.abs(), n.y.abs(), n.z.abs());
            [a, b, c].map(|p| {
                if y >= x && y >= z {
                    // Facing up or down: the tile's top points uphill.
                    if x > z {
                        [p.z, p.x * n.x.signum() * n.y.signum()]
                    } else {
                        [p.x, p.z * n.z.signum() * n.y.signum()]
                    }
                } else if x > z {
                    [-p.z * n.x.signum(), -p.y]
                } else {
                    [p.x * n.z.signum(), -p.y]
                }
            })
        })
        .collect();
    mesh.try_insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs).is_ok() && mesh.generate_tangents().is_ok()
}

/// A map block's box, centred on its middle, textured in world units.
pub fn block_mesh(min: Vec3, max: Vec3) -> Mesh {
    let mut mesh = Cuboid::from_size(max - min).mesh().build();
    box_project(&mut mesh, (min + max) / 2.0);
    mesh
}

/// A ramp, in world coordinates, every face flat and textured in metres
/// across it: along the ramp, and up the slope.
pub fn ramp_mesh(ramp: &Ramp) -> Mesh {
    let (mut positions, mut normals, mut uvs) = (Vec::new(), Vec::new(), Vec::new());
    let mut indices = Vec::new();
    for (index, face) in ramp.faces.iter().enumerate() {
        let normal = ramp.normal(index);
        let across = if normal.y.abs() < 0.99 { Vec3::Y.cross(normal).normalize() } else { Vec3::X };
        let up = normal.cross(across);
        let first = positions.len() as u32;
        for &corner in face.iter() {
            let p = ramp.corners[corner];
            positions.push(p.to_array());
            normals.push(normal.to_array());
            uvs.push([p.dot(across), -p.dot(up)]);
        }
        // A fan, each triangle wound to face out.
        for k in 1..face.len() as u32 - 1 {
            let [a, b, c] = [first, first + k, first + k + 1];
            let corner = |i: u32| Vec3::from(positions[i as usize]);
            let facing = (corner(b) - corner(a)).cross(corner(c) - corner(a)).dot(normal);
            indices.extend_from_slice(&if facing >= 0.0 { [a, b, c] } else { [a, c, b] });
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices));
    if let Err(error) = mesh.generate_tangents() {
        warn!("no tangents for a ramp: {error}");
    }
    mesh
}

/// Texture coordinates for a mesh that lies flat (the ground, a road): its
/// x and z in metres, and tangents. Its corners stay shared.
pub fn flat_uvs(mut mesh: Mesh) -> Mesh {
    if let Ok(VertexAttributeValues::Float32x3(positions)) = mesh.try_attribute(Mesh::ATTRIBUTE_POSITION) {
        let uvs: Vec<[f32; 2]> = positions.iter().map(|p| [p[0], p[2]]).collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        if let Err(error) = mesh.generate_tangents() {
            warn!("no tangents for a flat mesh: {error}");
        }
    }
    mesh
}

// ---------------------------------------------------------------- making them

/// A texel of a pattern before it is stored: linear colour (at most 1),
/// height (0 to 1) and how rough it is (multiplies the material's).
struct Texel {
    albedo: Vec3,
    height: f32,
    rough: f32,
}

/// A pattern's tile, texel by texel, then mip-mapped into images.
fn make_set(pattern: Pattern, images: &mut Assets<Image>) -> Set {
    let (albedo, normal, rough, mean, mean_rough) = make_maps(pattern);
    Set {
        albedo: images.add(albedo),
        normal: images.add(normal),
        rough: images.add(rough),
        mean,
        mean_rough,
    }
}

pub fn make_maps(pattern: Pattern) -> (Image, Image, Image, Vec3, f32) {
    let texels: Vec<Texel> = (0..SIZE * SIZE)
        .map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            texel(pattern, (x as f32 + 0.5) / SIZE as f32, (y as f32 + 0.5) / SIZE as f32)
        })
        .collect();
    let depth = depth(pattern);
    let height = |x: usize, y: usize| texels[(y % SIZE) * SIZE + x % SIZE].height;
    let normals: Vec<Vec4> = (0..SIZE * SIZE)
        .map(|i| {
            let (x, y) = (i % SIZE, i / SIZE);
            let dx = (height(x + 1, y) - height(x + SIZE - 1, y)) * 0.5 * depth;
            let dy = (height(x, y + 1) - height(x, y + SIZE - 1)) * 0.5 * depth;
            // +Y in the normal map is the tile's top, where y is 0.
            Vec3::new(-dx, dy, 1.0).normalize().extend(1.0)
        })
        .collect();

    let albedo: Vec<Vec4> = texels.iter().map(|t| t.albedo.clamp(Vec3::ZERO, Vec3::ONE).extend(1.0)).collect();
    let rough: Vec<Vec4> = texels.iter().map(|t| Vec4::new(0.0, t.rough.clamp(0.05, 1.0), 1.0, 1.0)).collect();
    // The averages as stored, so the material's colour divides out exactly.
    let mean = albedo.iter().map(|c| decode_srgb(encode_srgb(c.truncate()))).sum::<Vec3>() / albedo.len() as f32;
    let mean_rough = rough.iter().map(|r| unorm(r.y) as f32 / 255.0).sum::<f32>() / rough.len() as f32;

    let albedo = image(albedo, TextureFormat::Rgba8UnormSrgb, false, |c| {
        let [r, g, b] = encode_srgb(c.truncate());
        [r, g, b, 255]
    });
    let normal = image(normals, TextureFormat::Rgba8Unorm, true, |n| {
        [unorm(n.x * 0.5 + 0.5), unorm(n.y * 0.5 + 0.5), unorm(n.z * 0.5 + 0.5), 255]
    });
    let rough = image(rough, TextureFormat::Rgba8Unorm, false, |r| [0, unorm(r.y), unorm(r.z), 255]);
    (albedo, normal, rough, mean.max(Vec3::splat(0.01)), mean_rough.max(0.01))
}

/// An image of the tile with its whole mip chain, each level the average of
/// the one above (normals renormalised), sampled so it repeats.
fn image(mut level: Vec<Vec4>, format: TextureFormat, normal: bool, store: impl Fn(Vec4) -> [u8; 4]) -> Image {
    let mut size = SIZE;
    let mut data = Vec::new();
    let mut levels = 0;
    loop {
        data.extend(level.iter().flat_map(|&v| store(v)));
        levels += 1;
        if size == 1 {
            break;
        }
        let half = size / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                let sum = level[y * size + x] + level[y * size + x + 1] + level[(y + 1) * size + x] + level[(y + 1) * size + x + 1];
                if normal {
                    (sum.truncate().normalize_or(Vec3::Z)).extend(1.0)
                } else {
                    sum / 4.0
                }
            })
            .collect();
        size = half;
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        // The player's "Texture filtering". 16 halved the frame rate on
        // Valley, whose ground runs to the horizon at a grazing angle; 4
        // (the default) costs next to nothing.
        anisotropy_clamp: crate::graphics::texture_filtering_in_use(),
        ..default()
    });
    image
}

fn unorm(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn encode_srgb(c: Vec3) -> [u8; 3] {
    c.to_array().map(|x| {
        let x = x.clamp(0.0, 1.0);
        unorm(if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 })
    })
}

fn decode_srgb(c: [u8; 3]) -> Vec3 {
    Vec3::from_array(c.map(|b| {
        let x = b as f32 / 255.0;
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    }))
}

/// How steep a pattern's bumps are: a step of the whole height across one
/// texel tilts the normal this many times 45 degrees.
fn depth(pattern: Pattern) -> f32 {
    use Pattern::*;
    match pattern {
        Plaster => 3.0,
        Brick => 6.0,
        Stone => 6.0,
        Wood | Boards => 4.0,
        Concrete => 3.0,
        Metal => 1.5,
        Corrugated | CorrugatedRust => 5.0,
        Rust => 3.0,
        RoofTiles => 6.0,
        Cloth => 3.0,
        Hay => 4.0,
        Bark => 6.0,
        BirchBark => 3.0,
        Leaves => 5.0,
        Rock => 8.0,
        Moss => 4.0,
        Paint => 1.0,
        Rubber => 2.0,
        GunSteel => 0.4,
        Anodized => 0.3,
        Polymer => 0.8,
        Grip => 2.0,
        Panels => 4.0,
        Cinder => 5.0,
        Slab => 3.0,
        Ground => 3.0,
        Asphalt => 2.0,
        Water => 5.0,
    }
}

/// One texel of a pattern at `u`, `v` (0 to 1 across the tile, `v` down).
fn texel(pattern: Pattern, u: f32, v: f32) -> Texel {
    use Pattern::*;
    let fbm = |px: i32, py: i32, octaves: u32, seed: u32| fbm(u, v, px, py, octaves, seed);
    let grey = Vec3::splat;
    match pattern {
        Plaster => {
            let bumps = fbm(16, 16, 4, 1);
            let blotch = fbm(3, 3, 4, 2);
            Texel {
                albedo: grey(0.86 + 0.16 * spread(blotch) + 0.06 * spread(bumps)),
                height: bumps,
                rough: 1.0,
            }
        }
        Brick => {
            let (rows, across) = (8.0, 4.0);
            let row = (v * rows).floor();
            let fy = v * rows - row;
            let x = u * across + if row as i32 % 2 == 1 { 0.5 } else { 0.0 };
            let col = x.floor();
            let fx = x - col;
            // From the mortar's middle, in rows; a brick is two rows long.
            let edge = (fx.min(1.0 - fx) * rows / across).min(fy.min(1.0 - fy));
            let brick = smoothstep(0.07, 0.12, edge);
            let id = hash(col as i32 % across as i32, row as i32, 3);
            let grain = fbm(24, 24, 3, 4);
            let fired = Vec3::new(0.62 + 0.18 * id, 0.55 + 0.12 * id, 0.52 + 0.1 * id) * (0.9 + 0.25 * spread(grain));
            let mortar = Vec3::new(0.8, 0.95, 1.0) * (0.9 + 0.2 * spread(grain));
            Texel {
                albedo: mortar.lerp(fired, brick),
                height: brick * (0.8 + 0.2 * grain),
                rough: 1.0,
            }
        }
        Stone => {
            let rows = 4.0;
            let row = (v * rows).floor();
            let fy = v * rows - row;
            let x = u * 3.0 + hash(row as i32, 0, 5);
            let col = x.floor();
            let fx = x - col;
            let edge = (fx.min(1.0 - fx) / 3.0).min(fy.min(1.0 - fy) / rows);
            let block = smoothstep(0.006, 0.03, edge);
            let id = hash(col.rem_euclid(3.0) as i32, row as i32, 6);
            let surface = fbm(8, 8, 5, 7);
            Texel {
                albedo: grey(0.55).lerp(grey(0.78 + 0.22 * id + 0.2 * spread(surface)), block),
                height: block * 0.7 + 0.3 * surface,
                rough: 1.0,
            }
        }
        Wood => planks(u, v, 4.0),
        Boards => planks(v, u, 8.0),
        Concrete => {
            let blotch = fbm(3, 3, 4, 10);
            let fine = fbm(48, 48, 2, 11);
            let (d, id) = cells(u, v, 24, 12);
            let pit = if id > 0.6 { 1.0 - smoothstep(0.08, 0.16, d) } else { 0.0 };
            Texel {
                albedo: grey((0.86 + 0.16 * spread(blotch) + 0.06 * spread(fine)) * (1.0 - 0.3 * pit)),
                height: 0.5 + 0.3 * fine - 0.5 * pit,
                rough: 1.0,
            }
        }
        Metal => {
            let brushed = fbm(1, 96, 3, 20);
            let wear = fbm(4, 4, 4, 21);
            Texel {
                albedo: grey(0.88 + 0.08 * spread(brushed) + 0.12 * spread(wear)),
                height: brushed * 0.5,
                rough: 0.75 + 0.25 * smoothstep(0.3, 0.7, wear),
            }
        }
        Corrugated | CorrugatedRust => {
            let ridge = 0.5 + 0.5 * (std::f32::consts::TAU * u * 10.0).cos();
            let streaks = fbm(32, 2, 3, 30);
            let mut albedo = grey(0.86 + 0.12 * spread(streaks) + 0.06 * ridge);
            let mut rough = 0.85 + 0.15 * streaks;
            if pattern == CorrugatedRust {
                let rust = smoothstep(0.5, 0.62, fbm(4, 4, 5, 31) + 0.15 * spread(streaks));
                albedo = albedo.lerp(Vec3::new(0.85, 0.5, 0.32) * (0.8 + 0.3 * fbm(32, 32, 2, 32)), rust);
                rough = rough.lerp(1.0, rust);
            }
            Texel {
                albedo,
                height: ridge,
                rough,
            }
        }
        Rust => {
            let rust = smoothstep(0.45, 0.7, fbm(4, 4, 5, 40));
            let pits = fbm(48, 48, 2, 41);
            Texel {
                albedo: grey(0.95).lerp(Vec3::new(0.85, 0.55, 0.4), rust) * (0.9 + 0.15 * spread(pits)),
                height: 0.5 + 0.3 * rust * pits,
                rough: 0.8 + 0.2 * rust,
            }
        }
        RoofTiles => {
            let (rows, across) = (8.0, 6.0);
            let row = (v * rows).floor();
            let fy = v * rows - row;
            let x = u * across + if row as i32 % 2 == 1 { 0.5 } else { 0.0 };
            let col = x.floor();
            let fx = x - col;
            let gap = smoothstep(0.0, 0.06, fx.min(1.0 - fx));
            let id = hash(col as i32 % across as i32, row as i32, 50);
            let dirt = fbm(4, 4, 4, 51);
            // Each row's lower edge lies on the row below, so a tile rises
            // towards its bottom and sits in the shade of the row above.
            let shade = 0.7 + 0.3 * smoothstep(0.0, 0.35, fy);
            Texel {
                albedo: grey((0.8 + 0.2 * id + 0.12 * spread(dirt)) * shade * (0.6 + 0.4 * gap)),
                height: (0.3 + 0.7 * fy) * (0.7 + 0.3 * gap),
                rough: 1.0,
            }
        }
        Cloth => {
            let threads = 48.0;
            let (fu, fv) = ((u * threads).fract(), (v * threads).fract());
            let over = ((u * threads) as i32 + (v * threads) as i32) % 2 == 0;
            let weave = if over { (fv * std::f32::consts::PI).sin() } else { (fu * std::f32::consts::PI).sin() };
            let blotch = fbm(4, 4, 4, 60);
            Texel {
                albedo: grey(0.86 + 0.08 * spread(weave) + 0.14 * spread(blotch)),
                height: weave,
                rough: 1.0,
            }
        }
        Hay => {
            // Thin straws lying four ways: along, across and both diagonals
            // (which still repeat with the tile).
            let (s, t) = (u + v, u - v);
            let straws = [
                self::fbm(u, v, 2, 96, 2, 70),
                self::fbm(v, u, 2, 96, 2, 71),
                self::fbm(s, t, 2, 64, 2, 72),
                self::fbm(t, s, 2, 64, 2, 73),
            ];
            let straw = smoothstep(0.55, 0.8, straws.into_iter().fold(0.0, f32::max));
            Texel {
                albedo: grey(0.55 + 0.45 * straw),
                height: straw,
                rough: 1.0,
            }
        }
        Bark => {
            // Deep cracks wandering up the trunk between fibrous plates.
            let crack = smoothstep(0.8, 0.95, 1.0 - (2.0 * fbm(10, 2, 4, 80) - 1.0).abs());
            let fibre = fbm(48, 6, 2, 81);
            Texel {
                albedo: grey((0.8 + 0.2 * spread(fibre)) * (1.0 - 0.6 * crack)),
                height: 0.8 * (1.0 - crack) + 0.2 * fibre,
                rough: 1.0,
            }
        }
        BirchBark => {
            let marks = smoothstep(0.6, 0.68, fbm(3, 24, 3, 90));
            Texel {
                albedo: grey((0.95 + 0.05 * spread(fbm(8, 8, 3, 91))) * (1.0 - 0.75 * marks)),
                height: 1.0 - 0.5 * marks,
                rough: 1.0,
            }
        }
        Leaves => {
            let (d, id) = cells(u, v, 10, 100);
            let (d2, id2) = cells(u, v, 16, 101);
            let big = (1.0 - smoothstep(0.3, 0.75, d)) * (0.75 + 0.25 * id);
            let small = (1.0 - smoothstep(0.25, 0.7, d2)) * (0.7 + 0.3 * id2);
            let leaf = big.max(small);
            Texel {
                albedo: grey(0.5 + 0.5 * leaf),
                height: leaf,
                rough: 1.0,
            }
        }
        Rock => {
            let base = fbm(4, 4, 6, 110);
            let ridged = 1.0 - (2.0 * fbm(3, 3, 4, 111) - 1.0).abs();
            let crack = smoothstep(0.9, 0.97, ridged);
            let strata = 0.5 + 0.5 * (std::f32::consts::TAU * (v * 6.0 + base * 1.5)).sin();
            Texel {
                albedo: grey((0.8 + 0.3 * spread(base) + 0.06 * strata) * (1.0 - 0.45 * crack)),
                height: base * 0.75 + strata * 0.15 - crack * 0.3 + 0.3,
                rough: 1.0,
            }
        }
        Moss => {
            let moss = smoothstep(0.3, 0.7, fbm(8, 8, 5, 120));
            Texel {
                albedo: grey(0.65 + 0.35 * moss),
                height: moss,
                rough: 1.0,
            }
        }
        Paint => {
            let blotch = fbm(4, 4, 4, 130);
            let fine = fbm(32, 32, 2, 131);
            Texel {
                albedo: grey(0.92 + 0.06 * spread(blotch) + 0.04 * spread(fine)),
                height: fine,
                rough: 0.85 + 0.15 * blotch,
            }
        }
        Rubber => {
            let fine = fbm(48, 48, 2, 140);
            Texel {
                albedo: grey(0.9 + 0.08 * spread(fine)),
                height: fine,
                rough: 1.0,
            }
        }
        GunSteel => {
            let brushed = fbm(1, 96, 2, 150);
            let wear = fbm(5, 5, 4, 151);
            Texel {
                albedo: grey(0.95 + 0.03 * spread(brushed) + 0.03 * spread(wear)),
                height: brushed,
                rough: 0.85 + 0.15 * smoothstep(0.3, 0.7, wear),
            }
        }
        Anodized => {
            let fine = fbm(64, 64, 2, 160);
            let wear = fbm(4, 4, 4, 161);
            Texel {
                albedo: grey(0.96 + 0.02 * spread(fine) + 0.03 * spread(wear)),
                height: fine,
                rough: 0.9 + 0.1 * smoothstep(0.3, 0.7, wear),
            }
        }
        Polymer => {
            // A fine matte grain, worn smoother in patches.
            let grain = fbm(64, 64, 2, 170);
            let wear = fbm(4, 4, 4, 171);
            Texel {
                albedo: grey(0.94 + 0.05 * spread(grain) + 0.05 * spread(wear)),
                height: grain,
                rough: 0.9 + 0.1 * smoothstep(0.3, 0.7, wear),
            }
        }
        Grip => {
            let (d, id) = cells(u, v, 32, 180);
            let bump = (1.0 - d / 0.7).clamp(0.0, 1.0) * (0.8 + 0.2 * id);
            Texel {
                albedo: grey(0.9 + 0.1 * bump),
                height: bump,
                rough: 1.0,
            }
        }
        Panels => {
            // Two by two plates, each with a bolt in from every corner.
            let (x, y) = (u * 2.0, v * 2.0);
            let (col, row) = (x.floor(), y.floor());
            let (fx, fy) = (x - col, y - row);
            let (ex, ey) = (fx.min(1.0 - fx), fy.min(1.0 - fy));
            let seam = smoothstep(0.004, 0.012, ex.min(ey));
            let bolt = 1.0 - smoothstep(0.008, 0.014, Vec2::new(ex - 0.05, ey - 0.05).length());
            let id = hash(col as i32 % 2, row as i32 % 2, 210);
            let grime = fbm(4, 4, 4, 211);
            let fine = fbm(64, 64, 2, 212);
            Texel {
                albedo: grey((0.82 + 0.16 * id + 0.1 * spread(grime) + 0.03 * spread(fine)) * (0.45 + 0.55 * seam)),
                height: 0.6 * seam + 0.4 * bolt + 0.05 * fine,
                rough: 0.85 + 0.15 * grime,
            }
        }
        Cinder => {
            let (rows, across) = (8.0, 4.0);
            let row = (v * rows).floor();
            let fy = v * rows - row;
            let x = u * across + if row as i32 % 2 == 1 { 0.5 } else { 0.0 };
            let col = x.floor();
            let fx = x - col;
            let edge = (fx.min(1.0 - fx) * rows / across).min(fy.min(1.0 - fy));
            let block = smoothstep(0.05, 0.09, edge);
            let id = hash(col as i32 % across as i32, row as i32, 230);
            let (d, pid) = cells(u, v, 48, 231);
            let pore = if pid > 0.55 { 1.0 - smoothstep(0.1, 0.25, d) } else { 0.0 };
            let fine = fbm(48, 48, 2, 232);
            Texel {
                albedo: grey(0.72).lerp(grey((0.84 + 0.14 * id + 0.08 * spread(fine)) * (1.0 - 0.25 * pore)), block),
                height: block * (0.85 + 0.15 * fine - 0.3 * pore),
                rough: 1.0,
            }
        }
        Slab => {
            let concrete = texel(Concrete, u, v);
            let seam = smoothstep(0.0, 0.004, u.min(1.0 - u).min(v.min(1.0 - v)));
            Texel {
                albedo: concrete.albedo * (0.55 + 0.45 * seam),
                height: concrete.height * seam,
                rough: 1.0,
            }
        }
        Ground => {
            let clumps = fbm(12, 12, 4, 220);
            let fine = fbm(96, 96, 2, 221);
            let (d, id) = cells(u, v, 40, 222);
            let pebble = if id > 0.9 { 1.0 - smoothstep(0.1, 0.25, d) } else { 0.0 };
            Texel {
                albedo: grey(((0.8 + 0.18 * spread(clumps) + 0.1 * spread(fine)) * (1.0 + 0.08 * pebble)).min(1.0)),
                height: 0.6 * clumps + 0.2 * fine + 0.3 * pebble,
                rough: 1.0,
            }
        }
        Asphalt => {
            let (d, id) = cells(u, v, 96, 250);
            let stone = 1.0 - smoothstep(0.2, 0.45, d);
            let speck = if id > 0.75 { 0.25 } else if id < 0.2 { -0.25 } else { 0.0 } * stone;
            let patches = fbm(3, 3, 4, 251);
            let crack = smoothstep(0.985, 0.995, 1.0 - (2.0 * fbm(4, 4, 5, 252) - 1.0).abs());
            Texel {
                albedo: grey((0.82 + 0.14 * spread(patches) + speck) * (1.0 - 0.4 * crack)),
                height: 0.5 + 0.3 * stone - 0.5 * crack,
                rough: 0.9 + 0.1 * patches,
            }
        }
        Water => {
            let swell = fbm(4, 4, 3, 260);
            let ripples = 0.5 + 0.5 * (std::f32::consts::TAU * (u * 6.0 + v * 2.0 + 2.0 * fbm(3, 3, 2, 261))).sin();
            Texel {
                albedo: grey(1.0),
                height: 0.6 * swell + 0.4 * ripples * fbm(8, 8, 2, 262),
                rough: 1.0,
            }
        }
    }
}

/// Planks lying along `u`, `count` of them across the tile, with grain,
/// seams and butt joints.
fn planks(u: f32, v: f32, count: f32) -> Texel {
    let p = v * count;
    let plank = p.floor();
    let fp = p - plank;
    let id = hash(plank as i32 % count as i32, 0, 200);
    // Each plank is a tile long, butted at its own place along it.
    let along = (u + id).fract();
    let seam = smoothstep(0.0, 0.08, fp.min(1.0 - fp)) * smoothstep(0.0, 0.006, along.min(1.0 - along));
    let warp = fbm(u, v, 3, 6, 2, 201);
    let rings = 0.5 + 0.5 * (std::f32::consts::TAU * (v * 24.0 + 2.5 * warp)).sin();
    let fibre = fbm(u, v, 2, 64, 2, 202);
    Texel {
        albedo: grey3((0.78 + 0.22 * id) * (0.82 + 0.12 * rings + 0.12 * spread(fibre)) * (0.4 + 0.6 * seam)),
        height: seam * (0.8 + 0.1 * rings + 0.1 * fibre),
        rough: 1.0,
    }
}

fn grey3(x: f32) -> Vec3 {
    Vec3::splat(x)
}

/// From 0..1 noise to about -1..1.
fn spread(x: f32) -> f32 {
    (x - 0.5) * 2.0
}

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 0 to 1, the same every time for the same cell.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xff_ffff) as f32 / 16_777_216.0
}

/// Smooth value noise at `x`, `y` (in cells) that repeats every `px` by `py` cells.
fn noise(x: f32, y: f32, px: i32, py: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let at = |i: i32, j: i32| hash(i.rem_euclid(px), j.rem_euclid(py), seed);
    let top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * sx;
    let bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * sx;
    top + (bottom - top) * sy
}

/// Layers of noise over the tile, `px` by `py` cells at the coarsest and
/// twice as fine each layer, so it repeats with the tile. About 0.2 to 0.8.
fn fbm(u: f32, v: f32, px: i32, py: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut total) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        let (qx, qy) = (px << o, py << o);
        sum += amp * noise(u * qx as f32, v * qy as f32, qx, qy, seed.wrapping_add(o * 101));
        total += amp;
        amp *= 0.5;
    }
    sum / total
}

/// The distance (in cells) to the nearest of one random point per cell, on
/// an `n` by `n` grid over the tile, and a 0..1 number for that point.
fn cells(u: f32, v: f32, n: i32, seed: u32) -> (f32, f32) {
    let (x, y) = (u * n as f32, v * n as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let mut best = (f32::MAX, 0.0);
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = (cx + i, cy + j);
            let (wx, wy) = (gx.rem_euclid(n), gy.rem_euclid(n));
            let px = gx as f32 + hash(wx, wy, seed);
            let py = gy as f32 + hash(wx, wy, seed + 1);
            let d = Vec2::new(x - px, y - py).length();
            if d < best.0 {
                best = (d, hash(wx, wy, seed + 2));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes every pattern's colour and normal maps side by side as BMP
    /// files to `$TEXTURE_SHEET` (a directory), to look at.
    #[test]
    #[ignore]
    fn write_sheet() {
        let Ok(dir) = std::env::var("TEXTURE_SHEET") else {
            return;
        };
        for pattern in PATTERNS {
            let (albedo, normal, ..) = make_maps(pattern);
            let take = |image: &Image| image.data.as_ref().unwrap()[..SIZE * SIZE * 4].to_vec();
            let (a, n) = (take(&albedo), take(&normal));
            let (w, h) = (SIZE * 2, SIZE);
            let mut bmp = Vec::new();
            let pixels = w * h * 3;
            bmp.extend(b"BM");
            bmp.extend(((54 + pixels) as u32).to_le_bytes());
            bmp.extend([0u8; 4]);
            bmp.extend(54u32.to_le_bytes());
            bmp.extend(40u32.to_le_bytes());
            bmp.extend((w as i32).to_le_bytes());
            bmp.extend((-(h as i32)).to_le_bytes());
            bmp.extend(1u16.to_le_bytes());
            bmp.extend(24u16.to_le_bytes());
            bmp.extend([0u8; 24]);
            for y in 0..h {
                for x in 0..w {
                    let (src, x) = if x < SIZE { (&a, x) } else { (&n, x - SIZE) };
                    let i = (y * SIZE + x) * 4;
                    bmp.extend([src[i + 2], src[i + 1], src[i]]);
                }
            }
            std::fs::write(format!("{dir}/{pattern:?}.bmp"), bmp).unwrap();
        }
    }

    #[test]
    fn tiles_repeat() {
        // Noise at the tile's far edge meets its near edge.
        for v in [0.1, 0.5, 0.9] {
            let a = fbm(0.0, v, 3, 5, 4, 1);
            let b = fbm(1.0, v, 3, 5, 4, 1);
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
        let (a, _) = cells(0.0, 0.3, 10, 2);
        let (b, _) = cells(1.0, 0.3, 10, 2);
        assert!((a - b).abs() < 1e-4);
    }
}
