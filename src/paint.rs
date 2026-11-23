//! Paint jobs: everybody paints their own body and head (see `paint_editor.rs`),
//! and everyone else sees it.
//!
//! A paint job is a `PAINT_SIZE` square texture of palette colours that wraps
//! the whole model: the body takes the top `BODY_ROWS` rows, the head the rest.
//! It is saved next to the settings, sent to the server after joining, and
//! passed on to every other player. Players who never painted (and bots) wear a
//! plain coat in the colour the server gave them.

use std::{collections::HashMap, path::PathBuf};

use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

pub use crate::canvas::{Canvas, PALETTE, PAINT_SIZE};
#[cfg(test)]
use crate::canvas::MAX_PAINT_BYTES;
use crate::{
    AppState,
    canvas::TEXELS,
    map::PLAYER_HALF,
    net::NetMsg,
    protocol::{PlayerId, ServerMsg},
};

/// The rows of the texture that wrap the body; the head gets the rest.
const BODY_ROWS: usize = 92;

// The model, measured from the centre of the standing collision box (feet at
// -0.9, top of the head at +0.9). Crouching squashes the body and lowers the head.
pub const BODY_RADIUS: f32 = 0.34;
pub const BODY_HEIGHT: f32 = 1.3;
pub const HEAD_RADIUS: f32 = 0.25;
/// The head sinks a little into the shoulders.
pub const NECK: f32 = HEAD_RADIUS * 0.8;
pub const HEAD_Y: f32 = BODY_HEIGHT - PLAYER_HALF.y + NECK;
pub const BODY_Y: f32 = BODY_HEIGHT / 2.0 - PLAYER_HALF.y;

/// The palette colour of a new player's first coat (orange).
pub const DEFAULT_COLOR: u8 = 6;
/// The colour of the trousers on a plain coat.
const TROUSERS: u8 = 9;
/// Below this height (from the middle of the standing box) a plain coat is trousers.
const TROUSERS_TOP: f32 = -0.33;

pub fn palette_color(index: u8) -> Color {
    let [r, g, b] = PALETTE[index as usize % PALETTE.len()];
    Color::srgb_u8(r, g, b)
}

impl Canvas {
    fn rgba(&self) -> Vec<u8> {
        self.0
            .iter()
            .flat_map(|&color| {
                let [r, g, b] = PALETTE[color as usize % PALETTE.len()];
                [r, g, b, 255]
            })
            .collect()
    }

    pub fn image(&self) -> Image {
        let mut image = Image::new(
            Extent3d {
                width: PAINT_SIZE as u32,
                height: PAINT_SIZE as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.rgba(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        // Crisp texels: it is painted texel by texel.
        image.sampler = ImageSampler::nearest();
        image
    }

    /// Puts this paint job into an image made by `image`.
    pub fn write_to(&self, image: &mut Image) {
        image.data = Some(self.rgba());
    }
}

// ---------------------------------------------------------------- the model

/// The player model's meshes, and where on it each texel of a paint job lies.
#[derive(Resource)]
pub struct PlayerModel {
    pub body: Handle<Mesh>,
    pub head: Handle<Mesh>,
    /// Per texel: its point on the model and the outward direction there,
    /// relative to the middle of the standing collision box. `None` for the
    /// few texels that are on no part of it.
    pub texels: Vec<Option<(Vec3, Vec3)>>,
}

impl PlayerModel {
    /// A plain coat: one colour, with dark trousers.
    pub fn plain(&self, color: u8) -> Canvas {
        Canvas(
            self.texels
                .iter()
                .map(|texel| match texel {
                    Some((point, _)) if point.y < TROUSERS_TOP => TROUSERS,
                    _ => color % PALETTE.len() as u8,
                })
                .collect(),
        )
    }

    /// Paints every texel within `radius` of `point`.
    pub fn dab(&self, canvas: &mut Canvas, point: Vec3, radius: f32, color: u8) -> bool {
        let mut changed = false;
        let reach = radius * radius;
        for (pixel, texel) in canvas.0.iter_mut().zip(&self.texels) {
            if let Some((at, _)) = texel
                && at.distance_squared(point) <= reach
                && *pixel != color
            {
                *pixel = color;
                changed = true;
            }
        }
        changed
    }

    /// The nearest texel the ray from `origin` along `dir` (normalized) runs
    /// into, on a part of the model that faces it.
    pub fn pick(&self, origin: Vec3, dir: Vec3) -> Option<usize> {
        const SLACK: f32 = 0.025;
        self.texels
            .iter()
            .enumerate()
            .filter_map(|(i, texel)| {
                let (point, normal) = (*texel)?;
                let along = (point - origin).dot(dir);
                let off = (point - origin - dir * along).length_squared();
                (along > 0.0 && off < SLACK * SLACK && normal.dot(dir) < 0.0).then_some((i, along))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

/// The body: a capsule, standing on the floor of the collision box. Its
/// texture coordinates are squeezed into the body's rows of the paint job.
fn body_mesh() -> Mesh {
    let mut mesh = Capsule3d::new(BODY_RADIUS, BODY_HEIGHT - 2.0 * BODY_RADIUS)
        .mesh()
        .longitudes(32)
        .latitudes(16)
        .build();
    squeeze_uvs(&mut mesh, 0.0, BODY_ROWS as f32 / PAINT_SIZE as f32);
    mesh
}

/// The head: a ball in the head's rows of the paint job.
fn head_mesh() -> Mesh {
    let mut mesh = Sphere::new(HEAD_RADIUS).mesh().uv(32, 18);
    squeeze_uvs(&mut mesh, BODY_ROWS as f32 / PAINT_SIZE as f32, 1.0);
    mesh
}

fn squeeze_uvs(mesh: &mut Mesh, top: f32, bottom: f32) {
    if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for uv in uvs {
            uv[1] = top + uv[1].clamp(0.0, 1.0) * (bottom - top);
        }
    }
}

/// Finds the point on the model under every texel, by drawing each triangle
/// of each mesh into the texture.
fn map_texels(parts: &[(&Mesh, Vec3)]) -> Vec<Option<(Vec3, Vec3)>> {
    let mut texels = vec![None; TEXELS];
    for (mesh, offset) in parts {
        let (
            Some(VertexAttributeValues::Float32x3(points)),
            Some(VertexAttributeValues::Float32x3(normals)),
            Some(VertexAttributeValues::Float32x2(uvs)),
            Some(indices),
        ) = (
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            mesh.attribute(Mesh::ATTRIBUTE_UV_0),
            mesh.indices(),
        )
        else {
            continue;
        };
        let corners: Vec<usize> = match indices {
            Indices::U16(_) | Indices::U32(_) => indices.iter().collect(),
        };
        for triangle in corners.chunks_exact(3) {
            let at = |i: usize| Vec2::from(uvs[triangle[i]]) * PAINT_SIZE as f32;
            let (a, b, c) = (at(0), at(1), at(2));
            let area = (b - a).perp_dot(c - a);
            if area.abs() < 1e-6 {
                continue;
            }
            let min = a.min(b).min(c).floor().max(Vec2::ZERO);
            let max = a.max(b).max(c).ceil().min(Vec2::splat(PAINT_SIZE as f32));
            for y in min.y as usize..max.y as usize {
                for x in min.x as usize..max.x as usize {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w = Vec3::new((c - b).perp_dot(p - b), (a - c).perp_dot(p - c), (b - a).perp_dot(p - a)) / area;
                    if w.min_element() < -1e-4 {
                        continue;
                    }
                    let blend3 = |values: &Vec<[f32; 3]>| {
                        Vec3::from(values[triangle[0]]) * w.x
                            + Vec3::from(values[triangle[1]]) * w.y
                            + Vec3::from(values[triangle[2]]) * w.z
                    };
                    texels[y * PAINT_SIZE + x] = Some((blend3(points) + *offset, blend3(normals).normalize_or_zero()));
                }
            }
        }
    }
    // Texels just outside the triangles still show at their edges: give them
    // the point of a neighbour.
    for _ in 0..3 {
        let before = texels.clone();
        for y in 0..PAINT_SIZE {
            for x in 0..PAINT_SIZE {
                if before[y * PAINT_SIZE + x].is_some() {
                    continue;
                }
                let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                texels[y * PAINT_SIZE + x] = neighbours
                    .into_iter()
                    .filter(|(nx, ny)| *nx < PAINT_SIZE && *ny < PAINT_SIZE)
                    .find_map(|(nx, ny)| before[ny * PAINT_SIZE + nx]);
            }
        }
    }
    texels
}

// ---------------------------------------------------------------- skins

/// What other players look like: one texture and material per player.
#[derive(Resource, Default)]
pub struct Skins(HashMap<PlayerId, Skin>);

pub struct Skin {
    pub material: Handle<StandardMaterial>,
    image: Handle<Image>,
    /// Their most used colour.
    pub main: u8,
    /// They sent a paint job (otherwise they wear a plain coat).
    painted: bool,
}

impl Skins {
    /// The material of player `id`, made on first use: a plain coat in
    /// `color` until their paint job arrives.
    pub fn material(
        &mut self,
        id: PlayerId,
        color: u8,
        model: &PlayerModel,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        self.0
            .entry(id)
            .or_insert_with(|| new_skin(&model.plain(color), false, images, materials))
            .material
            .clone()
    }

    pub fn main_color(&self, id: PlayerId) -> Option<u8> {
        self.0.get(&id).filter(|skin| skin.painted).map(|skin| skin.main)
    }

    fn set(
        &mut self,
        id: PlayerId,
        canvas: &Canvas,
        images: &mut Assets<Image>,
        materials: &mut Assets<StandardMaterial>,
    ) {
        match self.0.get_mut(&id) {
            Some(skin) => {
                if let Some(mut image) = images.get_mut(&skin.image) {
                    canvas.write_to(&mut image);
                }
                // So the material picks up the new texture.
                let _ = materials.get_mut(&skin.material);
                skin.main = canvas.main_color();
                skin.painted = true;
            }
            None => {
                self.0.insert(id, new_skin(canvas, true, images, materials));
            }
        }
    }
}

pub fn skin_material(image: Handle<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(image),
        perceptual_roughness: 0.6,
        ..default()
    }
}

fn new_skin(
    canvas: &Canvas,
    painted: bool,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Skin {
    let image = images.add(canvas.image());
    Skin {
        material: materials.add(skin_material(image.clone())),
        image,
        main: canvas.main_color(),
        painted,
    }
}

// ---------------------------------------------------------------- our own paint job

/// Our paint job. `version` goes up with every change, so the connection
/// knows when to send it again.
#[derive(Resource)]
pub struct MyPaint {
    pub canvas: Canvas,
    pub version: u32,
    /// False during the self-test, which must not touch the player's file.
    persist: bool,
}

impl MyPaint {
    pub fn set(&mut self, canvas: Canvas) {
        if canvas != self.canvas {
            self.canvas = canvas;
            self.version += 1;
            self.save();
        }
    }

    fn save(&self) {
        let Some(path) = paint_path().filter(|_| self.persist) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(err) = std::fs::write(&path, self.canvas.encode()) {
            warn!("could not save the paint job to {}: {err}", path.display());
        }
    }
}

/// `paint.bin`, beside the settings file.
fn paint_path() -> Option<PathBuf> {
    crate::settings::settings_path().map(|path| path.with_file_name("paint.bin"))
}

pub struct PaintPlugin;

impl Plugin for PaintPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Skins>()
            .add_systems(PreStartup, setup)
            .add_systems(OnEnter(AppState::Menu), |mut skins: ResMut<Skins>| skins.0.clear())
            .add_systems(Update, receive);
    }
}

fn setup(
    mut commands: Commands,
    args: Res<crate::LaunchArgs>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (body, head) = (body_mesh(), head_mesh());
    let texels = map_texels(&[(&body, Vec3::Y * BODY_Y), (&head, Vec3::Y * HEAD_Y)]);
    let model = PlayerModel {
        body: meshes.add(body),
        head: meshes.add(head),
        texels,
    };
    let persist = args.selftest.is_none();
    let saved = paint_path()
        .filter(|_| persist)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| Canvas::decode(&bytes));
    commands.insert_resource(MyPaint {
        canvas: saved.unwrap_or_else(|| model.plain(DEFAULT_COLOR)),
        version: 0,
        persist,
    });
    commands.insert_resource(model);
}

/// Paint jobs from the server.
fn receive(
    mut messages: MessageReader<NetMsg>,
    mut skins: ResMut<Skins>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for NetMsg(msg) in messages.read() {
        if let ServerMsg::Paint { id, paint } = msg
            && let Some(canvas) = Canvas::decode(paint)
        {
            skins.set(*id, &canvas, &mut images, &mut materials);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> PlayerModel {
        let (body, head) = (body_mesh(), head_mesh());
        PlayerModel {
            body: Handle::default(),
            head: Handle::default(),
            texels: map_texels(&[(&body, Vec3::Y * BODY_Y), (&head, Vec3::Y * HEAD_Y)]),
        }
    }

    #[test]
    fn a_paint_job_survives_encoding() {
        let mut canvas = Canvas::solid(3);
        for (i, pixel) in canvas.0.iter_mut().enumerate() {
            if i % 7 == 0 || (5000..9000).contains(&i) {
                *pixel = (i % 32) as u8;
            }
        }
        let bytes = canvas.encode();
        assert!(bytes.len() <= MAX_PAINT_BYTES);
        assert_eq!(Canvas::decode(&bytes), Some(canvas));
        // A plain coat is tiny.
        assert!(Canvas::solid(1).encode().len() < 200);
    }

    #[test]
    fn broken_paint_jobs_are_refused() {
        let bytes = Canvas::solid(2).encode();
        assert_eq!(Canvas::decode(&bytes[..bytes.len() - 2]), None, "too short");
        assert_eq!(Canvas::decode(&[bytes.clone(), vec![0, 1]].concat()), None, "too long");
        let mut bad_color = bytes.clone();
        bad_color[2] = 200;
        assert_eq!(Canvas::decode(&bad_color), None, "no such colour");
        assert_eq!(Canvas::decode(&[]), None);
    }

    #[test]
    fn every_texel_lies_on_the_model() {
        let model = model();
        let mapped = model.texels.iter().filter(|t| t.is_some()).count();
        assert!(mapped > TEXELS * 99 / 100, "only {mapped} of {TEXELS} texels mapped");
        for (point, normal) in model.texels.iter().flatten() {
            assert!(point.y >= -PLAYER_HALF.y - 0.01 && point.y <= HEAD_Y + HEAD_RADIUS + 0.01, "{point}");
            assert!((normal.length() - 1.0).abs() < 0.01);
        }
    }

    #[test]
    fn painting_where_the_mouse_points_paints_there() {
        let model = model();
        let mut canvas = model.plain(0);
        // Straight at the chest from in front (the model faces -z).
        let hit = model.pick(Vec3::new(0.0, 0.1, -3.0), Vec3::Z).expect("the chest is in the way");
        let (point, normal) = model.texels[hit].unwrap();
        assert!((point.z + BODY_RADIUS).abs() < 0.03, "{point}");
        assert!(normal.z < -0.9);
        assert!(model.dab(&mut canvas, point, 0.05, 5));
        assert_eq!(canvas.0[hit], 5);
        // Nothing on the back changed.
        for (pixel, texel) in canvas.0.iter().zip(&model.texels) {
            if let Some((at, _)) = texel
                && at.z > 0.0
            {
                assert_ne!(*pixel, 5);
            }
        }
        // Missing the model picks nothing.
        assert_eq!(model.pick(Vec3::new(2.0, 0.0, -3.0), Vec3::Z), None);
    }
}
