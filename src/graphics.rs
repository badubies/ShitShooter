//! Graphics settings: anti-aliasing, shadows, ambient occlusion, bloom,
//! brightness, texture filtering, render scale, vertical sync and a frame
//! rate limit. They are part of `Settings` (saved with the rest, edited on
//! the settings panel's Graphics page), and this module puts them on the
//! main camera, the sun and the window whenever they change.
//!
//! The defaults are how the game looked before there were any: MSAA 4x, the
//! sun's shadows at 2048 texels over four cascades, no bloom, textures
//! filtered 4x, the 3D view drawn at the window's own resolution, vertical
//! sync on and no limit.
//!
//! At a render scale other than 100% the main camera draws the 3D view
//! into an image that much smaller (or bigger) than the screen, and a
//! second camera, `ScreenCamera`, shows it stretched over the whole screen
//! behind the HUD and menus, which it draws at full resolution. The image
//! has the screen's shape, so whatever is placed over the 3D view from the
//! camera's field of view and the window's size (pings, flags, names)
//! still lines up.

use std::{
    sync::atomic::{AtomicU16, Ordering},
    time::{Duration, Instant},
};

use bevy::{
    anti_alias::{contrast_adaptive_sharpening::ContrastAdaptiveSharpening, fxaa::Fxaa, smaa::Smaa},
    camera::{Hdr, ImageRenderTarget, RenderTarget},
    core_pipeline::prepass::{DepthPrepass, NormalPrepass},
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap},
    pbr::ScreenSpaceAmbientOcclusion,
    post_process::bloom::Bloom,
    prelude::*,
    render::{
        render_resource::TextureFormat,
        view::{ColorGrading, Msaa},
    },
    window::{PresentMode, PrimaryWindow, WindowRef},
};

use crate::{
    settings::Settings,
    world::{MainCamera, Sun},
};

pub struct GraphicsPlugin;

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirectionalLightShadowMap>()
            .init_resource::<ScreenTarget>()
            .add_systems(Update, (apply_graphics.run_if(resource_changed::<Settings>), route_picture))
            // After everything else in the frame, so the wait is the frame's last.
            .add_systems(Last, limit_frame_rate);
    }
}

// ---------------------------------------------------------------- the settings

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AntiAlias {
    Off,
    /// Fast, a little soft.
    Fxaa,
    /// Sharper than FXAA, a little dearer.
    Smaa,
    /// Smooths geometry edges best, but not the inside of textures, and does
    /// not go with ambient occlusion.
    Msaa4,
}

impl AntiAlias {
    pub const ALL: [AntiAlias; 4] = [AntiAlias::Off, AntiAlias::Fxaa, AntiAlias::Smaa, AntiAlias::Msaa4];

    /// (name on the panel, name in the settings file)
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            AntiAlias::Off => ("Off", "off"),
            AntiAlias::Fxaa => ("FXAA", "fxaa"),
            AntiAlias::Smaa => ("SMAA", "smaa"),
            AntiAlias::Msaa4 => ("MSAA 4x", "msaa4"),
        }
    }
}

/// How sharp the sun's shadows are and how far they reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shadows {
    Off,
    Low,
    Medium,
    High,
    Ultra,
}

impl Shadows {
    pub const ALL: [Shadows; 5] = [Shadows::Off, Shadows::Low, Shadows::Medium, Shadows::High, Shadows::Ultra];

    /// (name on the panel, name in the settings file)
    pub fn names(self) -> (&'static str, &'static str) {
        match self {
            Shadows::Off => ("Off", "off"),
            Shadows::Low => ("Low", "low"),
            Shadows::Medium => ("Medium", "medium"),
            Shadows::High => ("High", "high"),
            Shadows::Ultra => ("Ultra", "ultra"),
        }
    }

    /// (texels across each cascade's map, cascades, the first one's far
    /// edge, how far the last one reaches), in metres. `None` for no shadows.
    fn detail(self) -> Option<(usize, usize, f32, f32)> {
        match self {
            Shadows::Off => None,
            Shadows::Low => Some((1024, 2, 10.0, 70.0)),
            Shadows::Medium => Some((2048, 3, 12.0, 140.0)),
            // Far enough for the outdoor maps.
            Shadows::High => Some((2048, 4, 14.0, 220.0)),
            Shadows::Ultra => Some((4096, 4, 14.0, 260.0)),
        }
    }
}

/// How much textures are filtered when seen at a slant (1 is not at all).
/// Images are made with it once, so a change shows after a restart.
pub const TEXTURE_FILTERING: [u16; 5] = [1, 2, 4, 8, 16];

/// The frame rate limits offered on the panel; 0 is none. The file takes any.
pub const FRAME_LIMITS: [u32; 7] = [0, 30, 60, 120, 144, 165, 240];

/// The lowest and highest limit read from the file.
const FRAME_LIMIT_RANGE: (u32, u32) = (20, 1000);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Graphics {
    pub anti_alias: AntiAlias,
    pub shadows: Shadows,
    /// Screen-space ambient occlusion: shade where surfaces meet.
    pub ambient_occlusion: bool,
    /// A glow round bright lights: tracers, blasts, the jump pads.
    pub bloom: bool,
    /// Mist and steam on the maps that have them (`fog.rs`). They need the
    /// shadows: with those off there is none either.
    pub fog: bool,
    /// One of `TEXTURE_FILTERING`.
    pub texture_filtering: u16,
    /// How bright the 3D view is, 1 as designed. The HUD and menus keep theirs.
    pub brightness: f32,
    /// The 3D view's resolution, as a share of the screen's along each side:
    /// below 1 for speed, above for a sharper picture. The HUD and menus are
    /// always drawn at the screen's.
    pub render_scale: f32,
    pub vsync: bool,
    /// Frames a second at most; 0 for no limit.
    pub max_fps: u32,
    /// The frame rate and ping box in the top right corner while playing.
    pub show_stats: bool,
}

impl Default for Graphics {
    fn default() -> Self {
        Graphics {
            anti_alias: AntiAlias::Msaa4,
            shadows: Shadows::High,
            ambient_occlusion: false,
            bloom: false,
            fog: true,
            texture_filtering: 4,
            brightness: 1.0,
            render_scale: 1.0,
            vsync: true,
            max_fps: 0,
            show_stats: true,
        }
    }
}

impl Graphics {
    /// MSAA and ambient occlusion do not go together: picking MSAA turns
    /// ambient occlusion off.
    pub fn set_anti_alias(&mut self, anti_alias: AntiAlias) {
        self.anti_alias = anti_alias;
        if anti_alias == AntiAlias::Msaa4 {
            self.ambient_occlusion = false;
        }
    }

    /// Turning ambient occlusion on swaps MSAA for SMAA.
    pub fn set_ambient_occlusion(&mut self, on: bool) {
        self.ambient_occlusion = on;
        if on && self.anti_alias == AntiAlias::Msaa4 {
            self.anti_alias = AntiAlias::Smaa;
        }
    }

    /// Reads one `key = value` line of the settings file, if it is one of
    /// these. Anything unreadable keeps what it was.
    pub fn read(&mut self, key: &str, value: &str) -> bool {
        let flag = value.parse::<bool>().ok();
        match key {
            "anti_aliasing" => {
                if let Some(picked) = AntiAlias::ALL.into_iter().find(|a| a.names().1.eq_ignore_ascii_case(value)) {
                    self.anti_alias = picked;
                }
            }
            "shadows" => {
                if let Some(picked) = Shadows::ALL.into_iter().find(|s| s.names().1.eq_ignore_ascii_case(value)) {
                    self.shadows = picked;
                }
            }
            "ambient_occlusion" => self.ambient_occlusion = flag.unwrap_or(self.ambient_occlusion),
            "bloom" => self.bloom = flag.unwrap_or(self.bloom),
            "fog" => self.fog = flag.unwrap_or(self.fog),
            "texture_filtering" => {
                if let Some(n) = value.parse().ok().filter(|n| TEXTURE_FILTERING.contains(n)) {
                    self.texture_filtering = n;
                }
            }
            "brightness" => self.brightness = value.parse().unwrap_or(self.brightness),
            "render_scale" => self.render_scale = value.parse().unwrap_or(self.render_scale),
            "vsync" => self.vsync = flag.unwrap_or(self.vsync),
            "max_fps" => {
                if let Ok(n) = value.parse::<u32>() {
                    let (low, high) = FRAME_LIMIT_RANGE;
                    self.max_fps = if n == 0 { 0 } else { n.clamp(low, high) };
                }
            }
            "show_stats" => self.show_stats = flag.unwrap_or(self.show_stats),
            _ => return false,
        }
        true
    }

    /// Settles what a hand-edited file could leave at odds: with ambient
    /// occlusion asked for, MSAA gives way.
    pub fn tidy(&mut self) {
        self.set_ambient_occlusion(self.ambient_occlusion);
    }

    pub fn write(&self, text: &mut String) {
        text.push_str(&format!(
            "anti_aliasing = {}\nshadows = {}\nambient_occlusion = {}\nbloom = {}\nfog = {}\ntexture_filtering = {}\nbrightness = {}\nrender_scale = {}\nvsync = {}\nmax_fps = {}\nshow_stats = {}\n",
            self.anti_alias.names().1,
            self.shadows.names().1,
            self.ambient_occlusion,
            self.bloom,
            self.fog,
            self.texture_filtering,
            self.brightness,
            self.render_scale,
            self.vsync,
            self.max_fps,
            self.show_stats,
        ));
    }
}

/// A set of the settings that cost the most, picked together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    Low,
    Medium,
    High,
    Ultra,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Low, Preset::Medium, Preset::High, Preset::Ultra];

    pub fn name(self) -> &'static str {
        match self {
            Preset::Low => "Low",
            Preset::Medium => "Medium",
            Preset::High => "High",
            Preset::Ultra => "Ultra",
        }
    }

    /// (anti-aliasing, shadows, ambient occlusion, texture filtering). High
    /// is the defaults.
    fn picks(self) -> (AntiAlias, Shadows, bool, u16) {
        match self {
            Preset::Low => (AntiAlias::Fxaa, Shadows::Low, false, 1),
            Preset::Medium => (AntiAlias::Smaa, Shadows::Medium, false, 4),
            Preset::High => (AntiAlias::Msaa4, Shadows::High, false, 4),
            Preset::Ultra => (AntiAlias::Smaa, Shadows::Ultra, true, 8),
        }
    }

    pub fn apply(self, graphics: &mut Graphics) {
        let (anti_alias, shadows, ambient_occlusion, texture_filtering) = self.picks();
        graphics.anti_alias = anti_alias;
        graphics.shadows = shadows;
        graphics.ambient_occlusion = ambient_occlusion;
        graphics.texture_filtering = texture_filtering;
    }

    /// Whether the settings are this preset's, whatever the others are.
    pub fn matches(self, graphics: &Graphics) -> bool {
        let picks = (graphics.anti_alias, graphics.shadows, graphics.ambient_occlusion, graphics.texture_filtering);
        picks == self.picks()
    }
}

// ---------------------------------------------------------------- texture filtering

/// The filtering the textures are made with this run: the saved setting
/// when the game started (`SettingsPlugin` sets it), since images are made
/// with it and not changed after.
static TEXTURE_FILTERING_IN_USE: AtomicU16 = AtomicU16::new(4);

pub fn set_texture_filtering_in_use(n: u16) {
    TEXTURE_FILTERING_IN_USE.store(n, Ordering::Relaxed);
}

/// What `ImageSamplerDescriptor::anisotropy_clamp` is for the map and gun textures.
pub fn texture_filtering_in_use() -> u16 {
    TEXTURE_FILTERING_IN_USE.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------- putting them on

/// Puts the settings on the camera, the sun and the window: only what
/// changed since the last time, so dragging another setting's slider does
/// not rebuild the render pipelines.
fn apply_graphics(
    mut commands: Commands,
    settings: Res<Settings>,
    mut last: Local<Option<Graphics>>,
    camera: Single<(Entity, Option<&mut ColorGrading>), With<MainCamera>>,
    sun: Single<(Entity, &mut DirectionalLight), With<Sun>>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let wanted = settings.graphics;
    let before = *last;
    if before == Some(wanted) {
        return;
    }
    *last = Some(wanted);
    let changed = |field: fn(&Graphics) -> u32| before.is_none_or(|before| field(&before) != field(&wanted));
    let (camera, grading) = camera.into_inner();
    let mut camera = commands.entity(camera);

    if changed(|g| g.anti_alias as u32) || changed(|g| g.ambient_occlusion as u32) {
        // Ambient occlusion needs MSAA off (`Graphics` never has both).
        let msaa = if wanted.anti_alias == AntiAlias::Msaa4 && !wanted.ambient_occlusion {
            Msaa::Sample4
        } else {
            Msaa::Off
        };
        camera.insert(msaa);
        camera.remove::<(Fxaa, Smaa)>();
        match wanted.anti_alias {
            AntiAlias::Fxaa => {
                camera.insert(Fxaa::default());
            }
            AntiAlias::Smaa => {
                camera.insert(Smaa::default());
            }
            AntiAlias::Off | AntiAlias::Msaa4 => {}
        }
        if wanted.ambient_occlusion {
            // Brings the depth and normal prepasses it needs.
            camera.insert(ScreenSpaceAmbientOcclusion::default());
        } else {
            camera.remove::<(ScreenSpaceAmbientOcclusion, DepthPrepass, NormalPrepass)>();
        }
    }

    if changed(|g| g.bloom as u32) {
        if wanted.bloom {
            // Brings HDR, which it needs.
            camera.insert(Bloom::NATURAL);
        } else {
            camera.remove::<(Bloom, Hdr)>();
        }
    }

    if changed(|g| g.brightness.to_bits()) {
        // As exposure: each doubling is one stop brighter.
        let exposure = wanted.brightness.max(0.01).log2();
        match grading {
            Some(mut grading) => grading.global.exposure = exposure,
            None => {
                let mut grading = ColorGrading::default();
                grading.global.exposure = exposure;
                camera.insert(grading);
            }
        }
    }

    if changed(|g| g.shadows as u32) {
        let (sun, mut light) = sun.into_inner();
        match wanted.shadows.detail() {
            Some((size, num_cascades, first_cascade_far_bound, maximum_distance)) => {
                light.shadow_maps_enabled = true;
                shadow_map.size = size;
                commands.entity(sun).insert(
                    CascadeShadowConfigBuilder {
                        num_cascades,
                        first_cascade_far_bound,
                        maximum_distance,
                        ..default()
                    }
                    .build(),
                );
            }
            None => light.shadow_maps_enabled = false,
        }
    }

    if changed(|g| g.vsync as u32) {
        // FIFO is vertical sync everywhere; the other picks the fastest the
        // system has without it (mailbox or immediate).
        window.present_mode = if wanted.vsync { PresentMode::Fifo } else { PresentMode::AutoNoVsync };
    }
}

// ---------------------------------------------------------------- render scale

/// Where the finished picture goes: the window, or the image the self-test
/// takes its pictures from (`devtools.rs` sets it).
#[derive(Resource, Default)]
pub struct ScreenTarget(pub Option<ImageRenderTarget>);

impl ScreenTarget {
    fn render_target(&self) -> RenderTarget {
        match &self.0 {
            Some(image) => RenderTarget::Image(image.clone()),
            None => RenderTarget::Window(WindowRef::Primary),
        }
    }
}

/// Draws the HUD and menus, and the 3D view stretched under them, when the
/// 3D view is drawn at another resolution than the screen's.
#[derive(Component)]
struct ScreenCamera;

/// The 3D view as the screen camera shows it: an image over the whole
/// screen, under everything else.
#[derive(Component)]
struct Backdrop;

/// The biggest image the 3D view is drawn into, along either side: the
/// largest texture every graphics card takes.
const MAX_RENDER_SIZE: f32 = 8192.0;

/// How strongly a picture drawn smaller than the screen is sharpened, to
/// win back some of what stretching it softens.
const UPSCALE_SHARPENING: f32 = 0.5;

/// The size the 3D view is drawn at for a screen of `screen` pixels, kept
/// to the screen's shape.
pub fn render_size(screen: UVec2, scale: f32) -> UVec2 {
    let wanted = screen.as_vec2() * scale;
    let fit = (MAX_RENDER_SIZE / wanted.max_element()).min(1.0);
    (wanted * fit).round().as_uvec2().max(UVec2::ONE)
}

/// What `route_picture` last set up: the render scale, the screen's size and
/// scale factor, and where the picture went.
#[derive(PartialEq)]
struct Route {
    scale: u32,
    screen: UVec2,
    scale_factor: u32,
    target: Option<AssetId<Image>>,
}

/// Points the main camera at the screen, or at an image of the render
/// scale's size that the screen camera shows, whenever the scale, the
/// window's size or the screen target changes.
fn route_picture(
    mut commands: Commands,
    settings: Res<Settings>,
    target: Res<ScreenTarget>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    mut last: Local<Option<Route>>,
    camera: Single<Entity, With<MainCamera>>,
    screen_cameras: Query<Entity, With<ScreenCamera>>,
    mut backdrops: Query<(Entity, &mut ImageNode, &mut Node), With<Backdrop>>,
) {
    // Within half a step of 100% is 100%: the slider's steps are 5%.
    let mut scale = settings.graphics.render_scale;
    if (scale - 1.0).abs() < 0.025 {
        scale = 1.0;
    }
    let screen = window.physical_size();
    // Minimised: nothing to draw, and nothing to size an image by.
    if screen.min_element() == 0 {
        return;
    }
    let route = Route {
        scale: scale.to_bits(),
        screen,
        scale_factor: window.scale_factor().to_bits(),
        target: target.0.as_ref().map(|image| image.handle.id()),
    };
    if last.as_ref() == Some(&route) {
        return;
    }
    *last = Some(route);

    if scale == 1.0 {
        // Straight to the screen, HUD and all, as without a render scale.
        commands
            .entity(*camera)
            .insert((target.render_target(), IsDefaultUiCamera))
            .remove::<ContrastAdaptiveSharpening>();
        for screen_camera in &screen_cameras {
            commands.entity(screen_camera).despawn();
        }
        for (backdrop, ..) in &backdrops {
            commands.entity(backdrop).despawn();
        }
        return;
    }

    let size = render_size(screen, scale);
    // The format the loadout's pictures use too: stored plainly, drawn into as sRGB.
    let image = images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));
    let picture = ImageRenderTarget {
        handle: image.clone(),
        // So the camera's size in logical pixels is the window's.
        scale_factor: window.scale_factor() * size.x as f32 / screen.x as f32,
    };
    let mut main = commands.entity(*camera);
    main.insert(RenderTarget::Image(picture)).remove::<IsDefaultUiCamera>();
    if scale < 1.0 {
        main.insert(ContrastAdaptiveSharpening {
            sharpening_strength: UPSCALE_SHARPENING,
            ..default()
        });
    } else {
        main.remove::<ContrastAdaptiveSharpening>();
    }

    match screen_cameras.iter().next() {
        Some(screen_camera) => {
            commands.entity(screen_camera).insert(target.render_target());
        }
        None => {
            commands.spawn((
                ScreenCamera,
                Camera2d,
                // After the main camera, whose picture it shows.
                Camera {
                    order: 1,
                    ..default()
                },
                target.render_target(),
                IsDefaultUiCamera,
            ));
        }
    }
    match backdrops.iter_mut().next() {
        // The old image goes once nothing holds it.
        Some((_, mut node_image, _)) => node_image.image = image,
        None => {
            commands.spawn((
                Backdrop,
                ImageNode::new(image),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                GlobalZIndex(i32::MIN),
                Pickable::IGNORE,
            ));
        }
    }
}

/// Holds each frame back until its turn when there is a limit. It sleeps
/// for most of the wait and spins for the last moment, as a sleep can
/// overrun by a millisecond or so.
fn limit_frame_rate(settings: Res<Settings>, mut next: Local<Option<Instant>>) {
    let fps = settings.graphics.max_fps;
    if fps == 0 {
        *next = None;
        return;
    }
    let frame = Duration::from_secs_f64(1.0 / f64::from(fps));
    let now = Instant::now();
    // Far behind (a load, a stall): start again from now rather than rush
    // to catch up.
    let due = next.filter(|due| now < *due + frame).unwrap_or(now);
    if due > now {
        let wait = due - now;
        let spin = Duration::from_micros(1500);
        if wait > spin {
            std::thread::sleep(wait - spin);
        }
        while Instant::now() < due {
            std::hint::spin_loop();
        }
    }
    *next = Some(due + frame);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphics_survive_a_save_and_load() {
        let graphics = Graphics {
            anti_alias: AntiAlias::Smaa,
            shadows: Shadows::Ultra,
            ambient_occlusion: true,
            bloom: true,
            fog: false,
            texture_filtering: 16,
            brightness: 1.35,
            render_scale: 0.75,
            vsync: false,
            max_fps: 144,
            show_stats: false,
        };
        let mut text = String::new();
        graphics.write(&mut text);
        let mut read = Graphics::default();
        for line in text.lines() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(read.read(key.trim(), value.trim()), "{line}");
        }
        assert_eq!(read, graphics);
    }

    #[test]
    fn msaa_and_ambient_occlusion_never_go_together() {
        let mut graphics = Graphics::default();
        assert_eq!(graphics.anti_alias, AntiAlias::Msaa4);
        graphics.set_ambient_occlusion(true);
        assert_eq!(graphics.anti_alias, AntiAlias::Smaa);
        graphics.set_anti_alias(AntiAlias::Msaa4);
        assert!(!graphics.ambient_occlusion);
        // A file that asks for both keeps the ambient occlusion.
        let mut read = Graphics::default();
        read.read("ambient_occlusion", "true");
        read.tidy();
        assert_eq!((read.anti_alias, read.ambient_occlusion), (AntiAlias::Smaa, true));
        for preset in Preset::ALL {
            let mut graphics = Graphics::default();
            preset.apply(&mut graphics);
            assert!(!(graphics.ambient_occlusion && graphics.anti_alias == AntiAlias::Msaa4), "{preset:?}");
            assert!(preset.matches(&graphics));
        }
    }

    #[test]
    fn the_picture_keeps_the_screen_shape_within_the_size_limit() {
        assert_eq!(render_size(UVec2::new(2560, 1440), 1.0), UVec2::new(2560, 1440));
        assert_eq!(render_size(UVec2::new(2560, 1440), 0.75), UVec2::new(1920, 1080));
        assert_eq!(render_size(UVec2::new(2560, 1440), 0.5), UVec2::new(1280, 720));
        // A 5K screen at 200% would be 10240 wide: held to 8192, same shape.
        assert_eq!(render_size(UVec2::new(5120, 2880), 2.0), UVec2::new(8192, 4608));
        assert_eq!(render_size(UVec2::new(1, 1), 0.5), UVec2::new(1, 1));
    }

    #[test]
    fn the_defaults_are_the_high_preset_and_a_bad_file_keeps_them() {
        assert!(Preset::High.matches(&Graphics::default()));
        let mut read = Graphics::default();
        for (key, value) in [
            ("anti_aliasing", "txaa"),
            ("shadows", "max"),
            ("texture_filtering", "3"),
            ("max_fps", "lots"),
            ("vsync", "maybe"),
        ] {
            read.read(key, value);
        }
        assert_eq!(read, Graphics::default());
        read.read("max_fps", "5");
        assert_eq!(read.max_fps, FRAME_LIMIT_RANGE.0);
        read.read("max_fps", "0");
        assert_eq!(read.max_fps, 0);
    }
}
