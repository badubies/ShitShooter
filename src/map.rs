//! The maps: static axis-aligned blocks, ramps, jump pads and spawn points,
//! plus the collision helpers used by the player controller, the bots,
//! grenades and hit detection. Pure data and math, so the server can use it without any rendering.

use bevy::math::{Quat, Vec2, Vec3};

use crate::{
    course::{Course, Zone},
    modes::{self, Needs, Offers},
    protocol::{FLASH_BLIND, FLASH_RANGE, SMOKE_RADIUS, SMOKE_RISE, TEAM_SIZE},
    props::{FLAGPOLE, PROPS},
    ramp::{Ramp, WALKABLE},
    terrain::Terrain,
};

/// Half extents of the player's collision box when standing (0.8 x 1.8 x 0.8 m).
pub const PLAYER_HALF: Vec3 = Vec3::new(0.4, 0.9, 0.4);
/// The same when crouching or sliding (1.2 m tall).
pub const CROUCH_HALF: Vec3 = Vec3::new(0.4, 0.6, 0.4);
/// Eye height above the feet.
pub const STAND_EYE: f32 = 1.6;
pub const CROUCH_EYE: f32 = 1.0;
/// Half the side of the little box a grenade collides as.
pub const GRENADE_HALF: f32 = 0.12;
const GRENADE_GRAVITY: f32 = 20.0;
/// A grenade hitting something faster than this makes a sound.
pub const BOUNCE_SOUND: f32 = 3.0;
/// Walking into a ledge at most this high above your feet (a stair, a kerb)
/// lifts you onto it, and walking off one this high keeps you on the ground.
pub const STEP_UP: f32 = 0.5;
/// Side of a cell of the grids that find the blocks and ramps near a point quickly.
const GRID_CELL: f32 = 8.0;

pub const MAP_NAMES: [&str; 6] = ["Arena", "Depot", "Valley", "Hamlet", "Surf", "Gauntlet"];
/// The id of the built-in surf course, the one built-in map with a `Course`.
pub const GAUNTLET: u8 = 5;

/// Maps made in Blender (`custom.rs`) come after the built-in ones: map
/// `BUILT_IN + i` is the `i`th of them.
pub const BUILT_IN: u8 = MAP_NAMES.len() as u8;

/// How many maps there are: the built-in ones and the ones made in Blender.
pub fn count() -> u8 {
    BUILT_IN + crate::custom::count() as u8
}

/// Map `id`'s name.
pub fn name(id: u8) -> String {
    match MAP_NAMES.get(id as usize) {
        Some(name) => name.to_string(),
        None => crate::custom::name((id - BUILT_IN) as usize).unwrap_or_else(|| "?".to_string()),
    }
}

/// The id of the map called `name` (any case), if there is one.
pub fn by_name(name: &str) -> Option<u8> {
    (0..count()).find(|id| self::name(*id).eq_ignore_ascii_case(name))
}

/// True if map `id` is a surf course.
pub fn is_course(id: u8) -> bool {
    plays(id, modes::MODE_RACE)
}

/// What each built-in map has to play on, in the order of `MAP_NAMES`.
/// Hamlet is built for close fights only, and Surf for riding ramps: neither
/// has flags for conquest nor a hill.
const BUILT_IN_OFFERS: [Offers; MAP_NAMES.len()] = {
    let all = Offers { hill: true, flags: true, course: false };
    let fights = Offers { hill: false, flags: false, course: false };
    let course = Offers { hill: false, flags: false, course: true };
    [all, all, all, fights, fights, course]
};

/// True if map `id` can be played in `mode`: it has what the mode needs.
/// The racing modes are run on the courses, and the courses are for nothing
/// else. A map made in Blender says for itself (`custom.rs`).
pub fn plays(id: u8, mode: u8) -> bool {
    if mode >= modes::count() {
        return false;
    }
    match BUILT_IN_OFFERS.get(id as usize) {
        Some(offers) => offers.has(modes::info(mode).needs),
        None => crate::custom::plays((id - BUILT_IN) as usize, mode),
    }
}

/// The most maps any one mode is played on.
pub fn most_maps_in_a_mode() -> usize {
    (0..modes::count())
        .map(|mode| (0..MAP_NAMES.len() as u8).filter(|id| plays(*id, mode)).count())
        .max()
        .unwrap_or(1)
}

pub fn player_half(crouching: bool) -> Vec3 {
    if crouching { CROUCH_HALF } else { PLAYER_HALF }
}

/// Slightly fatter box used for bullet hits, to be forgiving at high speed.
pub fn hitbox_half(crouching: bool) -> Vec3 {
    player_half(crouching) + Vec3::new(0.08, 0.05, 0.08)
}

/// The part of the bullet hitbox that counts as the head: the top of it,
/// narrower than the shoulders. `center` is the centre of the collision box.
pub fn head_box(center: Vec3, crouching: bool) -> (Vec3, Vec3) {
    const HEAD_HALF: Vec3 = Vec3::new(0.3, 0.27, 0.3);
    let top = center.y + hitbox_half(crouching).y;
    let mid = Vec3::new(center.x, top - HEAD_HALF.y, center.z);
    (mid - HEAD_HALF, mid + HEAD_HALF)
}

pub struct Block {
    pub min: Vec3,
    pub max: Vec3,
    pub color: [f32; 3],
    /// Invisible blocks still collide (used for the ceiling).
    pub visible: bool,
}

/// Two kinds of pad, told apart by `launch`:
/// - arrow pads (`launch` has a horizontal part) throw you that way, keeping
///   only half the speed you came in with, so they land you somewhere precise;
/// - plain pads (`launch` points straight up) only throw you up, keeping
///   your speed and direction.
pub struct JumpPad {
    /// Centre of the pad on the floor.
    pub center: Vec3,
    pub half_size: f32,
    /// Velocity a player standing still is launched with.
    pub launch: Vec3,
}

impl JumpPad {
    pub fn arrow(center: Vec3, launch: Vec3) -> JumpPad {
        JumpPad { center, half_size: 1.0, launch }
    }

    pub fn plain(center: Vec3, lift: f32) -> JumpPad {
        JumpPad { center, half_size: 1.0, launch: Vec3::Y * lift }
    }

    /// The horizontal direction an arrow pad pushes you in, or `None` for a plain pad.
    pub fn direction(&self) -> Option<Vec3> {
        let push = self.launch.with_y(0.0);
        (push.length_squared() > 1e-6).then(|| push.normalize())
    }

    /// Velocity after touching the pad while moving at `vel`.
    pub fn launch_velocity(&self, vel: Vec3) -> Vec3 {
        let kept = if self.direction().is_some() { 0.5 } else { 1.0 };
        Vec3::new(vel.x * kept + self.launch.x, self.launch.y, vel.z * kept + self.launch.z)
    }
}

pub struct Spawn {
    pub pos: Vec3,
    pub yaw: f32,
}

/// A model from `props::PROPS` standing on the map. Its collision boxes are
/// among the map's (invisible) blocks, and its solids among its (hidden) ramps.
pub struct Prop {
    pub model: usize,
    /// Where its origin stands.
    pub pos: Vec3,
    /// Turned about the vertical, in radians. Models that collide are only
    /// ever turned by quarter turns, so their boxes stay upright.
    pub yaw: f32,
    pub scale: f32,
}

impl Prop {
    /// Its collision boxes (min, max), where it stands.
    fn boxes(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        let turn = Quat::from_rotation_y(self.yaw);
        PROPS[self.model].colliders.iter().map(move |[x0, y0, z0, x1, y1, z1]| {
            let a = turn * (Vec3::new(*x0, *y0, *z0) * self.scale) + self.pos;
            let b = turn * (Vec3::new(*x1, *y1, *z1) * self.scale) + self.pos;
            (a.min(b), a.max(b))
        })
    }

    /// Its solids (roofs, rocks, stairs) as hidden ramps, where it stands.
    pub fn ramps(&self) -> impl Iterator<Item = Ramp> + '_ {
        let turn = Quat::from_rotation_y(self.yaw);
        PROPS[self.model].solids.iter().map(move |solid| {
            let corners = solid.corners.iter().map(|c| turn * (Vec3::from(*c) * self.scale) + self.pos).collect();
            let faces = solid.faces.iter().map(|face| face.iter().map(|&i| i as usize).collect()).collect();
            let mut ramp = Ramp::solid(corners, faces, [0.0; 3]);
            ramp.visible = false;
            ramp
        })
    }
}

/// A road drawn over the terrain, along its centre line.
pub struct Road {
    pub points: Vec<Vec2>,
    pub width: f32,
    /// Asphalt, drawn as a strip of its own; dirt roads are only coloured
    /// into the ground.
    pub paved: bool,
}

/// A capture point in conquest.
pub struct FlagPoint {
    /// "A", "B"...
    pub name: char,
    /// The foot of the flagpole, on the ground.
    pub pos: Vec3,
    /// Players within this distance (along the ground) are on the point.
    pub radius: f32,
    /// Where its holders come back in.
    pub spawns: Vec<Spawn>,
}

impl FlagPoint {
    /// True if a player whose box is centred at `center` is on the flag.
    pub fn holds(&self, center: Vec3) -> bool {
        (center - self.pos).with_y(0.0).length() < self.radius
            && (center.y - PLAYER_HALF.y - self.pos.y).abs() < crate::protocol::FLAG_HEIGHT
    }
}

/// A lamp or a fire on a map made in Blender: light from a point.
pub struct Lamp {
    pub pos: Vec3,
    /// sRGB.
    pub color: [f32; 3],
    /// How bright (lumens): a million lights the ground 3 m away about as
    /// the sun does by day.
    pub power: f32,
    /// It lights nothing further away than this.
    pub range: f32,
    /// A fire: it wavers.
    pub flicker: bool,
}

/// How a map is lit and what is around it.
pub struct Look {
    /// The colour of the sky (sRGB).
    pub sky: [f32; 3],
    /// Fog from this far to this far, if any. Its colour is the sky's.
    pub fog: Option<(f32, f32)>,
    /// Where the sun shines from, and how brightly (lux).
    pub sun_from: Vec3,
    pub sun: f32,
    /// The colour of its light (sRGB): white by day, blue for a moon.
    pub sun_color: [f32; 3],
    pub ambient: f32,
    /// The menu camera circles this point at this distance and height.
    pub orbit: (Vec3, f32, f32),
}

impl Default for Look {
    /// The indoor arenas: a dark sky, no fog.
    fn default() -> Look {
        Look {
            sky: [0.03, 0.04, 0.07],
            fog: None,
            sun_from: Vec3::new(30.0, 60.0, 18.0),
            sun: 9000.0,
            sun_color: [1.0; 3],
            ambient: 350.0,
            orbit: (Vec3::new(0.0, 2.0, 0.0), 46.0, 26.0),
        }
    }
}

pub struct Map {
    /// Index into `MAP_NAMES`, or past it, a map made in Blender (`BUILT_IN`).
    pub id: u8,
    /// Do not change after `finish`: the grid indexes them.
    pub blocks: Vec<Block>,
    /// Sloped solids, to stand on or (the steep ones) to surf. Do not
    /// change after `finish` either.
    pub ramps: Vec<Ramp>,
    /// Maps made in Blender: the texture (`textures::Pattern::named`) for
    /// each colour that names one.
    pub textures: Vec<([f32; 3], String)>,
    /// Maps made in Blender: how strongly each colour that glows does (a
    /// lit window, a fire, a lamp), as a multiple of the colour.
    pub glows: Vec<([f32; 3], f32)>,
    /// Maps made in Blender: their lamps and fires.
    pub lamps: Vec<Lamp>,
    /// Fallen out of the map below this (well under anything solid): put
    /// back in the middle. Courses catch falls well before it.
    pub abyss: f32,
    /// The ground of the outdoor maps. Indoor maps stand on a floor block.
    pub terrain: Option<Terrain>,
    /// The corners of the part of the map that is played in (x, z): bots
    /// pick where to run within it.
    pub bounds: [Vec2; 2],
    pub pads: Vec<JumpPad>,
    pub spawns: Vec<Spawn>,
    /// Centre of the zone to hold in king of the hill.
    pub hill: Vec3,
    /// Where armor pickups lie, on top of the floor or block below them.
    /// At most 32 (they are sent as a bit mask).
    pub armor_spots: Vec<Vec3>,
    /// Models standing on the map.
    pub props: Vec<Prop>,
    pub roads: Vec<Road>,
    /// Shallow water: strips along these centre lines (points at the water
    /// surface), this wide.
    pub water: Vec<(Vec<Vec3>, f32)>,
    /// Conquest: the capture points, and where each team starts.
    pub flags: Vec<FlagPoint>,
    pub bases: [Vec<Spawn>; 2],
    /// The racing modes: the start, checkpoints and finish.
    pub course: Option<Course>,
    /// Spawns set aside for a role, each with the role's name ("sniper"):
    /// `spawns` are everybody else's. Only maps made in Blender have them.
    pub posts: Vec<(String, Spawn)>,
    /// Places a mode looks for by name ("finish"). Only maps made in
    /// Blender have them.
    pub zones: Vec<(String, Zone)>,
    pub look: Look,
    grid: BlockGrid,
    ramp_grid: BlockGrid,
}

/// Which blocks (or ramps) are near which part of the map: the ones whose
/// bounds touch each square cell of a grid over the map's footprint.
#[derive(Default)]
struct BlockGrid {
    origin: Vec2,
    nx: usize,
    nz: usize,
    cells: Vec<Vec<u32>>,
    /// How many things it indexes.
    count: usize,
}

impl BlockGrid {
    /// A grid over things with these bounds (min, max).
    fn new(bounds: &[(Vec3, Vec3)]) -> BlockGrid {
        let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for (lo, hi) in bounds {
            min = min.min(Vec2::new(lo.x, lo.z));
            max = max.max(Vec2::new(hi.x, hi.z));
        }
        if bounds.is_empty() {
            (min, max) = (Vec2::ZERO, Vec2::ONE);
        }
        let nx = (((max.x - min.x) / GRID_CELL).ceil() as usize).max(1);
        let nz = (((max.y - min.y) / GRID_CELL).ceil() as usize).max(1);
        let mut grid = BlockGrid {
            origin: min,
            nx,
            nz,
            cells: vec![Vec::new(); nx * nz],
            count: bounds.len(),
        };
        for (index, (lo, hi)) in bounds.iter().enumerate() {
            let (x0, z0, x1, z1) = grid.span(*lo, *hi);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    grid.cells[z * nx + x].push(index as u32);
                }
            }
        }
        grid
    }

    /// True if it indexes all `count` of the things (it was built after
    /// they were all in); if not, check every one.
    fn covers(&self, count: usize) -> bool {
        !self.cells.is_empty() && self.count == count
    }

    /// The cells (x0, z0, x1, z1, inclusive) a box covers, clamped to the grid.
    fn span(&self, min: Vec3, max: Vec3) -> (usize, usize, usize, usize) {
        let cell = |v: f32, origin: f32, n: usize| (((v - origin) / GRID_CELL).floor().max(0.0) as usize).min(n - 1);
        (
            cell(min.x, self.origin.x, self.nx),
            cell(min.z, self.origin.y, self.nz),
            cell(max.x, self.origin.x, self.nx),
            cell(max.z, self.origin.y, self.nz),
        )
    }

    /// The things in the cells the box from `min` to `max` touches (one may
    /// come up more than once).
    fn near(&self, min: Vec3, max: Vec3) -> impl Iterator<Item = usize> + '_ {
        let (x0, z0, x1, z1) = self.span(min, max);
        (z0..=z1).flat_map(move |z| (x0..=x1).flat_map(move |x| self.cells[z * self.nx + x].iter().map(|&i| i as usize)))
    }

    /// The nearest hit along the ray, `hit` giving the distance to thing
    /// `index` if the ray meets it nearer than `max`. Walks the cells the
    /// ray crosses (seen from above), nearest first, and stops at the first
    /// cell that holds a hit closer than its far side.
    fn raycast(&self, origin: Vec3, dir: Vec3, max: f32, mut hit: impl FnMut(usize) -> Option<f32>) -> Option<f32> {
        let size = Vec2::new(self.nx as f32, self.nz as f32) * GRID_CELL;
        let flat = Vec2::new(dir.x, dir.z);
        let start = Vec2::new(origin.x, origin.z);
        // Clip the ray to the grid.
        let (mut t0, mut t1) = (0.0_f32, max);
        for axis in 0..2 {
            let (o, d, lo, hi) = (start[axis], flat[axis], self.origin[axis], self.origin[axis] + size[axis]);
            if d.abs() < 1e-8 {
                if o < lo || o > hi {
                    return None;
                }
                continue;
            }
            let (a, b) = minmax((lo - o) / d, (hi - o) / d);
            t0 = t0.max(a);
            t1 = t1.min(b);
        }
        if t0 > t1 {
            return None;
        }
        let entry = (start + flat * t0 - self.origin) / GRID_CELL;
        let counts = [self.nx, self.nz];
        let mut cell = [
            (entry.x.floor().max(0.0) as usize).min(self.nx - 1),
            (entry.y.floor().max(0.0) as usize).min(self.nz - 1),
        ];
        let mut next = [f32::INFINITY; 2];
        let mut delta = [f32::INFINITY; 2];
        for axis in 0..2 {
            let d = flat[axis];
            if d.abs() < 1e-8 {
                continue;
            }
            delta[axis] = GRID_CELL / d.abs();
            let edge = self.origin[axis] + (cell[axis] as f32 + if d > 0.0 { 1.0 } else { 0.0 }) * GRID_CELL;
            next[axis] = (edge - start[axis]) / d;
        }
        let mut best: Option<f32> = None;
        loop {
            for &index in &self.cells[cell[1] * self.nx + cell[0]] {
                if let Some(t) = hit(index as usize)
                    && t < max
                    && best.is_none_or(|b| t < b)
                {
                    best = Some(t);
                }
            }
            let axis = if next[0] < next[1] { 0 } else { 1 };
            let leave = next[axis];
            if best.is_some_and(|b| b <= leave) || leave > t1 {
                return best;
            }
            let forward = flat[axis] > 0.0;
            if (!forward && cell[axis] == 0) || (forward && cell[axis] + 1 >= counts[axis]) {
                return best;
            }
            cell[axis] = if forward { cell[axis] + 1 } else { cell[axis] - 1 };
            next[axis] += delta[axis];
        }
    }
}

/// True if a player with feet at `feet` is close enough to grab the pickup
/// lying at `spot`.
pub fn touches_pickup(feet: Vec3, spot: Vec3) -> bool {
    (feet - spot).with_y(0.0).length() < 1.2 && (-0.5..1.5).contains(&(feet.y - spot.y))
}

pub(crate) const FLOOR: [f32; 3] = [0.20, 0.22, 0.26];
pub(crate) const WALL: [f32; 3] = [0.30, 0.33, 0.40];
pub(crate) const COVER: [f32; 3] = [0.75, 0.42, 0.18];
pub(crate) const PILLAR: [f32; 3] = [0.45, 0.48, 0.55];
pub(crate) const PLATFORM: [f32; 3] = [0.22, 0.45, 0.60];
pub(crate) const LEDGE: [f32; 3] = [0.50, 0.30, 0.60];

pub(crate) const DEPOT_FLOOR: [f32; 3] = [0.25, 0.24, 0.22];
pub(crate) const DEPOT_WALL: [f32; 3] = [0.36, 0.33, 0.30];
pub(crate) const CONCRETE: [f32; 3] = [0.52, 0.51, 0.48];
pub(crate) const CONTAINER_RED: [f32; 3] = [0.62, 0.20, 0.16];
pub(crate) const CONTAINER_BLUE: [f32; 3] = [0.16, 0.34, 0.60];
pub(crate) const CRATE: [f32; 3] = [0.62, 0.46, 0.24];
pub(crate) const BARRIER: [f32; 3] = [0.80, 0.68, 0.20];
pub(crate) const ROOF: [f32; 3] = [0.20, 0.42, 0.36];

/// Arena half size and wall height. Every map uses the same outer box.
const A: f32 = 32.0;
const H: f32 = 16.0;

const QUADRANTS: [(f32, f32); 4] = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
const CARDINALS: [(f32, f32); 4] = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)];

impl Map {
    /// A map with nothing in it yet. Fill it in, then call `finish`.
    pub fn empty(id: u8) -> Map {
        Map {
            id,
            blocks: Vec::new(),
            ramps: Vec::new(),
            abyss: -20.0,
            textures: Vec::new(),
            glows: Vec::new(),
            lamps: Vec::new(),
            terrain: None,
            bounds: [Vec2::splat(-A + 4.0), Vec2::splat(A - 4.0)],
            pads: Vec::new(),
            spawns: Vec::new(),
            hill: Vec3::ZERO,
            armor_spots: Vec::new(),
            props: Vec::new(),
            roads: Vec::new(),
            water: Vec::new(),
            flags: Vec::new(),
            bases: [Vec::new(), Vec::new()],
            course: None,
            posts: Vec::new(),
            zones: Vec::new(),
            look: Look::default(),
            grid: BlockGrid::default(),
            ramp_grid: BlockGrid::default(),
        }
    }

    /// Stands a model on the map, and adds its collision boxes as blocks and
    /// its solids as ramps. Call before `finish`.
    pub fn add_prop(&mut self, model: usize, pos: Vec3, yaw: f32, scale: f32) {
        let prop = Prop { model, pos, yaw, scale };
        for (min, max) in prop.boxes() {
            self.blocks.push(Block {
                min,
                max,
                color: [0.0; 3],
                visible: false,
            });
        }
        self.ramps.extend(prop.ramps());
        self.props.push(prop);
    }

    /// Stands a model on the map for show only: nothing collides with it.
    pub fn add_decor(&mut self, model: usize, pos: Vec3, yaw: f32, scale: f32) {
        self.props.push(Prop { model, pos, yaw, scale });
    }

    /// Where a point marked on a model (see `PropModel::points`) is once the
    /// model stands at `pos`, turned by `yaw`.
    pub fn prop_point(model: usize, name: &str, pos: Vec3, yaw: f32) -> Vec3 {
        let point = PROPS[model]
            .points
            .iter()
            .find(|(point, _)| *point == name)
            .map_or(Vec3::ZERO, |(_, at)| Vec3::from(*at));
        Quat::from_rotation_y(yaw) * point + pos
    }

    /// The spawns set aside for `role`.
    pub fn posts(&self, role: &str) -> impl Iterator<Item = &Spawn> {
        self.posts.iter().filter(move |(name, _)| name == role).map(|(_, spawn)| spawn)
    }

    /// The place called `name`, if the map has one.
    pub fn zone(&self, name: &str) -> Option<&Zone> {
        self.zones.iter().find(|(known, _)| known == name).map(|(_, zone)| zone)
    }

    /// Indexes the blocks and ramps. Call once they are all in.
    pub fn finish(mut self) -> Map {
        self.index_blocks();
        let bounds: Vec<_> = self.ramps.iter().map(|ramp| (ramp.min, ramp.max)).collect();
        self.ramp_grid = BlockGrid::new(&bounds);
        let lowest = self.blocks.iter().map(|b| b.min.y).chain(self.ramps.iter().map(|r| r.min.y)).fold(0.0_f32, f32::min);
        self.abyss = (lowest - 20.0).min(-20.0);
        self
    }

    fn index_blocks(&mut self) {
        let bounds: Vec<_> = self.blocks.iter().map(|block| (block.min, block.max)).collect();
        self.grid = BlockGrid::new(&bounds);
    }

    /// Calls `f` with every block whose cell the box from `min` to `max`
    /// touches (a block may come up more than once).
    fn near(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Block)) {
        if !self.grid.covers(self.blocks.len()) {
            self.blocks.iter().for_each(f);
            return;
        }
        self.grid.near(min, max).for_each(|index| f(&self.blocks[index]));
    }

    /// The height of the ground at (x, z), if the map has terrain.
    pub fn ground(&self, x: f32, z: f32) -> Option<f32> {
        self.terrain.as_ref().map(|terrain| terrain.height(x, z))
    }

    /// The map to play `mode` on: outside conquest, without its flagpoles
    /// (nor anything left of them to bump into).
    pub fn for_game(id: u8, mode: u8) -> Map {
        let mut map = Map::by_id(id);
        if modes::info(mode).needs != Needs::Flags {
            let boxes: Vec<(Vec3, Vec3)> =
                map.props.iter().filter(|prop| prop.model == FLAGPOLE).flat_map(Prop::boxes).collect();
            map.props.retain(|prop| prop.model != FLAGPOLE);
            map.blocks
                .retain(|block| block.visible || !boxes.contains(&(block.min, block.max)));
            map.index_blocks();
        }
        map
    }

    /// Whether the map's flagpoles are standing (see `for_game`).
    pub fn has_flagpoles(&self) -> bool {
        self.props.iter().any(|prop| prop.model == FLAGPOLE)
    }

    /// Unknown ids fall back to the first map.
    pub fn by_id(id: u8) -> Map {
        match id {
            1 => Map::depot(),
            2 => crate::valley::valley(),
            3 => crate::hamlet::hamlet(),
            4 => crate::surf::surf(),
            GAUNTLET => crate::gauntlet::gauntlet(),
            id if id >= BUILT_IN => crate::custom::map((id - BUILT_IN) as usize, id).unwrap_or_else(|| {
                // Not sent it yet: somewhere to stand meanwhile.
                let mut map = Map::arena();
                map.id = id;
                map
            }),
            _ => Map::arena(),
        }
    }

    /// Open and vertical: a central tower, corner platforms and wall ledges.
    pub fn arena() -> Map {
        let mut blocks = Vec::new();
        let mut pads = Vec::new();
        let mut spawns = Vec::new();
        shell(&mut blocks, FLOOR, WALL);
        let mut add = |min: [f32; 3], max: [f32; 3], color: [f32; 3]| {
            blocks.push(Block {
                min: Vec3::from(min),
                max: Vec3::from(max),
                color,
                visible: true,
            });
        };

        // Central tower: a pillar holding a platform, reached with the inner jump pads.
        add([-1.5, 0.0, -1.5], [1.5, 3.5, 1.5], PILLAR);
        add([-4.5, 3.5, -4.5], [4.5, 4.0, 4.5], PLATFORM);

        for (sx, sz) in QUADRANTS {
            // Low cover you can hop onto.
            let (cx, cz) = (12.0 * sx, 12.0 * sz);
            add([cx - 2.0, 0.0, cz - 2.0], [cx + 2.0, 1.2, cz + 2.0], COVER);

            // Raised corner platform with a step up to it.
            let (x0, x1) = minmax(21.0 * sx, A * sx);
            let (z0, z1) = minmax(21.0 * sz, A * sz);
            add([x0, 0.0, z0], [x1, 2.5, z1], PLATFORM);
            let (s0, s1) = minmax(18.0 * sx, 21.0 * sx);
            let (t0, t1) = minmax(24.0 * sz, 29.0 * sz);
            add([s0, 0.0, t0], [s1, 1.25, t1], COVER);

            spawns.push(spawn_facing_center(Vec3::new(27.0 * sx, 2.5, 27.0 * sz)));
            spawns.push(spawn_facing_center(Vec3::new(29.5 * sx, 2.5, 23.0 * sz)));

            // Plain pad by the tower: run across it to hop onto the platform,
            // or over the low cover.
            pads.push(JumpPad::plain(Vec3::new(7.0 * sx, 0.0, 7.0 * sz), 14.0));
        }

        for (dx, dz) in CARDINALS {
            let dir = Vec3::new(dx, 0.0, dz);
            let side = Vec3::new(dz, 0.0, dx);

            // Tall pillar that breaks the long sight lines.
            let c = dir * 18.0;
            add([c.x - 1.5, 0.0, c.z - 1.5], [c.x + 1.5, 6.0, c.z + 1.5], PILLAR);

            // High ledge along the middle of each wall.
            let a = dir * 27.5 - side * 10.0;
            let b = dir * A + side * 10.0;
            let (x0, x1) = minmax(a.x, b.x);
            let (z0, z1) = minmax(a.z, b.z);
            add([x0, 4.5, z0], [x1, 5.0, z1], LEDGE);

            // Inner pad: up onto the central tower.
            pads.push(JumpPad::arrow(dir * 8.5, -dir * 7.0 + Vec3::Y * 15.5));
            // Outer pad: up onto the wall ledge.
            pads.push(JumpPad::arrow(dir * 24.0, dir * 6.5 + Vec3::Y * 17.0));

            spawns.push(spawn_facing_center(dir * 13.5 + side * 5.0));
            spawns.push(spawn_facing_center(dir * 13.5 - side * 5.0));
        }

        // Armor: on top of the tower, and out on each wall ledge.
        let mut armor_spots = vec![Vec3::new(0.0, 4.0, 0.0)];
        armor_spots.extend(CARDINALS.map(|(dx, dz)| Vec3::new(dx, 0.0, dz) * 29.75 + Vec3::Y * 5.0));

        let map = Map {
            blocks,
            pads,
            spawns,
            armor_spots,
            ..Map::empty(0)
        };
        // Conquest: a flag at each end and one under the tower.
        with_flags(map, &[('A', -21.5, 0.0), ('B', 0.0, 6.5), ('C', 21.5, 0.0)], 7.0)
    }

    /// A cluttered freight yard: containers, crates and barriers everywhere, so
    /// you are rarely more than a few steps from cover. A roofed bunker sits in
    /// the middle.
    pub fn depot() -> Map {
        let mut blocks = Vec::new();
        let mut pads = Vec::new();
        let mut spawns = Vec::new();
        shell(&mut blocks, DEPOT_FLOOR, DEPOT_WALL);
        // A block given by the centre and half size of its footprint, and its
        // bottom and top.
        let mut add = |cx: f32, cz: f32, hx: f32, hz: f32, y0: f32, y1: f32, color: [f32; 3]| {
            blocks.push(Block {
                min: Vec3::new(cx - hx, y0, cz - hz),
                max: Vec3::new(cx + hx, y1, cz + hz),
                color,
                visible: true,
            });
        };

        // Central bunker: four pillars under a roof, with a crate inside. The
        // inner jump pads put you on the roof.
        add(0.0, 0.0, 1.0, 1.0, 0.0, 1.2, CRATE);
        add(0.0, 0.0, 5.5, 5.5, 3.0, 3.4, ROOF);

        for (sx, sz) in QUADRANTS {
            add(4.0 * sx, 4.0 * sz, 1.0, 1.0, 0.0, 3.0, CONCRETE);
            // Parapet on each roof corner.
            add(4.5 * sx, 4.5 * sz, 1.0, 1.0, 3.4, 4.2, CONCRETE);

            // Two shipping containers, too tall to see over.
            let (first, second) = if sx * sz > 0.0 {
                (CONTAINER_RED, CONTAINER_BLUE)
            } else {
                (CONTAINER_BLUE, CONTAINER_RED)
            };
            add(13.0 * sx, 8.0 * sz, 1.5, 3.5, 0.0, 2.8, first);
            add(8.0 * sx, 19.0 * sz, 3.5, 1.5, 0.0, 2.8, second);

            // Crates: stand behind the low ones, climb the stacked pair.
            add(8.5 * sx, 8.5 * sz, 1.0, 1.0, 0.0, 1.2, CRATE);
            add(16.0 * sx, 14.5 * sz, 1.0, 1.0, 0.0, 1.2, CRATE);
            add(18.0 * sx, 14.5 * sz, 1.0, 1.0, 0.0, 2.4, CRATE);
            add(27.0 * sx, 16.0 * sz, 1.0, 1.0, 0.0, 1.2, CRATE);

            // Barriers: they hide you only when you crouch.
            add(20.0 * sx, 22.0 * sz, 3.0, 0.4, 0.0, 1.3, BARRIER);
            add(23.0 * sx, 9.0 * sz, 0.4, 3.0, 0.0, 1.3, BARRIER);

            // A wall that shields the corner spawn.
            add(28.0 * sx, 24.5 * sz, 2.5, 0.4, 0.0, 2.0, CONCRETE);
            spawns.push(spawn_facing_center(Vec3::new(28.5 * sx, 0.0, 28.5 * sz)));
            spawns.push(spawn_facing_center(Vec3::new(23.5 * sx, 0.0, 28.5 * sz)));

            // Plain pad beside the near container: run at it to land on top.
            pads.push(JumpPad::plain(Vec3::new(16.5 * sx, 0.0, 8.0 * sz), 12.0));
        }

        for (dx, dz) in CARDINALS {
            let dir = Vec3::new(dx, 0.0, dz);
            let side = Vec3::new(dz, 0.0, dx);
            let (along, across) = (dir.abs(), side.abs());

            // A long wall across each lane, and a barrier closer to the middle.
            let c = dir * 21.0;
            let half = across * 5.0 + along * 0.5;
            add(c.x, c.z, half.x, half.z, 0.0, 3.5, CONCRETE);
            let c = dir * 14.5;
            let half = across * 2.5 + along * 0.4;
            add(c.x, c.z, half.x, half.z, 0.0, 1.3, BARRIER);

            // Inner pad: up onto the bunker roof.
            pads.push(JumpPad::arrow(dir * 9.5, -dir * 7.0 + Vec3::Y * 14.5));
            // Outer pad: back over the long wall, into the fight.
            pads.push(JumpPad::arrow(dir * 27.0, -dir * 7.0 + Vec3::Y * 15.0));

            spawns.push(spawn_facing_center(dir * 27.0 + side * 7.0));
            spawns.push(spawn_facing_center(dir * 27.0 - side * 7.0));
        }

        // Armor: on the bunker roof, and on top of each stacked pair of crates.
        let mut armor_spots = vec![Vec3::new(0.0, 3.4, 0.0)];
        armor_spots.extend(QUADRANTS.map(|(sx, sz)| Vec3::new(18.0 * sx, 2.4, 14.5 * sz)));

        let map = Map {
            blocks,
            pads,
            spawns,
            armor_spots,
            ..Map::empty(1)
        };
        // Conquest: a flag in each lane and one in the bunker.
        with_flags(map, &[('A', -17.5, 0.0), ('B', 0.0, 2.5), ('C', 17.5, 0.0)], 7.0)
    }

    /// Moves a box with half extents `half` by `vel * dt`, sliding along blocks
    /// and the ground. Velocity is zeroed on any axis that hits a block.
    /// Walking into a ledge no higher than `STEP_UP` climbs it. A ramp gentle
    /// enough to walk is stood on like the ground; against a steeper one only
    /// the velocity into it is lost, so you slide along it (surfing).
    /// Returns true if the box ended up standing on something.
    pub fn move_and_slide(&self, pos: &mut Vec3, vel: &mut Vec3, half: Vec3, dt: f32) -> bool {
        let total = *vel * dt;
        let steps = (total.abs().max_element() / 0.25).ceil().clamp(1.0, 32.0) as usize;
        let sub_dt = dt / steps as f32;
        let mut grounded = false;
        for _ in 0..steps {
            // From the velocity left after the last part, which a ramp may have turned.
            let step = *vel * sub_dt;
            // Horizontal axes first, so that walking off a ledge behaves sensibly.
            for axis in [0, 2, 1] {
                let delta = step[axis];
                if delta == 0.0 || vel[axis] == 0.0 {
                    continue;
                }
                pos[axis] += delta;
                let reach = Vec3::splat(0.01);
                let (mut moved, mut hit) = (*pos, false);
                self.near(*pos - half - reach, *pos + half + reach, |block| {
                    if !overlaps(moved, half, block) {
                        return;
                    }
                    if axis != 1 {
                        let rise = block.max.y - (moved.y - half.y);
                        if rise > 0.0 && rise <= STEP_UP {
                            let raised = moved.with_y(block.max.y + half.y + 1e-3);
                            if self.is_free(raised, half) {
                                moved = raised;
                                return;
                            }
                        }
                    }
                    if delta > 0.0 {
                        moved[axis] = block.min[axis] - half[axis];
                    } else {
                        moved[axis] = block.max[axis] + half[axis];
                        if axis == 1 {
                            grounded = true;
                        }
                    }
                    hit = true;
                });
                *pos = moved;
                if hit {
                    vel[axis] = 0.0;
                }
            }
            grounded |= self.slide_off_ramps(pos, vel, half);
            grounded |= self.slide_off_ground(pos, vel, half);
        }
        grounded
    }

    /// The ground holds up the middle of the box. Where it is gentle enough
    /// to walk, the box is lifted straight out of it and stands; where it
    /// is steeper, like a steep ramp, the box is pushed square out of it and
    /// only loses the velocity into it, so it slides down (or surfs it).
    /// Returns true if it stands.
    fn slide_off_ground(&self, pos: &mut Vec3, vel: &mut Vec3, half: Vec3) -> bool {
        let Some(terrain) = &self.terrain else {
            return false;
        };
        // A few times over, for where the box slides onto another slope.
        for _ in 0..3 {
            let depth = terrain.height(pos.x, pos.z) - (pos.y - half.y);
            if depth < -1e-3 {
                return false;
            }
            let normal = terrain.normal(pos.x, pos.z);
            if normal.y >= WALKABLE {
                pos.y += depth.max(0.0);
                if vel.y > 0.0 {
                    return false;
                }
                vel.y = 0.0;
                return true;
            }
            // Square out of the slope, as far as the middle of the box is in it.
            *pos += normal * depth.max(0.0) * normal.y;
            let into = vel.dot(normal);
            if into < 0.0 {
                *vel -= normal * into;
            }
            if depth <= 1e-4 {
                return false;
            }
        }
        // Still in it: wedged in a fold between steep slopes. It rests there.
        let ground = terrain.height(pos.x, pos.z);
        if pos.y - half.y < ground - 1e-3 {
            pos.y = ground + half.y;
            vel.y = vel.y.max(0.0);
            return true;
        }
        false
    }

    /// True if the ground under (x, z) is gentle enough to stand on.
    pub fn walkable_ground(&self, x: f32, z: f32) -> bool {
        self.terrain.as_ref().is_some_and(|terrain| terrain.normal(x, z).y >= WALKABLE)
    }

    /// The first ramp a box at `center` overlaps: which way to push it out, and how far.
    fn ramp_contact(&self, center: Vec3, half: Vec3) -> Option<(Vec3, f32)> {
        if !self.ramp_grid.covers(self.ramps.len()) {
            return self.ramps.iter().find_map(|ramp| ramp.push_out(center, half));
        }
        self.ramp_grid.near(center - half, center + half).find_map(|index| self.ramps[index].push_out(center, half))
    }

    /// Takes a box out of the ramps it went into. Off a walkable face it is
    /// lifted straight up and stands, as on the ground, so it does not slide
    /// down a gentle slope; off a steep one it is pushed square out of it and
    /// keeps all its speed along the face. Returns true if it stands.
    fn slide_off_ramps(&self, pos: &mut Vec3, vel: &mut Vec3, half: Vec3) -> bool {
        let mut grounded = false;
        // A few times over, for where two ramps meet.
        for _ in 0..4 {
            let Some((out, depth)) = self.ramp_contact(*pos, half) else {
                break;
            };
            if out.y >= WALKABLE {
                pos.y += depth / out.y;
                if vel.y <= 0.0 {
                    vel.y = 0.0;
                    grounded = true;
                }
            } else if let Some(stood) = self.step_onto_ramp(*pos, half) {
                pos.y = stood;
                if vel.y <= 0.0 {
                    vel.y = 0.0;
                    grounded = true;
                }
            } else {
                *pos += out * depth;
                let into = vel.dot(out);
                if into < 0.0 {
                    *vel -= out * into;
                }
            }
        }
        grounded
    }

    /// Walking into a steep side of a ramp whose top is no higher than
    /// `STEP_UP` above your feet (the side of a flight of stairs, the edge of
    /// a rock) lifts you onto it, as onto a ledge, if what you would stand
    /// on there is walkable. Where the box's centre ends up, if it does.
    fn step_onto_ramp(&self, pos: Vec3, half: Vec3) -> Option<f32> {
        let stood = self.ramp_floor(pos + Vec3::Y * STEP_UP, half, STEP_UP)? + 1e-3;
        (stood > pos.y && self.is_free(pos.with_y(stood), half)).then_some(stood)
    }

    /// The first ramp a box clear of them at `center` would touch moving
    /// along `dir`, at most `max`: how far it gets, and the ramp's normal there.
    fn ramp_sweep(&self, center: Vec3, half: Vec3, dir: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let hit = |ramp: &Ramp| ramp.sweep(center, half, dir, max);
        let nearest = |a: &(f32, Vec3), b: &(f32, Vec3)| a.0.total_cmp(&b.0);
        if !self.ramp_grid.covers(self.ramps.len()) {
            return self.ramps.iter().filter_map(hit).min_by(nearest);
        }
        let end = center + dir * max;
        self.ramp_grid
            .near(center.min(end) - half, center.max(end) + half)
            .filter_map(|index| hit(&self.ramps[index]))
            .min_by(nearest)
    }

    /// Where a box would stand on a walkable ramp face under it, at most
    /// `drop` down: the height of the box's centre.
    fn ramp_floor(&self, pos: Vec3, half: Vec3, drop: f32) -> Option<f32> {
        let (fall, normal) = self.ramp_sweep(pos, half, Vec3::NEG_Y, drop)?;
        (normal.y >= WALKABLE).then_some(pos.y - fall)
    }

    /// Puts a box that was standing on something last frame back down on
    /// what is under it, if that is at most `drop` below its feet: walking
    /// down a slope or a stair, you stay on the ground instead of skipping
    /// off it. Returns true if it did.
    pub fn snap_to_ground(&self, pos: &mut Vec3, half: Vec3, drop: f32) -> bool {
        let feet = pos.y - half.y;
        // Not down a slope too steep to stand on: you slide off that.
        let mut floor = self
            .ground(pos.x, pos.z)
            .filter(|ground| *ground <= feet + 1e-3 && self.walkable_ground(pos.x, pos.z));
        let (min, max) = (*pos - half.with_y(0.0) - Vec3::Y * (half.y + drop), *pos + half);
        let center = *pos;
        self.near(min, max, |block| {
            let under = center.x + half.x > block.min.x
                && center.x - half.x < block.max.x
                && center.z + half.z > block.min.z
                && center.z - half.z < block.max.z;
            if under && block.max.y <= feet + 1e-3 && floor.is_none_or(|f| block.max.y > f) {
                floor = Some(block.max.y);
            }
        });
        if let Some(ramp) = self.ramp_floor(*pos, half, drop).map(|center| center - half.y)
            && floor.is_none_or(|f| ramp > f)
        {
            floor = Some(ramp);
        }
        let Some(floor) = floor.filter(|floor| feet - floor <= drop) else {
            return false;
        };
        let snapped = pos.with_y(floor + half.y);
        if !self.is_free(snapped, half) {
            return false;
        }
        *pos = snapped;
        true
    }

    /// True if a box with half extents `half` fits at `center` without
    /// touching a block or a ramp, with the middle of its bottom above the ground.
    pub fn is_free(&self, center: Vec3, half: Vec3) -> bool {
        if self.ground(center.x, center.z).is_some_and(|ground| center.y - half.y < ground - 0.02) {
            return false;
        }
        self.overlapping(center, half).is_none() && self.ramp_contact(center, half).is_none()
    }

    /// The first block a box at `center` overlaps, if any.
    fn overlapping(&self, center: Vec3, half: Vec3) -> Option<&Block> {
        if !self.grid.covers(self.blocks.len()) {
            return self.blocks.iter().find(|block| overlaps(center, half, block));
        }
        self.grid.near(center - half, center + half).map(|index| &self.blocks[index]).find(|block| overlaps(center, half, block))
    }

    /// Advances a grenade by one `GRENADE_STEP`: it falls, bounces off blocks
    /// and the ground, and rolls to a stop. Returns the speed of its hardest
    /// contact, 0 if it touched nothing (above `BOUNCE_SOUND` it can be heard).
    pub fn step_grenade(&self, pos: &mut Vec3, vel: &mut Vec3, dt: f32) -> f32 {
        const RESTITUTION: f32 = 0.42;
        /// Share of the sideways speed kept through each contact with a wall...
        const GRIP: f32 = 0.84;
        /// ...and with the floor, which it digs into rather than skating on.
        const GROUND_GRIP: f32 = 0.4;
        const GROUND_RESTITUTION: f32 = 0.35;
        let half = Vec3::splat(GRENADE_HALF);
        vel.y -= GRENADE_GRAVITY * dt;
        let steps = (vel.length() * dt / 0.1).ceil().clamp(1.0, 64.0);
        let sub_dt = dt / steps;
        let mut hardest: f32 = 0.0;
        for _ in 0..steps as usize {
            for axis in [0, 2, 1] {
                let delta = vel[axis] * sub_dt;
                if delta == 0.0 {
                    continue;
                }
                pos[axis] += delta;
                let Some(block) = self.overlapping(*pos, half) else {
                    continue;
                };
                pos[axis] = if delta > 0.0 {
                    block.min[axis] - half[axis]
                } else {
                    block.max[axis] + half[axis]
                };
                let impact = vel[axis].abs();
                let (grip, restitution) = if axis == 1 { (GROUND_GRIP, GROUND_RESTITUTION) } else { (GRIP, RESTITUTION) };
                *vel *= grip;
                // Too slow to bounce: it stays put on that axis (resting on the floor).
                vel[axis] = if impact > 1.5 { -vel[axis] * restitution / grip } else { 0.0 };
                hardest = hardest.max(impact);
            }
            if let Some((normal, depth)) = self.ramp_contact(*pos, half) {
                *pos += normal * depth;
                let into = vel.dot(normal);
                if into < 0.0 {
                    let impact = -into;
                    let along = *vel - normal * into;
                    let bounce = if impact > 1.5 { normal * impact * RESTITUTION } else { Vec3::ZERO };
                    // It comes to rest on a walkable slope, and slides down a steep one.
                    let grip = if normal.y >= WALKABLE {
                        GROUND_GRIP
                    } else if impact > 1.5 {
                        GRIP
                    } else {
                        1.0
                    };
                    *vel = along * grip + bounce;
                    hardest = hardest.max(impact);
                }
            }
            let Some(terrain) = &self.terrain else {
                continue;
            };
            let ground = terrain.height(pos.x, pos.z);
            if pos.y - half.y >= ground {
                continue;
            }
            pos.y = ground + half.y;
            let normal = terrain.normal(pos.x, pos.z);
            let into = vel.dot(normal);
            if into < 0.0 {
                let impact = -into;
                let along = *vel - normal * into;
                *vel = along * GROUND_GRIP + if impact > 1.5 { normal * impact * GROUND_RESTITUTION } else { Vec3::ZERO };
                hardest = hardest.max(impact);
            }
        }
        hardest
    }

    /// Returns the jump pad at the player's `feet`, if any.
    pub fn pad_at(&self, feet: Vec3) -> Option<&JumpPad> {
        self.pads.iter().find(|pad| {
            (feet.x - pad.center.x).abs() < pad.half_size
                && (feet.z - pad.center.z).abs() < pad.half_size
                && (feet.y - pad.center.y).abs() < 0.3
        })
    }

    /// Distance along the ray to the closest block, ramp or the ground, if it
    /// is nearer than `max`.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let block = self.raycast_blocks(origin, dir, max);
        let ramp = self.raycast_ramps(origin, dir, block.unwrap_or(max));
        let block = ramp.or(block);
        let ground = self
            .terrain
            .as_ref()
            .and_then(|terrain| terrain.raycast(origin, dir, block.unwrap_or(max)));
        ground.or(block)
    }

    /// The nearest block along the ray.
    fn raycast_blocks(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let hit = |block: &Block| ray_box(origin, dir, block.min, block.max).filter(|t| *t < max);
        if !self.grid.covers(self.blocks.len()) {
            return self.blocks.iter().filter_map(hit).min_by(f32::total_cmp);
        }
        self.grid.raycast(origin, dir, max, |index| hit(&self.blocks[index]))
    }

    /// The nearest ramp along the ray.
    fn raycast_ramps(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let hit = |ramp: &Ramp| ramp.raycast(origin, dir, max);
        if !self.ramp_grid.covers(self.ramps.len()) {
            return self.ramps.iter().filter_map(hit).min_by(f32::total_cmp);
        }
        self.ramp_grid.raycast(origin, dir, max, |index| hit(&self.ramps[index]))
    }

    /// True if no block is in the way between the two points.
    pub fn line_of_sight(&self, from: Vec3, to: Vec3) -> bool {
        let distance = from.distance(to);
        distance < 1e-3 || self.raycast(from, (to - from) / distance, distance).is_none()
    }

    /// Seconds somebody with their eyes at `eye`, looking along `look`, is
    /// blinded by a flashbang going off at `flash`. Looking right at it is
    /// worst; with your back to it, it still dazzles a little.
    pub fn flash_blindness(&self, flash: Vec3, eye: Vec3, look: Vec3) -> f32 {
        let flash = flash + Vec3::Y * 0.1;
        let to = flash - eye;
        let distance = to.length();
        if distance >= FLASH_RANGE || !self.line_of_sight(flash, eye) {
            return 0.0;
        }
        let facing = if distance < 1.5 { 1.0 } else { look.dot(to / distance) };
        let facing = 0.15 + 0.85 * ((facing + 0.2) / 0.9).clamp(0.0, 1.0);
        let near = 1.0 - (distance / FLASH_RANGE).powi(2);
        FLASH_BLIND * near * facing
    }
}

/// The middle of the smoke cloud from a smoke grenade gone off at `pos`.
pub fn smoke_center(pos: Vec3) -> Vec3 {
    pos + Vec3::Y * SMOKE_RISE
}

/// True if a smoke cloud centred at `center` hides `to` from `from`.
pub fn smoke_blocks(center: Vec3, from: Vec3, to: Vec3) -> bool {
    let line = to - from;
    let t = ((center - from).dot(line) / line.length_squared().max(1e-6)).clamp(0.0, 1.0);
    // A little smaller than it looks: its edges are thin.
    (from + line * t).distance(center) < SMOKE_RADIUS * 0.85
}

/// Floor, the four walls and an invisible ceiling.
fn shell(blocks: &mut Vec<Block>, floor: [f32; 3], wall: [f32; 3]) {
    let mut add = |min: [f32; 3], max: [f32; 3], color: [f32; 3], visible: bool| {
        blocks.push(Block {
            min: Vec3::from(min),
            max: Vec3::from(max),
            color,
            visible,
        });
    };
    add([-A - 2.0, -2.0, -A - 2.0], [A + 2.0, 0.0, A + 2.0], floor, true);
    add([-A - 2.0, 0.0, -A - 2.0], [-A, H, A + 2.0], wall, true);
    add([A, 0.0, -A - 2.0], [A + 2.0, H, A + 2.0], wall, true);
    add([-A, 0.0, -A - 2.0], [A, H, -A], wall, true);
    add([-A, 0.0, A], [A, H, A + 2.0], wall, true);
    add([-A - 2.0, 30.0, -A - 2.0], [A + 2.0, 32.0, A + 2.0], wall, false);
}

pub fn minmax(a: f32, b: f32) -> (f32, f32) {
    (a.min(b), a.max(b))
}

/// Finishes one of the small maps, with conquest flags (name, x, z) on the
/// floor, each with its pole (to look at: the arenas are tight enough), and
/// the spawns either side of x = 0 as the teams' bases.
fn with_flags(map: Map, flags: &[(char, f32, f32)], radius: f32) -> Map {
    let mut map = map.finish();
    for &(_, x, z) in flags {
        map.add_decor(FLAGPOLE, Vec3::new(x, 0.0, z), 0.0, 1.0);
    }
    map.flags = flags
        .iter()
        .map(|&(name, x, z)| FlagPoint {
            name,
            pos: Vec3::new(x, 0.0, z),
            radius,
            spawns: spawn_ring(&map, Vec2::new(x, z), 2.5, radius - 0.5, TEAM_SIZE),
        })
        .collect();
    for (team, west) in [(0, true), (1, false)] {
        map.bases[team] = map
            .spawns
            .iter()
            .filter(|spawn| (spawn.pos.x < 0.0) == west)
            .map(|spawn| Spawn { pos: spawn.pos, yaw: spawn.yaw })
            .collect();
    }
    map
}

/// Up to `count` places to spawn between `inner` and `outer` metres from
/// `center`, on the ground or floor, facing the middle of the map.
pub fn spawn_ring(map: &Map, center: Vec2, inner: f32, outer: f32, count: usize) -> Vec<Spawn> {
    let mut spawns: Vec<Spawn> = Vec::new();
    let rings = 4;
    for ring in 0..rings {
        let radius = inner + (outer - inner) * ring as f32 / (rings - 1) as f32;
        for i in 0..12 {
            if spawns.len() >= count {
                return spawns;
            }
            let angle = (i as f32 + ring as f32 * 0.5) * std::f32::consts::PI / 6.0;
            let at = center + Vec2::from_angle(angle) * radius;
            let feet = Vec3::new(at.x, map.ground(at.x, at.y).unwrap_or(0.0), at.y);
            let pos = feet + Vec3::Y * (PLAYER_HALF.y + 0.05);
            let crowded = spawns.iter().any(|other| other.pos.distance(pos) < 3.0);
            if !crowded && map.is_free(pos, PLAYER_HALF) && map.is_free(pos + Vec3::Y * 0.5, PLAYER_HALF) {
                spawns.push(spawn_facing_center(feet));
            }
        }
    }
    spawns
}

/// `feet` is a point on the ground; the spawn is the box centre standing there.
pub fn spawn_facing_center(feet: Vec3) -> Spawn {
    Spawn {
        pos: feet + Vec3::Y * (PLAYER_HALF.y + 0.05),
        yaw: feet.x.atan2(feet.z),
    }
}

fn overlaps(center: Vec3, half: Vec3, block: &Block) -> bool {
    const EPS: f32 = 1e-4;
    center.x + half.x > block.min.x + EPS
        && center.x - half.x < block.max.x - EPS
        && center.y + half.y > block.min.y + EPS
        && center.y - half.y < block.max.y - EPS
        && center.z + half.z > block.min.z + EPS
        && center.z - half.z < block.max.z - EPS
}

/// Slab test. Returns the entry distance (0 if the origin is inside the box).
pub fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut t_near = 0.0_f32;
    let mut t_far = f32::INFINITY;
    for axis in 0..3 {
        if dir[axis].abs() < 1e-8 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / dir[axis];
        let (t0, t1) = minmax((min[axis] - origin[axis]) * inv, (max[axis] - origin[axis]) * inv);
        t_near = t_near.max(t0);
        t_far = t_far.min(t1);
        if t_near > t_far {
            return None;
        }
    }
    Some(t_near)
}

/// Forward direction for a yaw/pitch pair (yaw 0 looks down -Z).
pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{MAX_PLAYERS, MODE_CONQUEST};

    #[test]
    fn the_head_is_at_eye_height() {
        for (crouching, eye) in [(false, STAND_EYE), (true, CROUCH_EYE)] {
            let center = Vec3::new(3.0, 5.0, -2.0);
            let feet = center.y - player_half(crouching).y;
            let (min, max) = head_box(center, crouching);
            let shot_at = |y: f32| ray_box(Vec3::new(3.0, y, 10.0), Vec3::NEG_Z, min, max).is_some();
            assert!(shot_at(feet + eye), "crouching: {crouching}");
            assert!(!shot_at(center.y - 0.1), "crouching: {crouching}");
            assert!(max.y <= center.y + hitbox_half(crouching).y + 1e-4);
        }
    }

    fn maps() -> Vec<Map> {
        (0..MAP_NAMES.len() as u8).map(Map::by_id).collect()
    }

    #[test]
    fn armor_lies_on_something_a_player_can_stand_in() {
        for map in maps() {
            // Nobody gets hurt on a course.
            assert_eq!(map.armor_spots.is_empty(), map.course.is_some(), "map {}", map.id);
            assert!(map.armor_spots.len() <= 32, "map {}", map.id);
            for spot in &map.armor_spots {
                let standing = *spot + Vec3::Y * (PLAYER_HALF.y + 0.01);
                assert!(map.is_free(standing, PLAYER_HALF), "map {} armor at {spot} is inside a block", map.id);
                assert!(!map.is_free(*spot - Vec3::Y * 0.05, Vec3::splat(0.04)), "map {} armor at {spot} floats", map.id);
                assert!(touches_pickup(*spot, *spot));
            }
        }
    }

    #[test]
    fn every_conquest_map_has_flags_and_bases() {
        for map in maps().into_iter().filter(|map| plays(map.id, MODE_CONQUEST)) {
            assert!(map.flags.len() >= 3, "map {}", map.id);
            for flag in &map.flags {
                assert!(flag.spawns.len() >= 3, "map {} flag {} has {} spawns", map.id, flag.name, flag.spawns.len());
                for spawn in &flag.spawns {
                    assert!(map.is_free(spawn.pos, PLAYER_HALF));
                    assert!(flag.holds(spawn.pos) || flag.radius > 8.0, "map {} flag {}: spawn off the flag", map.id, flag.name);
                }
            }
            for base in &map.bases {
                assert!(base.len() >= 4, "map {}", map.id);
            }
        }
    }

    #[test]
    fn flagpoles_stand_only_in_conquest() {
        for id in (0..MAP_NAMES.len() as u8).filter(|id| plays(*id, MODE_CONQUEST)) {
            let conquest = Map::for_game(id, MODE_CONQUEST);
            let other = Map::for_game(id, 0);
            assert!(conquest.has_flagpoles(), "map {id}");
            assert!(!other.has_flagpoles(), "map {id}");
            // The poles took their boxes with them, and nothing else.
            let boxes: Vec<_> = conquest.props.iter().filter(|prop| prop.model == FLAGPOLE).flat_map(Prop::boxes).collect();
            let solid = conquest.blocks.iter().filter(|block| boxes.contains(&(block.min, block.max))).count();
            assert_eq!(conquest.blocks.len() - other.blocks.len(), solid, "map {id}");
            assert!(other.blocks.iter().all(|block| !boxes.contains(&(block.min, block.max))), "map {id}");
        }
    }

    #[test]
    fn ids_match_the_name_table() {
        for (index, map) in maps().iter().enumerate() {
            assert_eq!(map.id as usize, index);
        }
    }

    #[test]
    fn spawns_are_clear_and_on_the_ground() {
        for map in maps() {
            assert!(map.spawns.len() >= 8);
            for spawn in &map.spawns {
                assert!(map.is_free(spawn.pos, PLAYER_HALF), "map {} spawn {:?}", map.id, spawn.pos);
                let (mut pos, mut vel) = (spawn.pos, Vec3::new(0.0, -5.0, 0.0));
                assert!(map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, 0.1));
                assert!(spawn.pos.y - pos.y < 0.1, "map {} spawn {:?} floats", map.id, spawn.pos);
            }
        }
    }

    /// Closer than this, two players' boxes touch (with a little to spare).
    const SPAWN_SPACING: f32 = 2.0 * PLAYER_HALF.x + 0.7;

    /// No two points of `spawns` so close that players coming in at both
    /// would stand in each other.
    fn spread_out(spawns: &[Spawn]) -> Option<(Vec3, Vec3)> {
        spawns.iter().enumerate().find_map(|(i, a)| {
            spawns[i + 1..]
                .iter()
                .find(|b| (a.pos - b.pos).with_y(0.0).length() < SPAWN_SPACING && (a.pos.y - b.pos.y).abs() < 2.0 * PLAYER_HALF.y)
                .map(|b| (a.pos, b.pos))
        })
    }

    #[test]
    fn every_map_has_a_spawn_for_everybody() {
        for map in maps() {
            assert!(map.spawns.len() >= MAX_PLAYERS, "map {} has {} spawns", map.id, map.spawns.len());
            assert_eq!(spread_out(&map.spawns), None, "map {}: spawns on top of each other", map.id);
            for spawn in &map.spawns {
                let ahead = map.raycast(spawn.pos + Vec3::Y * 0.7, look_dir(spawn.yaw, 0.0), 3.0);
                assert!(ahead.is_none(), "map {} spawn {:?} faces a wall", map.id, spawn.pos);
            }
            if !plays(map.id, MODE_CONQUEST) {
                continue;
            }
            for (team, base) in map.bases.iter().enumerate() {
                assert!(base.len() >= TEAM_SIZE, "map {} base {team} has {}", map.id, base.len());
                assert_eq!(spread_out(base), None, "map {} base {team}", map.id);
            }
            for flag in &map.flags {
                assert!(flag.spawns.len() >= TEAM_SIZE, "map {} flag {} has {}", map.id, flag.name, flag.spawns.len());
                assert_eq!(spread_out(&flag.spawns), None, "map {} flag {}", map.id, flag.name);
            }
        }
    }

    #[test]
    fn pads_are_not_blocked() {
        for map in maps() {
            for pad in &map.pads {
                let standing = pad.center + Vec3::Y * PLAYER_HALF.y;
                assert!(map.is_free(standing, PLAYER_HALF), "map {} pad {:?}", map.id, pad.center);
                assert!(map.pad_at(pad.center).is_some());
            }
        }
    }

    /// Follows a player launched by each pad and checks where they come down.
    #[test]
    fn depot_pads_land_where_intended() {
        let map = Map::depot();
        for pad in map.pads.iter().filter(|pad| pad.direction().is_some()) {
            let mut pos = pad.center + Vec3::Y * PLAYER_HALF.y;
            let mut vel = pad.launch;
            let mut landed = false;
            for _ in 0..600 {
                vel.y -= 22.0 / 120.0;
                if map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, 1.0 / 120.0) {
                    landed = true;
                    break;
                }
            }
            assert!(landed);
            let feet = pos.y - PLAYER_HALF.y;
            let from_center = pos.with_y(0.0).length();
            if pad.center.length() < 15.0 {
                // Inner pads: onto the bunker roof.
                assert!((feet - 3.4).abs() < 0.01 && from_center < 5.5, "landed at {pos:?}");
            } else {
                // Outer pads: over the long wall, onto the ground behind it.
                assert!(feet.abs() < 0.01 && from_center < 20.0, "landed at {pos:?}");
            }
        }
    }

    #[test]
    fn every_map_with_pads_has_both_kinds() {
        for map in maps().into_iter().filter(|map| !map.pads.is_empty()) {
            assert!(map.pads.iter().any(|pad| pad.direction().is_some()), "map {}", map.id);
            assert!(map.pads.iter().any(|pad| pad.direction().is_none()), "map {}", map.id);
        }
    }

    #[test]
    fn plain_pads_keep_your_direction() {
        let pad = JumpPad::plain(Vec3::ZERO, 14.0);
        let vel = pad.launch_velocity(Vec3::new(9.0, -3.0, -4.0));
        assert_eq!(vel, Vec3::new(9.0, 14.0, -4.0));
        let arrow = JumpPad::arrow(Vec3::ZERO, Vec3::new(7.0, 15.0, 0.0));
        assert_eq!(arrow.direction(), Some(Vec3::X));
        assert_eq!(arrow.launch_velocity(Vec3::ZERO), arrow.launch);
    }

    #[test]
    fn a_flash_blinds_most_when_looked_at() {
        let map = Map::arena();
        let eye = map.spawns[0].pos + Vec3::Y * 0.7;
        let look = look_dir(map.spawns[0].yaw, 0.0);
        let flash = eye + look * 6.0 - Vec3::Y * 1.0;
        assert!(map.line_of_sight(flash + Vec3::Y * 0.1, eye), "no open space in front of the spawn");
        let facing = map.flash_blindness(flash, eye, look);
        let away = map.flash_blindness(flash, eye, -look);
        assert!(facing > 2.5, "facing {facing}");
        assert!(away > 0.0 && away < facing * 0.3, "away {away}");
        // Out of range, nothing.
        assert_eq!(map.flash_blindness(eye + look * (FLASH_RANGE + 1.0), eye, look), 0.0);
    }

    #[test]
    fn smoke_hides_only_what_is_behind_it() {
        let center = smoke_center(Vec3::ZERO);
        let from = Vec3::new(-20.0, 1.6, 0.0);
        assert!(smoke_blocks(center, from, Vec3::new(20.0, 1.6, 0.0)));
        // Well clear of the cloud, on either side.
        assert!(!smoke_blocks(center, from, Vec3::new(-10.0, 1.6, 0.0)));
        assert!(!smoke_blocks(center, from, Vec3::new(20.0, 1.6, 12.0)));
    }

    #[test]
    fn grenades_bounce_and_come_to_rest() {
        let map = Map::arena();
        let mut pos = Vec3::new(10.0, 2.0, 0.0);
        let mut vel = Vec3::new(6.0, 4.0, 3.0);
        let mut bounces = 0;
        for _ in 0..600 {
            bounces += (map.step_grenade(&mut pos, &mut vel, 1.0 / 60.0) > BOUNCE_SOUND) as u32;
        }
        assert!(bounces >= 1);
        assert!((pos.y - GRENADE_HALF).abs() < 1e-3, "resting height {}", pos.y);
        assert!(vel.length() < 0.5, "still moving at {vel:?}");
    }

    /// Flat ground for x < 0, then a 56° slope rising 1.5 m per metre.
    fn cliff() -> Map {
        Map {
            terrain: Some(Terrain::generate(41, 2.0, |x, _| (x * 1.5).max(0.0))),
            ..Map::empty(0)
        }
        .finish()
    }

    /// Flat ground for x < 0, then a hill rising 0.4 m per metre.
    fn hillside() -> Map {
        Map {
            terrain: Some(Terrain::generate(41, 2.0, |x, _| (x * 0.4).max(0.0))),
            ..Map::empty(0)
        }
        .finish()
    }

    /// Runs like a player at a steady `run` speed for `frames` 60 Hz frames,
    /// snapping to the ground as the player does. Returns the frames spent
    /// off the ground.
    fn run_for(map: &Map, pos: &mut Vec3, run: Vec3, frames: u32) -> u32 {
        let dt = 1.0 / 60.0;
        let (mut vel, mut grounded, mut airborne) = (run, true, 0);
        for _ in 0..frames {
            vel = Vec3::new(run.x, vel.y - 22.0 * dt, run.z);
            let was = grounded;
            grounded = map.move_and_slide(pos, &mut vel, PLAYER_HALF, dt);
            if was && !grounded && vel.y <= 0.0 && map.snap_to_ground(pos, PLAYER_HALF, STEP_UP.max(run.length() * dt)) {
                grounded = true;
                vel.y = 0.0;
            }
            airborne += !grounded as u32;
        }
        airborne
    }

    #[test]
    fn walking_over_a_hill_keeps_you_on_the_ground() {
        let map = hillside();
        let mut pos = Vec3::new(-10.0, PLAYER_HALF.y, 3.0);
        // Up the slope...
        assert_eq!(run_for(&map, &mut pos, Vec3::new(9.5, 0.0, 0.0), 180), 0);
        let feet = pos.y - PLAYER_HALF.y;
        assert!((feet - map.ground(pos.x, pos.z).unwrap()).abs() < 0.01, "feet {feet} at {pos}");
        assert!(feet > 4.0);
        // ...and back down it, without skipping off.
        assert_eq!(run_for(&map, &mut pos, Vec3::new(-9.5, 0.0, 0.0), 180), 0);
        assert!((pos.y - PLAYER_HALF.y).abs() < 0.01, "{pos}");
    }

    /// Ground steeper than about 45° is no floor: a body on it slides to the
    /// bottom without ever standing (and never sinks into it).
    #[test]
    fn steep_ground_is_slid_down_not_stood_on() {
        let map = cliff();
        assert!(!map.walkable_ground(5.0, 0.0) && map.walkable_ground(-5.0, 0.0));
        let dt = 1.0 / 60.0;
        let mut pos = Vec3::new(6.0, 9.0 + PLAYER_HALF.y, 0.0);
        let mut vel = Vec3::ZERO;
        for frame in 0..180 {
            vel.y -= 22.0 * dt;
            let grounded = map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, dt);
            let feet = pos.y - PLAYER_HALF.y;
            assert!(feet >= map.ground(pos.x, pos.z).unwrap() - 0.02, "in the slope at {pos}");
            if pos.x > 0.5 {
                assert!(!grounded, "stood on the slope at {pos}, frame {frame}");
            }
        }
        assert!(pos.x < 0.5 && (pos.y - PLAYER_HALF.y).abs() < 0.05, "came to rest at {pos}");
    }

    #[test]
    fn low_ledges_are_climbed_and_high_ones_block() {
        for (height, climbs) in [(0.3, true), (0.5, true), (0.7, false)] {
            let mut map = Map::empty(0);
            shell(&mut map.blocks, FLOOR, WALL);
            map.blocks.push(Block {
                min: Vec3::new(2.0, 0.0, -2.0),
                max: Vec3::new(4.0, height, 2.0),
                color: COVER,
                visible: true,
            });
            let map = map.finish();
            let mut pos = Vec3::new(0.0, PLAYER_HALF.y, 0.0);
            run_for(&map, &mut pos, Vec3::new(6.0, 0.0, 0.0), 30);
            let feet = pos.y - PLAYER_HALF.y;
            if climbs {
                assert!((feet - height).abs() < 0.01 && pos.x > 2.0, "height {height}: {pos}");
            } else {
                assert!(feet.abs() < 0.01 && pos.x < 2.0, "height {height}: {pos}");
            }
        }
    }

    /// A house's roof is a smooth slope: you walk up it, over the ridge and
    /// down the other side without leaving it, however the house is turned.
    #[test]
    fn roofs_are_walked_up_and_over() {
        for yaw in [0.0, std::f32::consts::FRAC_PI_2] {
            let mut map = Map::empty(0);
            map.add_prop(crate::props::HOUSE, Vec3::ZERO, yaw, 1.0);
            let map = map.finish();
            assert!(map.ramps.iter().all(|ramp| !ramp.visible));
            // The house's ridge runs along its x, 5.48 m up; its slopes drop
            // 1 m for every 1.43 m out. Clear of the chimney (at +x).
            let turn = Quat::from_rotation_y(yaw);
            let across = turn * Vec3::Z;
            let mut pos = turn * Vec3::new(-2.5, 9.0, -2.6);
            let mut vel = Vec3::ZERO;
            for _ in 0..120 {
                vel.y -= 22.0 / 60.0;
                if map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, 1.0 / 60.0) {
                    break;
                }
            }
            let feet = |pos: Vec3| pos.y - PLAYER_HALF.y;
            assert!(feet(pos) > 3.5 && feet(pos) < 4.5, "landed at {pos}");
            assert_eq!(run_for(&map, &mut pos, across * 3.0, 52), 0, "yaw {yaw}: up to {pos}");
            assert!((feet(pos) - 5.48).abs() < 0.05, "yaw {yaw}: on the ridge at {pos}");
            assert_eq!(run_for(&map, &mut pos, across * 3.0, 52), 0, "yaw {yaw}: down to {pos}");
            let expected = 5.48 - (2.6 - PLAYER_HALF.z) / 1.429;
            assert!((feet(pos) - expected).abs() < 0.05, "yaw {yaw}: down the far side at {pos}");
        }
    }

    #[test]
    fn stairs_go_up_and_come_back_down() {
        let mut map = Map::empty(0);
        shell(&mut map.blocks, FLOOR, WALL);
        for step in 0..8 {
            let x = 2.0 + step as f32 * 0.4;
            map.blocks.push(Block {
                min: Vec3::new(x, 0.0, -1.0),
                max: Vec3::new(x + 0.4, 0.35 * (step + 1) as f32, 1.0),
                color: COVER,
                visible: true,
            });
        }
        let map = map.finish();
        let mut pos = Vec3::new(0.0, PLAYER_HALF.y, 0.0);
        run_for(&map, &mut pos, Vec3::new(5.0, 0.0, 0.0), 60);
        assert!((pos.y - PLAYER_HALF.y - 2.8).abs() < 0.01, "top of the stairs: {pos}");
        assert_eq!(run_for(&map, &mut pos, Vec3::new(-5.0, 0.0, 0.0), 60), 0);
        assert!((pos.y - PLAYER_HALF.y).abs() < 0.01 && pos.x < 1.5, "back down: {pos}");
    }

    #[test]
    fn the_grid_finds_the_same_hits_as_checking_every_block() {
        let map = Map::depot();
        let mut seed = 12345_u64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10_000) as f32 / 10_000.0
        };
        for _ in 0..3000 {
            let origin = Vec3::new(rand() * 70.0 - 35.0, rand() * 12.0, rand() * 70.0 - 35.0);
            let dir = Vec3::new(rand() - 0.5, rand() - 0.6, rand() - 0.5).normalize();
            let max = rand() * 80.0;
            let brute = map
                .blocks
                .iter()
                .filter_map(|block| ray_box(origin, dir, block.min, block.max))
                .filter(|t| *t < max)
                .min_by(f32::total_cmp);
            let fast = map.raycast(origin, dir, max);
            assert_eq!(brute.is_some(), fast.is_some(), "{origin} {dir} {max}");
            if let (Some(a), Some(b)) = (brute, fast) {
                assert!((a - b).abs() < 1e-4, "{origin} {dir}: {a} vs {b}");
            }
        }
    }

    /// Valley has hundreds of rocks: the ramp grid must find what checking
    /// every ramp finds, for rays and for boxes.
    #[test]
    fn the_ramp_grid_finds_the_same_hits_as_checking_every_ramp() {
        let map = Map::by_id(2);
        assert!(map.ramps.len() > 200, "{} ramps", map.ramps.len());
        let mut seed = 777_u64;
        let mut rand = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % 10_000) as f32 / 10_000.0
        };
        let mut hits = 0;
        for _ in 0..4000 {
            let ramp = &map.ramps[(rand() * map.ramps.len() as f32) as usize % map.ramps.len()];
            // Somewhere near a ramp, looking any way.
            let middle = (ramp.min + ramp.max) / 2.0;
            let origin = middle + Vec3::new(rand() - 0.5, rand() - 0.3, rand() - 0.5) * 30.0;
            let dir = Vec3::new(rand() - 0.5, rand() - 0.5, rand() - 0.5).normalize();
            let max = rand() * 60.0;
            let brute = map.ramps.iter().filter_map(|ramp| ramp.raycast(origin, dir, max)).min_by(f32::total_cmp);
            let fast = map.raycast_ramps(origin, dir, max);
            assert_eq!(brute.is_some(), fast.is_some(), "{origin} {dir} {max}");
            if let (Some(a), Some(b)) = (brute, fast) {
                assert!((a - b).abs() < 1e-4, "{origin} {dir}: {a} vs {b}");
                hits += 1;
            }
            let center = middle + Vec3::new(rand() - 0.5, rand() - 0.5, rand() - 0.5) * 4.0;
            let brute = map.ramps.iter().any(|ramp| ramp.push_out(center, PLAYER_HALF).is_some());
            assert_eq!(brute, map.ramp_contact(center, PLAYER_HALF).is_some(), "{center}");
        }
        assert!(hits > 100, "only {hits} rays hit");
    }

    /// Rocks are rounded: a body dropped on top of one stands on it, and
    /// one walking into its side is stopped by it, not lifted up it.
    #[test]
    fn rocks_are_stood_on_and_slid_off() {
        let mut map = Map::empty(0);
        map.terrain = Some(Terrain::generate(41, 2.0, |_, _| 0.0));
        map.add_prop(crate::props::ROCK_LARGE, Vec3::ZERO, 0.0, 1.0);
        let map = map.finish();
        let rock = &map.ramps[0];
        let fall = |mut pos: Vec3| {
            let mut vel = Vec3::ZERO;
            for _ in 0..240 {
                vel.y -= 22.0 / 60.0;
                let grounded = map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, 1.0 / 60.0);
                vel.x *= 0.9;
                vel.z *= 0.9;
                if grounded && vel.length() < 0.01 {
                    break;
                }
            }
            pos - Vec3::Y * PLAYER_HALF.y
        };
        let top = rock.top(0.0, 0.0).unwrap();
        let feet = fall(Vec3::new(0.0, top + 3.0, 0.0));
        assert!((feet.y - top).abs() < 0.3 && feet.x.abs() < 0.5 && feet.z.abs() < 0.5, "on top at {feet}, top {top}");
        let mut pos = Vec3::new(rock.max.x + 2.0, PLAYER_HALF.y, 0.0);
        run_for(&map, &mut pos, Vec3::new(-5.0, 0.0, 0.0), 60);
        let feet = pos - Vec3::Y * PLAYER_HALF.y;
        assert!(feet.y.abs() < 0.05 && feet.x > 0.5, "walked into it to {feet}");
    }

    #[test]
    fn bullets_and_grenades_stop_at_the_ground() {
        let map = hillside();
        // A level shot into the slope hits it where it is 2 m high.
        let t = map.raycast(Vec3::new(-10.0, 2.0, 0.0), Vec3::X, 100.0).unwrap();
        assert!((t - 15.0).abs() < 0.05, "{t}");
        assert!(!map.line_of_sight(Vec3::new(-10.0, 2.0, 0.0), Vec3::new(30.0, 2.0, 0.0)));
        // A grenade lobbed onto flat ground bounces and settles on it.
        let mut pos = Vec3::new(-20.0, 3.0, 0.0);
        let mut vel = Vec3::new(4.0, 3.0, 1.0);
        let mut bounces = 0;
        for _ in 0..600 {
            bounces += (map.step_grenade(&mut pos, &mut vel, 1.0 / 60.0) > BOUNCE_SOUND) as u32;
        }
        assert!(bounces >= 1);
        assert!((pos.y - GRENADE_HALF).abs() < 1e-3 && pos.x < 0.0, "rests at {pos}");
        assert!(vel.length() < 0.5, "still moving at {vel:?}");
    }
}

