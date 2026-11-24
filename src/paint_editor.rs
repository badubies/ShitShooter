//! The paint screen: your model, turning in front of you, that you paint on
//! with the mouse. It opens from the main menu and the lobby; what you paint
//! is saved and everyone else sees it (see `paint.rs`).
//!
//! The model is drawn by a camera of its own into a texture that the panel
//! shows, on a render layer the main camera does not see.

use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    input_focus::InputFocus,
    prelude::*,
    render::render_resource::TextureFormat,
    picking::hover::Hovered,
    ui_widgets::{
        Activate, Button, Slider, SliderDragState, SliderRange, SliderStep, SliderThumb, SliderValue, TrackClick,
        ValueChange, slider_self_update,
    },
    window::PrimaryWindow,
};

use crate::{
    AppState,
    paint::{BODY_Y, Canvas, DEFAULT_COLOR, HEAD_Y, MyPaint, PALETTE, PlayerModel, palette_color, skin_material},
    ui::{self, ACCENT, ButtonKind, DIM, TEXT},
    world::SharedAssets,
};

/// The render layer only the paint screen's camera sees.
const LAYER: usize = 3;
/// Pixels of the texture the model is drawn into; it is shown at half this
/// size, so it stays sharp on high-DPI screens.
const VIEW_SIZE: UVec2 = UVec2::new(760, 1000);
const VIEW_FOV: f32 = 0.55;
/// What the camera looks at: about the middle of the model.
const VIEW_TARGET: Vec3 = Vec3::new(0.0, 0.02, 0.0);
const START_DISTANCE: f32 = 3.4;
const DISTANCES: (f32, f32) = (1.3, 4.2);
/// The brush's radius in centimetres: smallest, largest, and to start with.
const BRUSH_SIZES: (f32, f32) = (1.5, 15.0);
const START_BRUSH: f32 = 5.0;
/// Width of the brush slider, and of its round thumb.
const SLIDER_WIDTH: f32 = 340.0;
const THUMB: f32 = 16.0;
const MAX_UNDO: usize = 40;
const SWATCH: f32 = 44.0;
/// Between the colours.
const SWATCH_GAP: f32 = 8.0;
const SWATCHES_PER_ROW: usize = 8;

pub struct PaintEditorPlugin;

impl Plugin for PaintEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PaintEditor>()
            // A match starting closes it (and keeps what was painted).
            .add_systems(OnEnter(AppState::Playing), |mut editor: ResMut<PaintEditor>| editor.open = false)
            .add_systems(
                Update,
                (
                    escape_key,
                    sync_editor,
                    (edit, show_changes).chain().run_if(resource_exists::<Editing>),
                )
                    .chain(),
            );
    }
}

/// Whether the paint screen is open.
#[derive(Resource, Default)]
pub struct PaintEditor {
    pub open: bool,
}

/// Observer for any button that opens the paint screen.
pub fn on_open_paint(_: On<Activate>, mut editor: ResMut<PaintEditor>) {
    editor.open = true;
}

/// Everything about the paint job being worked on, while the screen is open.
#[derive(Resource)]
struct Editing {
    canvas: Canvas,
    color: u8,
    /// The brush's radius, in centimetres.
    brush: f32,
    /// Every dab is also made on the other side of the body.
    mirror: bool,
    undo: Vec<Canvas>,
    /// Where the camera is, around the model.
    yaw: f32,
    pitch: f32,
    distance: f32,
    /// The left button went down on the model: a stroke is being painted.
    stroke: Option<Stroke>,
    /// The right button went down on the model: it is being turned.
    turning: bool,
    /// The texture on the model needs updating.
    dirty: bool,
    /// Close without keeping what was painted.
    discard: bool,
    texture: Handle<Image>,
    material: Handle<StandardMaterial>,
}

struct Stroke {
    /// Where the last dab went, so a fast stroke is joined up.
    last: Option<Vec3>,
    changed: bool,
}

impl Editing {
    fn remember(&mut self) {
        self.undo.push(self.canvas.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    fn replace(&mut self, canvas: Canvas) {
        if canvas != self.canvas {
            self.remember();
            self.canvas = canvas;
            self.dirty = true;
        }
    }

    fn undo(&mut self) {
        if let Some(canvas) = self.undo.pop() {
            self.canvas = canvas;
            self.dirty = true;
        }
    }

    fn dab(&mut self, model: &PlayerModel, point: Vec3) -> bool {
        let radius = self.brush / 100.0;
        let mut changed = model.dab(&mut self.canvas, point, radius, self.color);
        if self.mirror {
            changed |= model.dab(&mut self.canvas, point * Vec3::new(-1.0, 1.0, 1.0), radius, self.color);
        }
        self.dirty |= changed;
        changed
    }

    fn camera_transform(&self) -> Transform {
        let offset = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch) * Vec3::Z * self.distance;
        Transform::from_translation(VIEW_TARGET + offset).looking_at(VIEW_TARGET, Vec3::Y)
    }
}

/// Every entity of the paint screen, despawned when it closes.
#[derive(Component)]
struct EditorPart;
#[derive(Component)]
struct PreviewCamera;
/// The picture of the model in the panel.
#[derive(Component)]
struct PaintView;
#[derive(Component)]
struct Swatch(u8);
#[derive(Component)]
struct BrushText;
#[derive(Component)]
struct BrushSlider;
#[derive(Component)]
struct MirrorText;

fn escape_key(keys: Res<ButtonInput<KeyCode>>, mut editor: ResMut<PaintEditor>) {
    if editor.open && keys.just_pressed(KeyCode::Escape) {
        editor.open = false;
    }
}

/// Builds the screen when it is opened; when it is closed, keeps the paint
/// job (unless cancelled) and takes it all down again.
fn sync_editor(
    mut commands: Commands,
    editor: Res<PaintEditor>,
    editing: Option<Res<Editing>>,
    mut paint: ResMut<MyPaint>,
    model: Res<PlayerModel>,
    assets: Res<SharedAssets>,
    mut focus: ResMut<InputFocus>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    parts: Query<Entity, With<EditorPart>>,
) {
    match (editor.open, editing) {
        (true, None) => {
            // Otherwise a menu text field behind the screen keeps taking the typing.
            focus.clear();
            let texture = images.add(paint.canvas.image());
            let material = materials.add(skin_material(texture.clone()));
            let editing = Editing {
                canvas: paint.canvas.clone(),
                color: paint.canvas.main_color(),
                brush: START_BRUSH,
                mirror: false,
                undo: Vec::new(),
                // From the front: the model faces -z.
                yaw: std::f32::consts::PI,
                pitch: 0.1,
                distance: START_DISTANCE,
                stroke: None,
                turning: false,
                dirty: false,
                discard: false,
                texture,
                material,
            };
            spawn_editor(&mut commands, &editing, &model, &assets, &mut images);
            commands.insert_resource(editing);
        }
        (false, Some(editing)) => {
            if !editing.discard {
                paint.set(editing.canvas.clone());
            }
            for entity in &parts {
                commands.entity(entity).despawn();
            }
            commands.remove_resource::<Editing>();
        }
        _ => {}
    }
}

fn spawn_editor(
    commands: &mut Commands,
    editing: &Editing,
    model: &PlayerModel,
    assets: &SharedAssets,
    images: &mut Assets<Image>,
) {
    let layer = RenderLayers::layer(LAYER);
    let view = images.add(Image::new_target_texture(
        VIEW_SIZE.x,
        VIEW_SIZE.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    ));

    // The model, standing at the origin of its own little world.
    let material = MeshMaterial3d(editing.material.clone());
    commands.spawn((
        EditorPart,
        Transform::default(),
        Visibility::default(),
        children![
            (
                Mesh3d(model.body.clone()),
                material.clone(),
                Transform::from_xyz(0.0, BODY_Y, 0.0),
                layer.clone(),
            ),
            (
                Mesh3d(model.head.clone()),
                material,
                Transform::from_xyz(0.0, HEAD_Y, 0.0),
                layer.clone(),
            ),
            // The visor, as everyone else sees it.
            (
                Mesh3d(assets.cube.clone()),
                MeshMaterial3d(assets.dark.clone()),
                Transform::from_xyz(0.0, HEAD_Y + 0.03, -0.17).with_scale(Vec3::new(0.36, 0.11, 0.2)),
                layer.clone(),
            ),
        ],
    ));
    // A light over the shoulder of the camera, wherever it goes, and a dim one
    // from above so the far side is not black.
    commands.spawn((
        EditorPart,
        PreviewCamera,
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.075, 0.085, 0.115)),
            ..default()
        },
        RenderTarget::Image(view.clone().into()),
        Projection::from(PerspectiveProjection {
            fov: VIEW_FOV,
            ..default()
        }),
        editing.camera_transform(),
        layer.clone(),
        children![(
            DirectionalLight {
                illuminance: 6500.0,
                ..default()
            },
            Transform::default().looking_to(Vec3::new(0.35, -0.4, -1.0), Vec3::Y),
            layer.clone(),
        )],
    ));
    commands.spawn((
        EditorPart,
        DirectionalLight {
            illuminance: 1800.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.2, -1.0, 0.3), Vec3::Y),
        layer,
    ));

    // The screen: the view on the left, the tools on the right.
    commands
        .spawn((
            EditorPart,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(20),
            BackgroundColor(ui::SCRIM.with_alpha(1.0)),
        ))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    width: percent(100),
                    max_width: px(1240),
                    height: percent(100),
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(48), px(32)),
                    row_gap: px(20),
                    ..default()
                })
                .with_children(|page| {
                    page.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: px(20),
                        ..default()
                    })
                    .with_children(|title| {
                        title.spawn(ui::button("‹  Cancel")).observe(
                            |_: On<Activate>, mut editing: ResMut<Editing>, mut editor: ResMut<PaintEditor>| {
                                editing.discard = true;
                                editor.open = false;
                            },
                        );
                        title.spawn(ui::heading("Paint yourself", 40.0, ui::TEXT));
                    });
                    page.spawn(Node {
                        flex_grow: 1.0,
                        flex_basis: px(0),
                        min_height: px(0),
                        column_gap: px(24),
                        ..default()
                    })
                    .with_children(|row| {
                        // The model, as big as there is room for, and how to work it.
                        row.spawn((
                            Node {
                                flex_grow: 1.0,
                                flex_basis: px(0),
                                min_width: px(0),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: px(12),
                                padding: UiRect::all(px(16)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(24)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.075, 0.085, 0.115)),
                            BorderColor::all(ui::PANEL_EDGE),
                        ))
                        .with_children(|view_box| {
                            view_box.spawn((
                                PaintView,
                                ImageNode::new(view),
                                Node {
                                    flex_grow: 1.0,
                                    min_height: px(0),
                                    aspect_ratio: Some(VIEW_SIZE.x as f32 / VIEW_SIZE.y as f32),
                                    border_radius: BorderRadius::all(px(12)),
                                    ..default()
                                },
                            ));
                            view_box
                                .spawn(Node {
                                    flex_wrap: FlexWrap::Wrap,
                                    justify_content: JustifyContent::Center,
                                    column_gap: px(8),
                                    row_gap: px(6),
                                    ..default()
                                })
                                .with_children(|hints| {
                                    for (key, what) in PAINT_HINTS {
                                        hints.spawn(hint(key, what));
                                    }
                                });
                        });
                        row.spawn((
                            Node {
                                width: px(480),
                                flex_shrink: 0.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: px(20),
                                padding: UiRect::all(px(22)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(24)),
                                ..default()
                            },
                            BackgroundColor(ui::PANEL),
                            BorderColor::all(ui::PANEL_EDGE),
                            ui::soft_shadow(),
                        ))
                        .with_children(|side| {
                            side.spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(10),
                                ..default()
                            })
                            .with_children(|colour| {
                                colour.spawn(ui::section("Colour"));
                                colour
                                    .spawn(Node {
                                        flex_wrap: FlexWrap::Wrap,
                                        width: px(SWATCH * SWATCHES_PER_ROW as f32 + SWATCH_GAP * (SWATCHES_PER_ROW - 1) as f32),
                                        column_gap: px(SWATCH_GAP),
                                        row_gap: px(SWATCH_GAP),
                                        ..default()
                                    })
                                    .with_children(|grid| {
                                        for color in 0..PALETTE.len() as u8 {
                                            grid.spawn(swatch(color)).observe(
                                                move |_: On<Activate>, mut editing: ResMut<Editing>| editing.color = color,
                                            );
                                        }
                                    });
                            });
                            side.spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(10),
                                ..default()
                            })
                            .with_children(|brush| {
                                brush.spawn(ui::section("Brush size"));
                                brush
                                    .spawn(Node {
                                        align_items: AlignItems::Center,
                                        column_gap: px(14),
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        row.spawn(brush_slider(editing.brush))
                                            .observe(slider_self_update)
                                            .observe(|change: On<ValueChange<f32>>, mut editing: ResMut<Editing>| {
                                                editing.brush = change.value;
                                            });
                                        row.spawn((
                                            ui::weighted("", 15.0, 700, TEXT),
                                            BrushText,
                                            TextLayout::new(Justify::Right, LineBreak::NoWrap),
                                            Node {
                                                width: px(70),
                                                ..default()
                                            },
                                        ));
                                    });
                            });
                            side.spawn(text_button("", MirrorText)).observe(
                                |_: On<Activate>, mut editing: ResMut<Editing>| editing.mirror = !editing.mirror,
                            );
                            side.spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(10),
                                ..default()
                            })
                            .with_children(|whole| {
                                whole.spawn(ui::section("Whole body"));
                                whole.spawn(ui::button_row()).with_children(|row| {
                                    row.spawn(ui::button("Fill all")).observe(
                                        |_: On<Activate>, mut editing: ResMut<Editing>| {
                                            let fill = Canvas::solid(editing.color);
                                            editing.replace(fill);
                                        },
                                    );
                                    row.spawn(ui::button("Undo"))
                                        .observe(|_: On<Activate>, mut editing: ResMut<Editing>| editing.undo());
                                    row.spawn(ui::danger_button("Start over")).observe(
                                        |_: On<Activate>, mut editing: ResMut<Editing>, model: Res<PlayerModel>| {
                                            editing.replace(model.plain(DEFAULT_COLOR));
                                        },
                                    );
                                });
                            });
                            side.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            side.spawn(ui::primary_button("Done"))
                                .observe(|_: On<Activate>, mut editor: ResMut<PaintEditor>| editor.open = false);
                        });
                    });
                });
        });
}

/// How to work the view, under it.
const PAINT_HINTS: [(&str, &str); 5] = [
    ("LEFT MOUSE", "Paint"),
    ("RIGHT MOUSE", "Turn around"),
    ("WHEEL", "Closer, further"),
    ("ALT + CLICK", "Take a colour"),
    ("CTRL + Z", "Undo"),
];

/// A key in a little box, and what it does.
fn hint(key: &str, what: &str) -> impl Bundle + use<> {
    (
        Node {
            align_items: AlignItems::Center,
            column_gap: px(6),
            padding: UiRect::new(px(4), px(10), px(4), px(4)),
            border_radius: BorderRadius::all(px(14)),
            ..default()
        },
        BackgroundColor(ui::PANEL.with_alpha(0.8)),
        children![
            (
                Node {
                    padding: UiRect::axes(px(7), px(3)),
                    border_radius: BorderRadius::all(px(10)),
                    ..default()
                },
                BackgroundColor(ui::BUTTON_EDGE),
                children![ui::weighted(key.to_string(), 10.0, 700, TEXT)],
            ),
            ui::label(what.to_string(), 12.0, DIM),
        ],
    )
}

/// A colour to paint with. Not styled like the other buttons (it keeps its
/// colour); the one picked gets a bright edge.
fn swatch(color: u8) -> impl Bundle {
    (
        Swatch(color),
        Button,
        Node {
            width: px(SWATCH),
            height: px(SWATCH),
            border: UiRect::all(px(3)),
            border_radius: BorderRadius::all(px(12)),
            ..default()
        },
        BackgroundColor(palette_color(color)),
        BorderColor::all(Color::NONE),
    )
}

/// The bar that sets the brush size: a rail, and a round thumb dragged along it.
fn brush_slider(size: f32) -> impl Bundle {
    let (smallest, largest) = BRUSH_SIZES;
    (
        BrushSlider,
        Slider {
            track_click: TrackClick::Snap,
            ..default()
        },
        SliderValue(size),
        SliderRange::new(smallest, largest),
        SliderStep(0.5),
        Hovered::default(),
        Node {
            width: px(SLIDER_WIDTH),
            height: px(THUMB),
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                Node {
                    width: percent(100),
                    height: px(6),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(ui::BUTTON),
                Pickable::IGNORE,
            ),
            (
                SliderThumb,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(THUMB),
                    height: px(THUMB),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(ACCENT),
            ),
        ],
    )
}

/// A small button whose text changes.
fn text_button(text: &str, marker: impl Component) -> impl Bundle {
    (
        ui::button_box(ButtonKind::Normal, true),
        children![(ui::label(text, 15.0, TEXT), Pickable::IGNORE, marker)],
    )
}

/// The mouse on the model: painting, turning it, coming closer.
fn edit(
    mut editing: ResMut<Editing>,
    model: Res<PlayerModel>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    view: Single<(&ComputedNode, &UiGlobalTransform), With<PaintView>>,
    camera: Single<&Transform, With<PreviewCamera>>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if ctrl && keys.just_pressed(KeyCode::KeyZ) {
        editing.undo();
    }

    // Where the mouse is on the picture, from -0.5 to 0.5 each way (y down).
    let (node, node_transform) = *view;
    let on_view = window
        .physical_cursor_position()
        .and_then(|cursor| node.normalize_point(*node_transform, cursor))
        .filter(|at| at.abs().max_element() <= 0.5);
    let ray = on_view.map(|at| {
        let tan = (VIEW_FOV / 2.0).tan();
        let aspect = VIEW_SIZE.x as f32 / VIEW_SIZE.y as f32;
        let dir = Vec3::new(2.0 * at.x * tan * aspect, -2.0 * at.y * tan, -1.0).normalize();
        (camera.translation, camera.rotation * dir)
    });
    let hit = ray.and_then(|(origin, dir)| model.pick(origin, dir));

    if mouse.just_pressed(MouseButton::Left) && on_view.is_some() {
        if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
            if let Some(texel) = hit {
                editing.color = editing.canvas.0[texel];
            }
        } else {
            editing.remember();
            editing.stroke = Some(Stroke {
                last: None,
                changed: false,
            });
        }
    }
    if !mouse.pressed(MouseButton::Left)
        && let Some(stroke) = editing.stroke.take()
        && !stroke.changed
    {
        // Nothing was painted: nothing to undo.
        editing.undo.pop();
    }
    if editing.stroke.is_some() {
        let point = hit.and_then(|texel| model.texels[texel]).map(|(point, _)| point);
        let last = editing.stroke.as_ref().and_then(|stroke| stroke.last);
        let mut changed = false;
        if let Some(point) = point {
            // A dab every third of a brush along the way from the last one.
            let from = last.unwrap_or(point);
            let spacing = editing.brush / 100.0 * 0.35;
            let steps = (from.distance(point) / spacing).ceil().max(1.0) as usize;
            for step in 1..=steps {
                changed |= editing.dab(&model, from.lerp(point, step as f32 / steps as f32));
            }
        }
        if let Some(stroke) = &mut editing.stroke {
            stroke.last = point;
            stroke.changed |= changed;
        }
    }

    if mouse.just_pressed(MouseButton::Right) && on_view.is_some() {
        editing.turning = true;
    }
    if !mouse.pressed(MouseButton::Right) {
        editing.turning = false;
    }
    if editing.turning {
        editing.yaw -= motion.delta.x * 0.008;
        editing.pitch = (editing.pitch + motion.delta.y * 0.006).clamp(-0.6, 1.0);
    }
    if on_view.is_some() && scroll.delta.y != 0.0 {
        let (near, far) = DISTANCES;
        editing.distance = (editing.distance * (1.0 - 0.12 * scroll.delta.y.clamp(-3.0, 3.0))).clamp(near, far);
    }
}

/// Keeps the model, the camera and the buttons up to date.
fn show_changes(
    mut editing: ResMut<Editing>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera: Single<&mut Transform, With<PreviewCamera>>,
    mut swatches: Query<(&Swatch, &mut BorderColor)>,
    mut brush_text: Query<&mut Text, (With<BrushText>, Without<MirrorText>)>,
    mut mirror: Query<&mut Text, (With<MirrorText>, Without<BrushText>)>,
    sliders: Query<(&SliderValue, &SliderRange, &Hovered, &SliderDragState, &Children), With<BrushSlider>>,
    mut thumbs: Query<(&mut Node, &mut BackgroundColor), With<SliderThumb>>,
) {
    for (value, range, hovered, drag, children) in &sliders {
        for child in children.iter() {
            if let Ok((mut node, mut background)) = thumbs.get_mut(child) {
                let left = px(range.thumb_position(value.0) * (SLIDER_WIDTH - THUMB));
                if node.left != left {
                    node.left = left;
                }
                let color = if hovered.get() || drag.dragging { Color::WHITE } else { ACCENT };
                background.set_if_neq(BackgroundColor(color));
            }
        }
    }
    for mut text in &mut brush_text {
        ui::set_text(&mut text, format!("{:.1} cm", editing.brush));
    }
    if editing.dirty {
        editing.dirty = false;
        if let Some(mut image) = images.get_mut(&editing.texture) {
            editing.canvas.write_to(&mut image);
        }
        // So the material picks up the new texture.
        let _ = materials.get_mut(&editing.material);
    }
    let wanted = editing.camera_transform();
    if **camera != wanted {
        **camera = wanted;
    }
    for (swatch, mut border) in &mut swatches {
        let edge = if swatch.0 == editing.color { Color::WHITE } else { Color::srgba(0.0, 0.0, 0.0, 0.4) };
        if border.top != edge {
            *border = BorderColor::all(edge);
        }
    }
    for mut text in &mut mirror {
        ui::set_text(&mut text, format!("Mirror left and right: {}", if editing.mirror { "on" } else { "off" }));
    }
}
