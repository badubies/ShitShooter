//! Weapon models from `assets/models/weapons`, drawn in place of the box guns for the
//! weapons that have one (see `Weapon::model`).
//!
//! Every gun holder (the first-person gun, and each remote player's gun) gets a
//! child per model and shows the one for the weapon in hand. A model built for
//! the game (`Fit::AsBuilt`) is placed as it is. Any other is measured once it
//! has loaded, turned barrel-forward, centred on the holder and scaled to its
//! weapon's `size` length.
//!
//! A gun's own `AimPoint` node, if it has one, is on the line along its iron
//! sights, and aiming without a sight puts that line through the eye.
//!
//! A gun with a `SightMount` node can carry the sights its `Model::sights`
//! lists, each another glTF file in metres. All of them are mounted on it, and
//! the holder's `sight` picks the one shown. The mount's position and scale (sight metres
//! to gun-file units) are set in the gun's file. The gun's `IronSight...` nodes
//! are hidden under a sight, and aiming lines up the sight's `AimPoint` node
//! with the crosshair.
//!
//! The nodes that move in a reload (see `PartKind`) are found and marked with
//! a `GunPart`, which moves them in the holder's space, whatever the units
//! and axes of the file.

use std::f32::consts::PI;

use bevy::{camera::primitives::Aabb, light::NotShadowCaster, prelude::*};

use crate::weapons::{Fit, IRON_SIGHTS, SIGHTS, SightId, WEAPONS, WeaponId, sight_on, weapon};

pub struct GunModelPlugin;

impl Plugin for GunModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FittedModels>()
            .add_systems(Startup, load_models)
            .add_systems(
                Update,
                (
                    attach_models,
                    fit_models,
                    measure_sights,
                    show_models.in_set(GunModelSystems::Show),
                )
                    .chain(),
            )
            .configure_sets(
                Update,
                GunModelSystems::Show
                    .after(crate::player::PlayerSystems::Camera)
                    .after(crate::remote::RemoteSystems),
            );
    }
}

/// Shows the model of the weapon each holder has. Holders set their weapon before this.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GunModelSystems {
    Show,
}

/// An entity that holds a gun: its children get one model per modelled weapon.
#[derive(Component)]
pub struct GunHolder {
    pub weapon: WeaponId,
    /// The sight on it, if the weapon takes one.
    pub sight: SightId,
    /// Off for the first-person gun, whose shadow would float in front of you.
    pub shadows: bool,
}

/// What is known about each modelled weapon once its model is fitted, the same
/// for every holder. `None` while it is loading, and for weapons drawn as boxes.
#[derive(Resource, Default)]
pub struct FittedModels([Option<Fitted>; WEAPONS.len()]);

#[derive(Clone, Copy)]
pub struct Fitted {
    /// Width, height and length in metres, centred on the holder.
    pub size: Vec3,
    /// A point on the line along its iron sights, relative to the holder, if
    /// the model marks one.
    pub iron_aim: Option<Vec3>,
    /// Where each sight's dot is, relative to the holder, once that sight is in.
    aim_points: [Option<Vec3>; SIGHTS.len()],
}

impl Fitted {
    /// Where the dot of this sight is, once it is mounted. `None` for the iron sights.
    pub fn aim_point(&self, sight: SightId) -> Option<Vec3> {
        self.aim_points.get(sight as usize).copied().flatten()
    }
}

impl FittedModels {
    pub fn get(&self, id: WeaponId) -> Option<Fitted> {
        self.0.get(id as usize).copied().flatten()
    }
}

/// The loaded scenes of each weapon's model (by weapon id) and each sight's
/// (by sight id; `None` for the iron sights).
#[derive(Resource)]
struct ModelScenes {
    guns: Vec<Option<Handle<WorldAsset>>>,
    sights: Vec<Option<Handle<WorldAsset>>>,
}

/// One weapon's model under a holder. Hidden until it is fitted.
#[derive(Component)]
struct GunModel {
    weapon: WeaponId,
    fitted: bool,
    shadows: bool,
    /// The sights mounted on it, by sight id: each one's scene entity, and
    /// whether it has been measured yet. Empty for guns without a mount.
    sights: Vec<Option<(Entity, bool)>>,
    /// Its iron sight nodes, hidden while another sight is shown.
    irons: Vec<Entity>,
}

/// A node of a gun model that moves when reloading and firing, found by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PartKind {
    /// Drops out and is put back in. Its origin is the top of the magazine.
    Magazine,
    /// The pistol's slide and a rifle's bolt: back on a shot, held back when
    /// empty, forward at the end of the reload.
    Slide,
    Bolt,
    /// The shotgun's pump, worked after each shot and after loading it empty.
    Pump,
    /// A shotgun shell, hidden inside the receiver until it is loaded.
    Shell,
}

const PART_NAMES: [(&str, PartKind); 5] = [
    ("Magazine", PartKind::Magazine),
    ("Slide", PartKind::Slide),
    ("Bolt", PartKind::Bolt),
    ("Pump", PartKind::Pump),
    ("Shell", PartKind::Shell),
];

/// Marks a moving node of a gun model (see `PartKind`).
#[derive(Component)]
pub struct GunPart {
    pub kind: PartKind,
    /// The holder the gun is in, and whose model it is.
    pub holder: Entity,
    pub weapon: WeaponId,
    /// Where it is in the file.
    rest: Transform,
    /// Takes a direction in the holder's space to its parent's space.
    to_parent: Mat3,
}

impl GunPart {
    /// Puts the part `offset` away from where it rests, and turned by `angle`
    /// about `axis` round its own origin. Both are in the holder's space,
    /// where the barrel points along -Z and up is +Y.
    pub fn pose(&self, transform: &mut Transform, offset: Vec3, axis: Vec3, angle: f32) {
        let translation = self.rest.translation + self.to_parent * offset;
        let turn = Quat::from_axis_angle((self.to_parent * axis).normalize_or(Vec3::X), angle);
        let rotation = turn * self.rest.rotation;
        if transform.translation != translation || transform.rotation != rotation {
            transform.translation = translation;
            transform.rotation = rotation;
        }
    }
}

impl GunPart {
    /// A magazine `out` of the way: 0 in place, 1 dropped down and away from
    /// the gun and tipped (and hidden: gone, or not yet brought up).
    pub fn pose_magazine(&self, transform: &mut Transform, visibility: &mut Mut<Visibility>, out: f32) {
        let length = weapon(self.weapon).size[2];
        self.pose(transform, Vec3::new(0.0, -0.6, 0.12) * length * out, Vec3::X, 0.45 * out);
        visibility.set_if_neq(if out < 0.98 { Visibility::Inherited } else { Visibility::Hidden });
    }
}

const SIGHT_MOUNT: &str = "SightMount";
const AIM_POINT: &str = "AimPoint";
const IRON_SIGHT_PREFIX: &str = "IronSight";

fn load_models(mut commands: Commands, asset_server: Res<AssetServer>) {
    let load = |path: &'static str| asset_server.load(GltfAssetLabel::Scene(0).from_asset(path));
    commands.insert_resource(ModelScenes {
        guns: WEAPONS.iter().map(|w| w.model.as_ref().map(|model| load(model.path))).collect(),
        sights: SIGHTS.iter().map(|s| s.path.map(load)).collect(),
    });
}

fn attach_models(
    mut commands: Commands,
    scenes: Res<ModelScenes>,
    holders: Query<(Entity, &GunHolder), Added<GunHolder>>,
) {
    for (holder, settings) in &holders {
        for (id, scene) in scenes.guns.iter().enumerate() {
            let Some(scene) = scene else {
                continue;
            };
            let model = commands
                .spawn((
                    GunModel {
                        weapon: id as WeaponId,
                        fitted: false,
                        shadows: settings.shadows,
                        sights: Vec::new(),
                        irons: Vec::new(),
                    },
                    Transform::default(),
                    Visibility::Hidden,
                    ChildOf(holder),
                ))
                .id();
            // Its own entity, so the model is measured in its file's units,
            // whatever the fit above it does.
            commands.spawn((WorldAssetRoot(scene.clone()), ChildOf(model)));
        }
    }
}

/// Measures each newly loaded model, fits it in place of the box gun, and
/// mounts the sights.
fn fit_models(
    mut commands: Commands,
    scenes: Res<ModelScenes>,
    mut fitted: ResMut<FittedModels>,
    mut models: Query<(&mut GunModel, &mut Transform, &Children, &ChildOf)>,
    descendants: Query<&Children>,
    globals: Query<&GlobalTransform>,
    meshes: Query<(&GlobalTransform, Option<&Aabb>), With<Mesh3d>>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    locals: Query<&Transform, Without<GunModel>>,
) {
    for (mut model, mut transform, children, parent) in &mut models {
        if model.fitted {
            continue;
        }
        let Some(&scene) = children.first() else {
            continue;
        };
        let Ok(scene_global) = globals.get(scene) else {
            continue;
        };
        let to_scene = scene_global.affine().inverse();
        let (mut low, mut high) = (Vec3::MAX, Vec3::MIN);
        let mut parts = Vec::new();
        let mut waiting = false;
        for entity in descendants.iter_descendants(scene) {
            let Ok((global, bounds)) = meshes.get(entity) else {
                continue;
            };
            // Bounds are added the frame after the mesh appears.
            let Some(bounds) = bounds else {
                waiting = true;
                break;
            };
            let (min, max) = (Vec3::from(bounds.min()), Vec3::from(bounds.max()));
            for corner in 0..8 {
                let pick = |bit: u32, low: f32, high: f32| if corner & bit == 0 { low } else { high };
                let point = Vec3::new(pick(1, min.x, max.x), pick(2, min.y, max.y), pick(4, min.z, max.z));
                let point = to_scene.transform_point3(global.transform_point(point));
                low = low.min(point);
                high = high.max(point);
            }
            parts.push(entity);
        }
        // Nothing yet: the scene has not been spawned.
        if waiting || parts.is_empty() {
            continue;
        }

        let stats = weapon(model.weapon);
        let size = high - low;
        let Some(spec) = &stats.model else {
            continue;
        };
        if size.z <= 0.0 {
            continue;
        }
        let middle = (low + high) / 2.0;
        let scale = match spec.fit {
            Fit::AsBuilt => {
                // Placed as it is, so it should be built to the table.
                if (size.z - stats.size[2]).abs() > 0.001 || middle.length() > 0.001 {
                    warn!(
                        "{} is {:.4} m long and centred {middle}; it should be {} m, centred on its origin",
                        spec.path, size.z, stats.size[2]
                    );
                }
                1.0
            }
            Fit::Scaled { barrel_plus_z } => {
                let scale = stats.size[2] / size.z;
                let turn = if barrel_plus_z { Quat::from_rotation_y(PI) } else { Quat::IDENTITY };
                transform.rotation = turn;
                transform.scale = Vec3::splat(scale);
                transform.translation = -(turn * (middle * scale));
                scale
            }
        };
        model.fitted = true;
        // The moving parts, and how the holder's space maps to their parents'.
        let holder = parent.parent();
        for entity in descendants.iter_descendants(scene) {
            let Some(kind) = names
                .get(entity)
                .ok()
                .and_then(|name| PART_NAMES.iter().find(|(part, _)| name.as_str() == *part).map(|(_, kind)| *kind))
            else {
                continue;
            };
            let (Ok(rest), Some(parent_global)) = (
                locals.get(entity),
                parents.get(entity).ok().and_then(|of| globals.get(of.parent()).ok()),
            ) else {
                continue;
            };
            let parent_in_holder = transform.compute_affine() * to_scene * parent_global.affine();
            commands.entity(entity).insert((
                GunPart {
                    kind,
                    holder,
                    weapon: model.weapon,
                    rest: *rest,
                    to_parent: Mat3::from(parent_in_holder.matrix3).inverse(),
                },
                // The shell only shows while it is being loaded.
                if kind == PartKind::Shell { Visibility::Hidden } else { Visibility::Inherited },
            ));
        }
        // Looked for before any sight is mounted, so it is the gun's own.
        let iron_aim = descendants
            .iter_descendants(scene)
            .find(|&e| names.get(e).is_ok_and(|name| name.as_str() == AIM_POINT))
            .and_then(|aim| globals.get(aim).ok())
            .map(|aim| transform.transform_point(to_scene.transform_point3(aim.translation())));
        fitted.0[model.weapon as usize] = Some(Fitted {
            size: size * scale,
            iron_aim,
            aim_points: [None; SIGHTS.len()],
        });
        if !model.shadows {
            for part in parts {
                commands.entity(part).insert(NotShadowCaster);
            }
        }

        if spec.sights.is_empty() {
            continue;
        }
        let named = |prefix: &'static str| {
            descendants
                .iter_descendants(scene)
                .filter(move |&e| names.get(e).is_ok_and(|name| name.as_str().starts_with(prefix)))
        };
        let Some(mount) = named(SIGHT_MOUNT).next() else {
            warn!("{} has no {SIGHT_MOUNT} node", spec.path);
            continue;
        };
        model.sights = scenes
            .sights
            .iter()
            .enumerate()
            .map(|(id, sight)| {
                let sight = sight.as_ref().filter(|_| spec.sights.contains(&(id as SightId)))?;
                let entity = commands
                    .spawn((WorldAssetRoot(sight.clone()), Visibility::Hidden, ChildOf(mount)))
                    .id();
                Some((entity, false))
            })
            .collect();
        model.irons = named(IRON_SIGHT_PREFIX).collect();
        // So that `show_models` can hide them.
        for &iron in &model.irons {
            commands.entity(iron).insert(Visibility::Inherited);
        }
    }
}

/// Finds where each newly loaded sight's dot ended up, for aiming.
fn measure_sights(
    mut commands: Commands,
    mut fitted: ResMut<FittedModels>,
    mut models: Query<(&mut GunModel, &Transform, &Children)>,
    descendants: Query<&Children>,
    globals: Query<&GlobalTransform>,
    meshes: Query<Has<Aabb>, With<Mesh3d>>,
    names: Query<&Name>,
) {
    for (mut model, transform, children) in &mut models {
        let (id, shadows) = (model.weapon, model.shadows);
        for (sight_id, slot) in model.sights.iter_mut().enumerate() {
            let Some((sight, measured)) = slot else {
                continue;
            };
            if *measured {
                continue;
            }
            let mut parts = Vec::new();
            let mut aim = None;
            for entity in descendants.iter_descendants(*sight) {
                if let Ok(has_bounds) = meshes.get(entity) {
                    parts.push((entity, has_bounds));
                }
                if names.get(entity).is_ok_and(|name| name.as_str() == AIM_POINT) {
                    aim = Some(entity);
                }
            }
            // Bounds appear once transforms have caught up with the new parts.
            if parts.is_empty() || parts.iter().any(|&(_, has_bounds)| !has_bounds) {
                continue;
            }
            *measured = true;
            if !shadows {
                for &(part, _) in &parts {
                    commands.entity(part).insert(NotShadowCaster);
                }
            }
            let (Some(aim), Some(&scene)) = (aim, children.first()) else {
                warn!("sight {sight_id} has no {AIM_POINT} node");
                continue;
            };
            let (Ok(aim), Ok(scene)) = (globals.get(aim), globals.get(scene)) else {
                continue;
            };
            // From the gun's file space to the holder, through the fit.
            let in_scene = scene.affine().inverse().transform_point3(aim.translation());
            if let Some(entry) = &mut fitted.0[id as usize] {
                entry.aim_points[sight_id] = Some(transform.transform_point(in_scene));
            }
        }
    }
}

/// Shows the model of the weapon in each holder's hands, with the holder's
/// sight on it (or its iron sights).
fn show_models(
    holders: Query<&GunHolder>,
    models: Query<(Entity, &GunModel, &ChildOf)>,
    mut visibilities: Query<&mut Visibility>,
) {
    let mut set = |entity: Entity, shown: bool| {
        if let Ok(mut visibility) = visibilities.get_mut(entity) {
            visibility.set_if_neq(if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    };
    for (entity, model, parent) in &models {
        let holder = holders.get(parent.parent()).ok();
        set(entity, model.fitted && holder.is_some_and(|h| h.weapon == model.weapon));
        let wanted = holder.map_or(IRON_SIGHTS, |h| sight_on(model.weapon, h.sight));
        let mut mounted = false;
        for (sight_id, slot) in model.sights.iter().enumerate() {
            if let Some((sight, _)) = slot {
                let on = sight_id == wanted as usize;
                mounted |= on;
                set(*sight, on);
            }
        }
        for &iron in &model.irons {
            set(iron, !mounted);
        }
    }
}
