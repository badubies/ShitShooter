//! Mist and steam on maps made in Blender: real fog, which the moon and the
//! lamps shine through and things fade into, not shapes that hide what is
//! behind them. A map's mist (`Map::mists`) lies in boxes, thick at the
//! ground and thinning to nothing at its top, drifting on the wind; its
//! vents (`Map::vents`) are grates that a plume of thicker steam comes up
//! out of.
//!
//! All of it is Bevy's volumetric fog: boxes of fog (`FogVolume`), each with
//! a texture that says how thick it is where, moved a little every frame.
//! The textures are made here when the game starts. Fog only shows where a
//! light with shadows shines on it: the sun or the moon, and a map's spot
//! lamps, which cast shadows while there is fog to light. (Its other lamps
//! and its fires do not: a light that shines all round needs six shadow
//! maps, and thirteen of those took the frame rate from 430 to 75.) It can
//! be turned off (Settings, "Fog"), and is off with the shadows.

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    light::{FogVolume, VolumetricFog, VolumetricLight},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::{
    graphics::Shadows,
    settings::Settings,
    world::{GameMap, MainCamera, MapLamp, Sun},
};

/// How much light a metre of mist of density 1 takes out of what is seen
/// through it at the ground (it thins upwards): with this, something 30 m
/// into it is a third fainter.
const MIST: f32 = 0.02;
/// A box of mist is made of pieces no bigger than this across: fog is lit as
/// if each piece were alone, so one long box would come out dark.
const PIECE: f32 = 26.0;
/// The wind: which way the mist drifts, and how fast (m/s).
const WIND: Vec3 = Vec3::new(0.35, 0.0, 0.2);
/// How much of the light fog takes out it scatters (the rest it swallows),
/// and how much more of that goes on the way it was going than back.
const SCATTERING: f32 = 0.55;
const ABSORPTION: f32 = 0.05;
const FORWARD: f32 = 0.15;
/// How much brighter than life the mist and the steam are lit: the moon is
/// dim, and they are to be seen by it.
const MIST_GLOW: f32 = 3.0;
const STEAM_GLOW: f32 = 3.5;
/// Steps along each line of sight through a piece of fog.
const STEPS: u32 = 40;

/// A vent's plume, bottom to top, for a vent of size 1: where each part
/// starts and ends above the grate, how wide it is, how thick (as `MIST` is,
/// by the metre), how fast it rises (m/s) and how far the wind has pushed it.
const PLUME: [(f32, f32, f32, f32, f32, f32); 4] = [
    (0.0, 1.9, 1.9, 0.30, 1.5, 0.0),
    (1.3, 3.7, 3.1, 0.17, 1.2, 0.25),
    (3.0, 6.2, 4.8, 0.085, 0.9, 0.7),
    (5.2, 9.6, 7.2, 0.04, 0.6, 1.5),
];

pub struct FogPlugin;

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_textures)
            .add_systems(Update, (build_fog.run_if(resource_changed::<GameMap>), light_fog, drift));
    }
}

/// How thick fog is where, for a piece of mist and for a part of a plume.
#[derive(Resource)]
struct FogTextures {
    mist: Handle<Image>,
    plume: Handle<Image>,
}

/// A piece of the map's fog.
#[derive(Component)]
struct MapFog;

/// Its texture moves this much of its size a second.
#[derive(Component)]
struct Drift(Vec3);

fn make_textures(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(FogTextures {
        mist: images.add(mist_texture()),
        plume: images.add(plume_texture()),
    });
}

/// Replaces the fog whenever the map changes.
fn build_fog(mut commands: Commands, map: Res<GameMap>, textures: Res<FogTextures>, old: Query<Entity, With<MapFog>>) {
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let volume = |density: f32, glow: f32, texture: &Handle<Image>| FogVolume {
        density_factor: density / (SCATTERING + ABSORPTION),
        density_texture: Some(texture.clone()),
        scattering: SCATTERING,
        absorption: ABSORPTION,
        scattering_asymmetry: FORWARD,
        light_intensity: glow,
        ..default()
    };
    for mist in &map.0.mists {
        let turn = Quat::from_rotation_y(mist.yaw);
        let across = (mist.half.x * 2.0 / PIECE).ceil().max(1.0);
        let along = (mist.half.z * 2.0 / PIECE).ceil().max(1.0);
        let size = Vec3::new(mist.half.x * 2.0 / across, mist.half.y * 2.0, mist.half.z * 2.0 / along);
        // The wind as the box sees it, in pieces a second.
        let wind = turn.inverse() * WIND / size;
        for i in 0..across as usize {
            for k in 0..along as usize {
                let at = Vec3::new((i as f32 + 0.5) * size.x - mist.half.x, 0.0, (k as f32 + 0.5) * size.z - mist.half.z);
                commands.spawn((
                    MapFog,
                    Drift(-wind.with_y(0.0)),
                    volume(mist.density * MIST, MIST_GLOW, &textures.mist),
                    Transform {
                        translation: mist.center + turn * at,
                        rotation: turn,
                        scale: size,
                    },
                ));
            }
        }
    }
    let downwind = WIND.normalize();
    for vent in &map.0.vents {
        for (bottom, top, width, density, rise, pushed) in PLUME {
            let size = Vec3::new(width, top - bottom, width) * vent.size;
            commands.spawn((
                MapFog,
                // Up, with the steam.
                Drift(-Vec3::Y * rise / size.y),
                volume(density / vent.size.max(0.5), STEAM_GLOW, &textures.plume),
                Transform {
                    translation: vent.pos + (Vec3::Y * (bottom + top) / 2.0 + downwind * pushed) * vent.size,
                    scale: size,
                    ..default()
                },
            ));
        }
    }
}

/// Fog is drawn, and lit, while the map has any and the settings ask for it:
/// by the sun (or the moon), and by the map's spot lamps, which then cast
/// shadows.
fn light_fog(
    mut commands: Commands,
    map: Res<GameMap>,
    settings: Res<Settings>,
    camera: Single<(Entity, Has<VolumetricFog>), With<MainCamera>>,
    sun: Single<(Entity, Has<VolumetricLight>), With<Sun>>,
    mut lamps: Query<(Entity, &mut SpotLight, Has<VolumetricLight>), With<MapLamp>>,
) {
    let foggy = !(map.0.mists.is_empty() && map.0.vents.is_empty());
    let wanted = foggy && settings.graphics.fog && settings.graphics.shadows != Shadows::Off;
    let (camera, drawn) = *camera;
    if wanted && !drawn {
        commands.entity(camera).insert(VolumetricFog {
            // Bevy's ambient light on fog shows each box's shape.
            ambient_intensity: 0.0,
            step_count: STEPS,
            ..default()
        });
    } else if !wanted && drawn {
        commands.entity(camera).remove::<VolumetricFog>();
    }
    let (sun, lit) = *sun;
    if wanted && !lit {
        commands.entity(sun).insert(VolumetricLight);
    } else if !wanted && lit {
        commands.entity(sun).remove::<VolumetricLight>();
    }
    for (lamp, mut light, lit) in &mut lamps {
        if light.shadow_maps_enabled != wanted {
            light.shadow_maps_enabled = wanted;
        }
        if wanted && !lit {
            commands.entity(lamp).insert(VolumetricLight);
        } else if !wanted && lit {
            commands.entity(lamp).remove::<VolumetricLight>();
        }
    }
}

fn drift(time: Res<Time>, mut volumes: Query<(&Drift, &mut FogVolume)>) {
    for (drift, mut volume) in &mut volumes {
        let moved = volume.density_texture_offset + drift.0 * time.delta_secs();
        volume.density_texture_offset = moved - moved.floor();
    }
}

// ---------------------------------------------------------------- the textures

/// A number from 0 to 1 for each corner of a grid that comes round again
/// every `period` corners.
fn corner(x: i32, y: i32, z: i32, period: i32, seed: u32) -> f32 {
    let wrap = |v: i32| v.rem_euclid(period) as u32;
    let mut h = wrap(x).wrapping_mul(0x8da6_b343) ^ wrap(y).wrapping_mul(0xd816_3841) ^ wrap(z).wrapping_mul(0xcb1a_b31f) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth noise from 0 to 1 at `at` (each part 0 to 1), which comes round
/// again at 1 every way: `period` lumps across.
fn lumps(at: Vec3, period: i32, seed: u32) -> f32 {
    let p = at * period as f32;
    let cell = p.floor();
    let f = p - cell;
    let f = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let (x, y, z) = (cell.x as i32, cell.y as i32, cell.z as i32);
    let c = |dx: i32, dy: i32, dz: i32| corner(x + dx, y + dy, z + dz, period, seed);
    let along_x = |dy: i32, dz: i32| c(0, dy, dz) + (c(1, dy, dz) - c(0, dy, dz)) * f.x;
    let along_y = |dz: i32| along_x(0, dz) + (along_x(1, dz) - along_x(0, dz)) * f.y;
    along_y(0) + (along_y(1) - along_y(0)) * f.z
}

/// Lumps on lumps: big ones with smaller ones on them.
fn billows(at: Vec3, seed: u32) -> f32 {
    lumps(at, 3, seed) * 0.5 + lumps(at, 6, seed + 1) * 0.3 + lumps(at, 12, seed + 2) * 0.2
}

fn smooth(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A texture of thicknesses, `size` texels each way, linearly filtered. It
/// comes round again along the ways named in `repeats` (x, y, z).
fn thickness(size: [u32; 3], repeats: [bool; 3], at: impl Fn(Vec3) -> f32) -> Image {
    let [w, h, d] = size;
    let mut data = Vec::with_capacity((w * h * d) as usize);
    for z in 0..d {
        for y in 0..h {
            for x in 0..w {
                let p = Vec3::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32, (z as f32 + 0.5) / d as f32);
                data.push((at(p).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: d,
        },
        TextureDimension::D3,
        data,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    let mode = |repeat: bool| if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode(repeats[0]),
        address_mode_v: mode(repeats[1]),
        address_mode_w: mode(repeats[2]),
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// A piece of mist: thickest at the ground, nothing at its top, in drifts
/// that carry on into the pieces beside it.
fn mist_texture() -> Image {
    thickness([64, 16, 64], [true, false, true], |p| {
        let drifts = 0.3 + 0.7 * smooth(0.3, 0.75, billows(Vec3::new(p.x, p.y * 0.4, p.z), 11));
        (1.0 - p.y).powf(1.5) * drifts
    })
}

/// A part of a plume: thick down its middle and gone at its sides, which
/// are ragged, in puffs that come round again up it.
fn plume_texture() -> Image {
    thickness([40, 40, 40], [false, true, false], |p| {
        // How far out its side is here: never as far as the box's.
        let side = 0.76 + (billows(p, 23) - 0.5) * 0.4;
        let out = Vec2::new(p.x - 0.5, p.z - 0.5).length() * 2.0 / side;
        let middle = (1.0 - out * out).max(0.0);
        middle * middle * (0.25 + 0.75 * smooth(0.3, 0.7, billows(p, 31)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The noise meets itself where it comes round, so a texture that moves
    /// shows no seam, and neither do two pieces of mist side by side.
    #[test]
    fn the_noise_comes_round() {
        for (a, b) in [(Vec3::new(0.0, 0.3, 0.7), Vec3::new(1.0, 0.3, 0.7)), (Vec3::new(0.2, 0.0, 0.5), Vec3::new(0.2, 1.0, 0.5))] {
            assert!((billows(a, 5) - billows(b, 5)).abs() < 1e-4);
        }
        let all: Vec<f32> = (0..500).map(|i| billows(Vec3::new(i as f32 * 0.37, i as f32 * 0.11, i as f32 * 0.73).fract(), 5)).collect();
        assert!(all.iter().all(|n| (0.0..=1.0).contains(n)));
        // Not all one grey.
        let (low, high) = all.iter().fold((1.0_f32, 0.0_f32), |(low, high), n| (low.min(*n), high.max(*n)));
        assert!(high - low > 0.4);
    }

    /// Mist is gone at its top, and a plume at its sides: no box shows.
    #[test]
    fn fog_ends_inside_its_box() {
        let texel = |image: &Image, size: u32, x: u32, y: u32, z: u32, row: u32| image.data.as_ref().unwrap()[(x + y * size + z * size * row) as usize];
        let mist = mist_texture();
        assert!((0..64).all(|x| texel(&mist, 64, x, 15, 20, 16) < 6), "mist at the top of its box");
        assert!((0..64).any(|x| texel(&mist, 64, x, 0, 20, 16) > 100), "no mist at the ground");
        let plume = plume_texture();
        assert!((0..40).all(|y| texel(&plume, 40, 0, y, 20, 40) == 0 && texel(&plume, 40, 20, y, 39, 40) == 0), "steam at the side of its box");
        assert!((0..40).any(|y| texel(&plume, 40, 20, y, 20, 40) > 120), "no steam up the middle");
    }
}
