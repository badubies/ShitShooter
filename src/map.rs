//! The maps: static axis-aligned blocks, jump pads and spawn points, plus the
//! collision helpers used by the player controller, the bots, grenades and hit
//! detection. Pure data and math, so the server can use it without any rendering.

use bevy::math::{Quat, Vec2, Vec3};

use crate::{
    protocol::{FLASH_BLIND, FLASH_RANGE, MODE_CONQUEST, SMOKE_RADIUS, SMOKE_RISE},
    props::{FLAGPOLE, PROPS},
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
/// Side of a cell of the grid that finds the blocks near a point quickly.
const GRID_CELL: f32 = 8.0;

pub const MAP_NAMES: [&str; 3] = ["Arena", "Depot", "Valley"];

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

    fn plain(center: Vec3, lift: f32) -> JumpPad {
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
/// among the map's (invisible) blocks.
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

/// How a map is lit and what is around it.
pub struct Look {
    /// The colour of the sky (sRGB).
    pub sky: [f32; 3],
    /// Fog from this far to this far, if any. Its colour is the sky's.
    pub fog: Option<(f32, f32)>,
    /// Where the sun shines from, and how brightly (lux).
    pub sun_from: Vec3,
    pub sun: f32,
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
            ambient: 350.0,
            orbit: (Vec3::new(0.0, 2.0, 0.0), 46.0, 26.0),
        }
    }
}

pub struct Map {
    /// Index into `MAP_NAMES`.
    pub id: u8,
    /// Do not change after `finish`: the grid indexes them.
    pub blocks: Vec<Block>,
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
    pub look: Look,
    grid: BlockGrid,
}

/// Which blocks are near which part of the map: the blocks touching each
/// square cell of a grid over the map's footprint.
#[derive(Default)]
struct BlockGrid {
    origin: Vec2,
    nx: usize,
    nz: usize,
    cells: Vec<Vec<u32>>,
}

impl BlockGrid {
    fn new(blocks: &[Block]) -> BlockGrid {
        let (mut min, mut max) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for block in blocks {
            min = min.min(Vec2::new(block.min.x, block.min.z));
            max = max.max(Vec2::new(block.max.x, block.max.z));
        }
        if blocks.is_empty() {
            (min, max) = (Vec2::ZERO, Vec2::ONE);
        }
        let nx = (((max.x - min.x) / GRID_CELL).ceil() as usize).max(1);
        let nz = (((max.y - min.y) / GRID_CELL).ceil() as usize).max(1);
        let mut grid = BlockGrid {
            origin: min,
            nx,
            nz,
            cells: vec![Vec::new(); nx * nz],
        };
        for (index, block) in blocks.iter().enumerate() {
            let (x0, z0, x1, z1) = grid.span(block.min, block.max);
            for z in z0..=z1 {
                for x in x0..=x1 {
                    grid.cells[z * nx + x].push(index as u32);
                }
            }
        }
        grid
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
}

/// True if a player with feet at `feet` is close enough to grab the pickup
/// lying at `spot`.
pub fn touches_pickup(feet: Vec3, spot: Vec3) -> bool {
    (feet - spot).with_y(0.0).length() < 1.2 && (-0.5..1.5).contains(&(feet.y - spot.y))
}

const FLOOR: [f32; 3] = [0.20, 0.22, 0.26];
const WALL: [f32; 3] = [0.30, 0.33, 0.40];
const COVER: [f32; 3] = [0.75, 0.42, 0.18];
const PILLAR: [f32; 3] = [0.45, 0.48, 0.55];
const PLATFORM: [f32; 3] = [0.22, 0.45, 0.60];
const LEDGE: [f32; 3] = [0.50, 0.30, 0.60];

const DEPOT_FLOOR: [f32; 3] = [0.25, 0.24, 0.22];
const DEPOT_WALL: [f32; 3] = [0.36, 0.33, 0.30];
const CONCRETE: [f32; 3] = [0.52, 0.51, 0.48];
const CONTAINER_RED: [f32; 3] = [0.62, 0.20, 0.16];
const CONTAINER_BLUE: [f32; 3] = [0.16, 0.34, 0.60];
const CRATE: [f32; 3] = [0.62, 0.46, 0.24];
const BARRIER: [f32; 3] = [0.80, 0.68, 0.20];
const ROOF: [f32; 3] = [0.20, 0.42, 0.36];

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
            look: Look::default(),
            grid: BlockGrid::default(),
        }
    }

    /// Stands a model on the map, and adds its collision boxes as blocks.
    /// Call before `finish`.
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

    /// Indexes the blocks. Call once they are all in.
    pub fn finish(mut self) -> Map {
        self.grid = BlockGrid::new(&self.blocks);
        self
    }

    /// Calls `f` with every block whose cell the box from `min` to `max`
    /// touches (a block may come up more than once).
    fn near(&self, min: Vec3, max: Vec3, mut f: impl FnMut(&Block)) {
        if self.grid.cells.is_empty() {
            self.blocks.iter().for_each(f);
            return;
        }
        let (x0, z0, x1, z1) = self.grid.span(min, max);
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &index in &self.grid.cells[z * self.grid.nx + x] {
                    f(&self.blocks[index as usize]);
                }
            }
        }
    }

    /// The height of the ground at (x, z), if the map has terrain.
    pub fn ground(&self, x: f32, z: f32) -> Option<f32> {
        self.terrain.as_ref().map(|terrain| terrain.height(x, z))
    }

    /// The map to play `mode` on: outside conquest, without its flagpoles
    /// (nor anything left of them to bump into).
    pub fn for_game(id: u8, mode: u8) -> Map {
        let mut map = Map::by_id(id);
        if mode != MODE_CONQUEST {
            let boxes: Vec<(Vec3, Vec3)> =
                map.props.iter().filter(|prop| prop.model == FLAGPOLE).flat_map(Prop::boxes).collect();
            map.props.retain(|prop| prop.model != FLAGPOLE);
            map.blocks
                .retain(|block| block.visible || !boxes.contains(&(block.min, block.max)));
            map.grid = BlockGrid::new(&map.blocks);
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
    /// and the ground. Velocity is zeroed on any axis that hits something.
    /// Walking into a ledge no higher than `STEP_UP` climbs it. Returns true
    /// if the box ended up standing on something.
    pub fn move_and_slide(&self, pos: &mut Vec3, vel: &mut Vec3, half: Vec3, dt: f32) -> bool {
        let total = *vel * dt;
        let steps = (total.abs().max_element() / 0.25).ceil().clamp(1.0, 32.0) as usize;
        let step = total / steps as f32;
        let mut grounded = false;
        for _ in 0..steps {
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
            // The ground holds up the middle of the box.
            if let Some(ground) = self.ground(pos.x, pos.z)
                && pos.y - half.y < ground + 1e-3
            {
                pos.y = pos.y.max(ground + half.y);
                if vel.y <= 0.0 {
                    vel.y = 0.0;
                    grounded = true;
                }
            }
        }
        grounded
    }

    /// Puts a box that was standing on something last frame back down on
    /// what is under it, if that is at most `drop` below its feet: walking
    /// down a slope or a stair, you stay on the ground instead of skipping
    /// off it. Returns true if it did.
    pub fn snap_to_ground(&self, pos: &mut Vec3, half: Vec3, drop: f32) -> bool {
        let feet = pos.y - half.y;
        let mut floor = self.ground(pos.x, pos.z).filter(|ground| *ground <= feet + 1e-3);
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
    /// touching a block, with the middle of its bottom above the ground.
    pub fn is_free(&self, center: Vec3, half: Vec3) -> bool {
        if self.ground(center.x, center.z).is_some_and(|ground| center.y - half.y < ground - 0.02) {
            return false;
        }
        self.overlapping(center, half).is_none()
    }

    /// The first block a box at `center` overlaps, if any.
    fn overlapping(&self, center: Vec3, half: Vec3) -> Option<&Block> {
        if self.grid.cells.is_empty() {
            return self.blocks.iter().find(|block| overlaps(center, half, block));
        }
        let (x0, z0, x1, z1) = self.grid.span(center - half, center + half);
        for z in z0..=z1 {
            for x in x0..=x1 {
                for &index in &self.grid.cells[z * self.grid.nx + x] {
                    let block = &self.blocks[index as usize];
                    if overlaps(center, half, block) {
                        return Some(block);
                    }
                }
            }
        }
        None
    }

    /// Advances a grenade by one `GRENADE_STEP`: it falls, bounces off blocks
    /// and the ground, and rolls to a stop. Returns the speed of its hardest
    /// contact, 0 if it touched nothing (above `BOUNCE_SOUND` it can be heard).
    pub fn step_grenade(&self, pos: &mut Vec3, vel: &mut Vec3, dt: f32) -> f32 {
        const RESTITUTION: f32 = 0.42;
        /// Share of the sideways speed kept through each contact.
        const GRIP: f32 = 0.84;
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
                *vel *= GRIP;
                // Too slow to bounce: it stays put on that axis (resting on the floor).
                vel[axis] = if impact > 1.5 { -vel[axis] * RESTITUTION / GRIP } else { 0.0 };
                hardest = hardest.max(impact);
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
                *vel = along * GRIP + if impact > 1.5 { normal * impact * RESTITUTION } else { Vec3::ZERO };
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

    /// Distance along the ray to the closest block or the ground, if it is
    /// nearer than `max`.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let block = self.raycast_blocks(origin, dir, max);
        let ground = self
            .terrain
            .as_ref()
            .and_then(|terrain| terrain.raycast(origin, dir, block.unwrap_or(max)));
        ground.or(block)
    }

    /// The nearest block along the ray: walks the grid cells the ray crosses
    /// (seen from above), nearest first, and stops at the first cell that
    /// holds a hit closer than its far side.
    fn raycast_blocks(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let grid = &self.grid;
        if grid.cells.is_empty() {
            return self
                .blocks
                .iter()
                .filter_map(|block| ray_box(origin, dir, block.min, block.max))
                .filter(|t| *t < max)
                .min_by(f32::total_cmp);
        }
        let size = Vec2::new(grid.nx as f32, grid.nz as f32) * GRID_CELL;
        let flat = Vec2::new(dir.x, dir.z);
        let start = Vec2::new(origin.x, origin.z);
        // Clip the ray to the grid.
        let (mut t0, mut t1) = (0.0_f32, max);
        for axis in 0..2 {
            let (o, d, lo, hi) = (start[axis], flat[axis], grid.origin[axis], grid.origin[axis] + size[axis]);
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
        let entry = (start + flat * t0 - grid.origin) / GRID_CELL;
        let counts = [grid.nx, grid.nz];
        let mut cell = [
            (entry.x.floor().max(0.0) as usize).min(grid.nx - 1),
            (entry.y.floor().max(0.0) as usize).min(grid.nz - 1),
        ];
        let mut next = [f32::INFINITY; 2];
        let mut delta = [f32::INFINITY; 2];
        for axis in 0..2 {
            let d = flat[axis];
            if d.abs() < 1e-8 {
                continue;
            }
            delta[axis] = GRID_CELL / d.abs();
            let edge = grid.origin[axis] + (cell[axis] as f32 + if d > 0.0 { 1.0 } else { 0.0 }) * GRID_CELL;
            next[axis] = (edge - start[axis]) / d;
        }
        let mut best: Option<f32> = None;
        loop {
            for &index in &grid.cells[cell[1] * grid.nx + cell[0]] {
                let block = &self.blocks[index as usize];
                if let Some(t) = ray_box(origin, dir, block.min, block.max)
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
            spawns: spawn_ring(&map, Vec2::new(x, z), 3.5, radius, 4),
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
            assert!(!map.armor_spots.is_empty() && map.armor_spots.len() <= 32, "map {}", map.id);
            for spot in &map.armor_spots {
                let standing = *spot + Vec3::Y * (PLAYER_HALF.y + 0.01);
                assert!(map.is_free(standing, PLAYER_HALF), "map {} armor at {spot} is inside a block", map.id);
                assert!(!map.is_free(*spot - Vec3::Y * 0.05, Vec3::splat(0.04)), "map {} armor at {spot} floats", map.id);
                assert!(touches_pickup(*spot, *spot));
            }
        }
    }

    #[test]
    fn every_map_has_flags_and_bases_for_conquest() {
        for map in maps() {
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
        for id in 0..MAP_NAMES.len() as u8 {
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
