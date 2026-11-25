//! Hamlet: a small hilltop village for close fights, about the size of
//! Depot. A warehouse, a barn and houses to run through, walled yards,
//! alleys and a little market square, all inside an old town wall. The
//! square sits on top of a low rise (so nobody sees across the town over
//! it), with a terrace beside it, upper floors, a loft and a mezzanine.
//! Played in free for all and team deathmatch only: there are no flags and
//! no hill.
//!
//! What it is built for is checked by its tests: cover within 5 m of every
//! spot outdoors, no sightline across the town, and bots that get from any
//! spawn to any other. To see it from above while changing it, run the
//! ignored `preview` test with HAMLET_PREVIEW set to a .ppm path, and
//! `print_survey` for its longest sightlines (next to Depot's).
//!
//! Like Valley, everything is laid out here in code, the same on the server
//! and every client.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::math::{Quat, Vec2, Vec3};

use crate::{
    map::{Block, Look, Map, PLAYER_HALF, Spawn},
    props::*,
    terrain::{Terrain, fractal},
};

/// The inner face of the town wall: the square played in is twice this.
const PLAY: f32 = 34.0;
const WALL_HEIGHT: f32 = 4.0;
/// The ground grid: 120 m across in 1 m cells, so the hills outside the
/// wall show over it.
const GRID: usize = 121;
const CELL: f32 = 1.0;

const S: f32 = PI;
const E: f32 = FRAC_PI_2;
const W: f32 = -FRAC_PI_2;

/// A model standing on the ground (model, x, z, yaw). Yaw 0 faces its front
/// south (+z), `S` north, `E` east, `W` west.
type Placed = (usize, f32, f32, f32);

/// Buildings, each on a levelled plot.
const BUILDINGS: &[Placed] = &[
    // North row: the warehouse, a house and the barn against the town wall.
    (WAREHOUSE, -26.7, -19.7, S),
    (HOUSE, -13.0, -29.8, E),
    (HOUSE_TALL_BRICK, 2.0, -27.0, 0.0),
    (HOUSE_WARM, -5.5, -17.3, 0.0),
    (BARN, 21.0, -28.85, W),
    (RUIN, 10.0, -13.0, 0.0),
    (HOUSE, 24.0, -14.0, 0.0),
    // Middle row: the house in the square, and one either side of it.
    (HOUSE, -29.8, 2.0, 0.0),
    (HOUSE, -17.0, 3.0, E),
    (HOUSE_WARM, -4.5, 0.0, E),
    (HOUSE_WARM, 15.0, -4.5, S),
    (HOUSE_WARM, 29.8, 1.2, 0.0),
    // South row.
    (HOUSE_TALL, -29.3, 14.7, S),
    (HOUSE_WARM, -12.0, 13.6, S),
    (HOUSE, 1.0, 10.6, 0.0),
    (HOUSE, 11.0, 13.0, E),
    (HOUSE, 24.0, 13.0, 0.0),
    // Along the south wall.
    (HOUSE_WARM, -20.0, 29.8, E),
    (RUIN, -5.0, 25.0, E),
    (HOUSE, 8.0, 26.0, 0.0),
    (HOUSE_WARM, 22.0, 29.8, E),
];

/// Cover and clutter, standing on whatever ground is there.
const CLUTTER: &[Placed] = &[
    // Down the middle of the warehouse.
    (CONTAINER, -24.8, -21.0, E),
    // The square.
    (WELL, -10.5, 1.0, 0.0),
    (LAMP_POST, -8.6, -4.6, 0.0),
    (LAMP_POST, -0.6, 5.0, 0.0),
    (LAMP_POST, 9.0, -1.0, 0.0),
    (CAR, 2.5, 3.5, 0.0),
    (BARRELS, -7.5, -6.5, 0.0),
    (CRATES, 3.5, -1.5, E),
    (CONTAINER, 3.0, -7.0, 0.0),
    (SANDBAGS, -9.0, 8.5, 0.0),
    // The main street, west.
    (CAR, -30.0, -4.5, 0.0),
    (CONTAINER, -22.5, -5.6, 0.0),
    // The workshop yard by the warehouse.
    (CONTAINER, -15.0, -17.0, E),
    (CRATES, -12.0, -10.5, 0.0),
    // The north lane.
    (BARRELS, -5.0, -31.5, 0.0),
    (CRATES, -7.0, -24.0, E),
    // The barn yard.
    (HAYBALE, 8.5, -29.0, 0.0),
    (HAYBALE, 9.8, -28.6, E),
    (CRATES, 10.5, -21.0, 0.0),
    // In front of the tall brick house.
    (CAR, 3.0, -15.5, 0.0),
    // Between the barn and the house south of it.
    (CONTAINER, 24.0, -20.5, 0.0),
    (CRATES, 31.0, -12.0, E),
    // The main street, east.
    (CAR, 21.5, -5.5, E),
    (CONTAINER, 28.5, -6.5, 0.0),
    // The yard south of the main street, east.
    (CONTAINER, 16.0, 3.0, 0.0),
    (BARRELS, 11.5, 2.0, 0.0),
    (CONTAINER, 22.5, 6.5, E),
    // West of the square.
    (CRATES, -12.0, -6.5, 0.0),
    (OAK, -22.5, 6.5, 0.0),
    // South-east.
    (CONTAINER, 17.0, 21.0, 0.0),
    (CONTAINER, 31.0, 21.0, E),
    (CRATES, 13.5, 30.0, 0.0),
    // The gardens south-west.
    (TENT, -29.0, 27.0, E),
    (HAYBALE, -14.0, 20.5, 0.0),
    (OAK, -20.0, 21.0, 0.0),
];

pub(crate) const STONE: [f32; 3] = [0.50, 0.47, 0.42];
pub(crate) const STONE_DARK: [f32; 3] = [0.38, 0.36, 0.33];
pub(crate) const PLASTER: [f32; 3] = [0.72, 0.66, 0.55];
pub(crate) const WOOD: [f32; 3] = [0.45, 0.32, 0.20];

/// A wall of blocks: (x0, z0, x1, z1, height above the ground, colour).
type Wall = (f32, f32, f32, f32, f32, [f32; 3]);

const WALLS: &[Wall] = &[
    // The terrace on the north side of the square, and its step.
    (-6.0, -12.5, 4.0, -9.5, 1.0, STONE),
    (-2.0, -9.5, 0.0, -8.6, 0.5, STONE),
    // A market stall west of the square: a tall back, and a counter facing
    // the square.
    (-11.2, 4.5, -10.8, 7.5, 2.4, WOOD),
    (-10.8, 4.5, -10.2, 7.5, 1.0, WOOD),
    // Woodpiles: against the ruin on the main street, and in both ruins,
    // where their roofs fell in.
    (6.2, -9.3, 9.0, -7.9, 2.2, WOOD),
    (7.0, -14.5, 9.0, -12.5, 2.2, WOOD),
    (-6.0, 23.8, -4.0, 29.0, 2.2, WOOD),
    // The workshop yard's low wall.
    (-19.0, -11.4, -14.0, -11.0, 1.1, STONE),
    // Woodpiles filling the corners behind the warehouse and the barn.
    (-34.0, -34.0, -31.0, -31.0, 2.2, WOOD),
    (30.0, -34.0, 34.0, -31.5, 2.2, WOOD),
    // The courtyard in front of the tall brick house.
    (1.0, -19.0, 5.0, -18.6, 1.1, STONE),
    // A woodshed between the houses south of the square.
    (-2.0, 17.6, 2.0, 20.0, 2.4, WOOD),
    // The gardens south-west: a high wall, then a low one.
    (-23.0, 18.5, -17.0, 18.9, 2.4, PLASTER),
    (-15.0, 18.5, -10.0, 18.9, 1.1, STONE),
];

pub fn hamlet() -> Map {
    let mut map = Map::empty(3);
    let plots = plots();

    // Each plot is levelled at the height the ground has there once the
    // plots before it are, so plots that touch stay level with each other.
    let mut plot_heights: Vec<f32> = Vec::new();
    for (index, plot) in plots.iter().enumerate() {
        let mut h = base(plot.center.x, plot.center.y);
        for (before, target) in plots[..index].iter().zip(&plot_heights) {
            h += (target - h) * before.weight(plot.center);
        }
        plot_heights.push(h);
    }
    let mut terrain = Terrain::generate(GRID, CELL, |x, z| {
        let mut h = base(x, z);
        for (plot, target) in plots.iter().zip(&plot_heights) {
            h += (target - h) * plot.weight(Vec2::new(x, z));
        }
        h
    });
    terrain.colors = ground_colors(&terrain, &plots);
    let ground = |x: f32, z: f32| terrain.height(x, z);

    for (&(model, x, z, yaw), height) in BUILDINGS.iter().zip(&plot_heights) {
        map.add_prop(model, Vec3::new(x, *height, z), yaw, 1.0);
    }
    for &(model, x, z, yaw) in CLUTTER {
        map.add_prop(model, Vec3::new(x, ground(x, z), z), yaw, 1.0);
    }
    for &(x0, z0, x1, z1, height, color) in WALLS {
        wall(&mut map, &terrain, Vec2::new(x0, z0), Vec2::new(x1, z1), height, color);
    }
    town_wall(&mut map, &terrain);
    outskirts(&mut map, &terrain, &plot_heights[BUILDINGS.len()..]);

    map.terrain = Some(terrain);
    map.bounds = [Vec2::splat(-PLAY + 2.0), Vec2::splat(PLAY - 2.0)];
    map.look = Look {
        sky: [0.66, 0.76, 0.88],
        fog: Some((60.0, 260.0)),
        sun_from: Vec3::new(-60.0, 110.0, 45.0),
        sun: 10000.0,
        ambient: 450.0,
        orbit: (Vec3::new(0.0, 2.0, 0.0), 52.0, 30.0),
    };
    let mut map = map.finish();
    map.spawns = spawns(&map);
    // Not played here (see `map::plays`), but somewhere sensible all the same.
    map.hill = Vec3::new(0.0, map.ground(0.0, 0.0).unwrap_or(0.0), 0.0);
    map.armor_spots = ARMOR_IN
        .iter()
        .map(|&(model, x, local)| {
            let (index, &(_, bx, bz, yaw)) = BUILDINGS
                .iter()
                .enumerate()
                .find(|(_, building)| building.0 == model && building.1 == x)
                .expect("armor in a building that is not on the map");
            on_model(Vec3::new(bx, plot_heights[index], bz), yaw, Vec3::from(local))
        })
        .collect();
    // And on the terrace by the square: on top of whatever is there.
    let (tx, tz) = TERRACE_ARMOR;
    let from = Vec3::new(tx, 20.0, tz);
    let drop = map.raycast(from, Vec3::NEG_Y, 30.0).unwrap_or(20.0);
    map.armor_spots.push(from - Vec3::Y * drop);
    map
}

/// Armor up on the warehouse's mezzanine, in the barn's loft, upstairs in
/// both tall houses and in the house on the square: (the building's model
/// and x, the point in its own space).
const ARMOR_IN: [(usize, f32, [f32; 3]); 5] = [
    (WAREHOUSE, -26.7, [5.3, 3.4, -2.0]),
    (BARN, 21.0, [-1.0, 3.0, -4.5]),
    (HOUSE_TALL_BRICK, 2.0, [2.0, 3.5, 1.0]),
    (HOUSE_TALL, -29.3, [2.0, 3.5, 1.0]),
    (HOUSE_WARM, -4.5, [-2.0, 0.15, 1.5]),
];
/// And one on the terrace north of the square (x, z).
const TERRACE_ARMOR: (f32, f32) = (-4.0, -11.0);

/// Buildings outside the wall, to look at: a few farms and the old chapel.
const OUTSKIRTS: &[Placed] = &[
    (CHAPEL, -6.0, -50.0, 0.0),
    (HOUSE, -46.0, 12.0, E),
    (HOUSE_WARM, -45.0, -9.0, E),
    (SHED, -44.0, -20.0, E),
    (BARN, 48.0, -14.0, 0.0),
    (SILO, 50.0, 0.0, 0.0),
    (HOUSE_TALL, 45.0, 19.0, W),
    (HOUSE, 10.0, 46.0, S),
    (HOUSE_WARM, -14.0, 45.0, S),
    (HOUSE_TALL_BRICK, 26.0, -46.0, 0.0),
];

/// What lies beyond the town wall: the buildings in `OUTSKIRTS` on their
/// plots (`heights`), and woods on the hills. None of it collides: nobody
/// gets out there.
fn outskirts(map: &mut Map, terrain: &Terrain, heights: &[f32]) {
    for (&(model, x, z, yaw), height) in OUTSKIRTS.iter().zip(heights) {
        map.add_decor(model, Vec3::new(x, *height, z), yaw, 1.0);
    }
    let mut rng = Rng(0x4a3e_1e70_55aa_c0de);
    let mut taken = std::collections::HashSet::new();
    let reach = 58.0;
    for _ in 0..1500 {
        let at = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let edge = at.x.abs().max(at.y.abs());
        if edge < PLAY + 4.0 || !taken.insert(((at.x / 5.0).floor() as i32, (at.y / 5.0).floor() as i32)) {
            continue;
        }
        // Clear of the farms and the chapel.
        let crowded = map.props.iter().any(|prop| {
            prop.model < PINE && Vec2::new(prop.pos.x, prop.pos.z).distance(at) < footprint(prop.model, prop.yaw).length() + 2.5
        });
        if crowded || rng.unit() < 0.35 {
            continue;
        }
        let pick = rng.unit();
        let model = if pick < 0.4 {
            PINE
        } else if pick < 0.6 {
            OAK
        } else if pick < 0.8 {
            BIRCH
        } else {
            PINE_SMALL
        };
        let y = terrain.height(at.x, at.y) - 0.15;
        map.add_decor(model, Vec3::new(at.x, y, at.y), rng.range(0.0, 2.0 * PI), rng.range(0.85, 1.3));
        if rng.unit() < 0.3 {
            let bush = at + Vec2::from_angle(rng.range(0.0, 2.0 * PI)) * rng.range(2.0, 3.5);
            let y = terrain.height(bush.x, bush.y) - 0.1;
            map.add_decor(BUSH, Vec3::new(bush.x, y, bush.y), rng.range(0.0, 2.0 * PI), rng.range(0.7, 1.3));
        }
    }
}

/// A small repeatable random number generator (xorshift).
struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// The ground before the plots are levelled into it: the town sits on a low
/// rise with the square on top, a little higher to the north-west, and
/// hills beyond the town wall.
fn base(x: f32, z: f32) -> f32 {
    let rise = 1.4 * (-(x * x + z * z) / (2.0 * 17.0 * 17.0)).exp();
    let north = 0.5 * smoothstep(0.0, 30.0, -z - x * 0.5);
    let lumps = 0.25 * fractal(x / 13.0, z / 13.0, 2, 31);
    let edge = x.abs().max(z.abs());
    let hills = smoothstep(PLAY + 3.0, PLAY + 26.0, edge) * 9.0 * (0.75 + 0.25 * fractal(x / 17.0, z / 17.0, 2, 32));
    rise + north + lumps + hills
}

/// A levelled plot of ground under a building.
struct Plot {
    center: Vec2,
    half: Vec2,
}

/// Flat ground this far round every building, easing back into the slope
/// over `PLOT_EASE`.
const PLOT_MARGIN: f32 = 1.2;
const PLOT_EASE: f32 = 3.0;

/// Every building's plot: the town's, then those outside the wall.
fn plots() -> Vec<Plot> {
    BUILDINGS
        .iter()
        .chain(OUTSKIRTS)
        .map(|&(model, x, z, yaw)| Plot {
            center: Vec2::new(x, z),
            half: footprint(model, yaw) + Vec2::splat(PLOT_MARGIN),
        })
        .collect()
}

impl Plot {
    /// 1 on the plot, easing to 0 over `PLOT_EASE` round it.
    fn weight(&self, at: Vec2) -> f32 {
        let outside = ((at - self.center).abs() - self.half).max(Vec2::ZERO).length();
        1.0 - smoothstep(0.0, PLOT_EASE, outside)
    }
}

/// Half the size of a model's collision boxes seen from above, turned by `yaw`.
fn footprint(model: usize, yaw: f32) -> Vec2 {
    let mut half = Vec2::ZERO;
    for [x0, _, z0, x1, _, z1] in PROPS[model].colliders {
        half = half.max(Vec2::new(x0.abs().max(x1.abs()), z0.abs().max(z1.abs())));
    }
    if (yaw / FRAC_PI_2).round().rem_euclid(2.0) == 1.0 { Vec2::new(half.y, half.x) } else { half }
}

/// A wall of blocks from corner `a` to corner `b`, `height` above the
/// ground, cut into short lengths so it follows the slope.
fn wall(map: &mut Map, terrain: &Terrain, a: Vec2, b: Vec2, height: f32, color: [f32; 3]) {
    let (min, max) = (a.min(b), a.max(b));
    let size = max - min;
    let pieces = (size.max_element() / 3.0).ceil().max(1.0) as usize;
    let along_x = size.x >= size.y;
    for k in 0..pieces {
        let (t0, t1) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
        let (lo, hi) = if along_x {
            (Vec2::new(min.x + size.x * t0, min.y), Vec2::new(min.x + size.x * t1, max.y))
        } else {
            (Vec2::new(min.x, min.y + size.y * t0), Vec2::new(max.x, min.y + size.y * t1))
        };
        let corners = [lo, hi, Vec2::new(lo.x, hi.y), Vec2::new(hi.x, lo.y), (lo + hi) / 2.0];
        let heights = corners.map(|c| terrain.height(c.x, c.y));
        let low = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let mid = heights[4];
        map.blocks.push(Block {
            min: Vec3::new(lo.x, low - 0.4, lo.y),
            max: Vec3::new(hi.x, mid + height, hi.y),
            color,
            visible: true,
        });
    }
}

/// The old wall round the town, too tall to climb, with an invisible wall
/// above it so nobody stands on top.
fn town_wall(map: &mut Map, terrain: &Terrain) {
    let (inner, outer) = (PLAY, PLAY + 1.2);
    for (a, b) in [
        (Vec2::new(-outer, -outer), Vec2::new(outer, -inner)),
        (Vec2::new(-outer, inner), Vec2::new(outer, outer)),
        (Vec2::new(-outer, -inner), Vec2::new(-inner, inner)),
        (Vec2::new(inner, -inner), Vec2::new(outer, inner)),
    ] {
        wall(map, terrain, a, b, WALL_HEIGHT, STONE_DARK);
        map.blocks.push(Block {
            min: Vec3::new(a.x, -10.0, a.y),
            max: Vec3::new(b.x, 60.0, b.y),
            color: [0.0; 3],
            visible: false,
        });
    }
}

/// Colours of the ground (sRGB) at each vertex: trodden earth and gravel in
/// the town, grass in its corners and outside the wall.
fn ground_colors(terrain: &Terrain, plots: &[Plot]) -> Vec<[f32; 3]> {
    const GRASS: Vec3 = Vec3::new(0.31, 0.42, 0.17);
    const GRASS_DARK: Vec3 = Vec3::new(0.20, 0.31, 0.12);
    const EARTH: Vec3 = Vec3::new(0.45, 0.38, 0.28);
    const GRAVEL: Vec3 = Vec3::new(0.46, 0.44, 0.40);
    let mut colors = Vec::with_capacity(terrain.size * terrain.size);
    for j in 0..terrain.size {
        for i in 0..terrain.size {
            let v = terrain.vertex(i, j);
            let at = Vec2::new(v.x, v.z);
            let patch = (fractal(at.x / 9.0, at.y / 9.0, 3, 77) * 0.5 + 0.5).clamp(0.0, 1.0);
            let grass = GRASS_DARK.lerp(GRASS, patch);
            let town = EARTH.lerp(GRAVEL, (fractal(at.x / 5.0, at.y / 5.0, 2, 78) * 0.6 + 0.5).clamp(0.0, 1.0));
            let inside = 1.0 - smoothstep(PLAY - 1.0, PLAY + 2.0, at.x.abs().max(at.y.abs()));
            let mut c = grass.lerp(town, inside * (0.55 + 0.45 * patch));
            for plot in plots {
                c = c.lerp(EARTH, plot.weight(at) * 0.5);
            }
            colors.push(c.to_array());
        }
    }
    colors
}

/// Where players come in: spread out over the open ground of the town,
/// each facing the longest way it can see.
fn spawns(map: &Map) -> Vec<Spawn> {
    let mut candidates: Vec<Vec3> = Vec::new();
    let mut z = -PLAY + 2.0;
    while z <= PLAY - 2.0 {
        let mut x = -PLAY + 2.0;
        while x <= PLAY - 2.0 {
            let feet = Vec3::new(x, map.ground(x, z).unwrap_or(0.0), z);
            let body = feet + Vec3::Y * (PLAYER_HALF.y + 0.05);
            let room = PLAYER_HALF + Vec3::new(0.6, 0.0, 0.6);
            let roofed = map.raycast(feet + Vec3::Y * 2.0, Vec3::Y, 40.0).is_some();
            if !roofed && map.is_free(body, room) && map.is_free(body + Vec3::Y * 0.5, PLAYER_HALF) {
                candidates.push(feet);
            }
            x += 2.0;
        }
        z += 2.0;
    }
    // The farthest from those already picked, each time.
    let mut picked: Vec<Vec3> = Vec::new();
    while picked.len() < 16 && !candidates.is_empty() {
        let next = if picked.is_empty() {
            candidates.iter().position(|c| c.x < -20.0 && c.z < 30.0).unwrap_or(0)
        } else {
            let distance = |c: &Vec3| picked.iter().map(|p| p.distance(*c)).fold(f32::INFINITY, f32::min);
            (0..candidates.len()).max_by(|a, b| distance(&candidates[*a]).total_cmp(&distance(&candidates[*b]))).unwrap()
        };
        picked.push(candidates.swap_remove(next));
    }
    picked
        .into_iter()
        .map(|feet| {
            let eye = feet + Vec3::Y * 1.6;
            let yaw = (0..16)
                .map(|k| k as f32 * PI / 8.0)
                .max_by(|a, b| {
                    let reach = |yaw: f32| map.raycast(eye, crate::map::look_dir(yaw, 0.0), 30.0).unwrap_or(30.0);
                    reach(*a).total_cmp(&reach(*b))
                })
                .unwrap_or(0.0);
            Spawn {
                pos: feet + Vec3::Y * (PLAYER_HALF.y + 0.05),
                yaw,
            }
        })
        .collect()
}

/// Where a point given in a model's own space is, once the model stands at
/// `pos` turned by `yaw`.
fn on_model(pos: Vec3, yaw: f32, local: Vec3) -> Vec3 {
    Quat::from_rotation_y(yaw) * local + pos
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        map::{STEP_UP, look_dir, plays},
        nav::NavGrid,
        protocol::{MODE_CONQUEST, MODE_FFA, MODE_HILL, MODE_TEAMS},
    };

    #[test]
    fn the_hamlet_is_played_only_in_free_for_all_and_teams() {
        assert!(plays(3, MODE_FFA) && plays(3, MODE_TEAMS));
        assert!(!plays(3, MODE_HILL) && !plays(3, MODE_CONQUEST));
        for id in 0..3 {
            assert!([MODE_FFA, MODE_TEAMS, MODE_HILL, MODE_CONQUEST].iter().all(|mode| plays(id, *mode)));
        }
    }

    /// No two things placed on the map stand in each other, and no building
    /// has the ground poking up through its floor.
    #[test]
    fn nothing_stands_in_anything_else() {
        // Every collision box above the ground, and which object it is.
        let mut boxes: Vec<(usize, Vec3, Vec3)> = Vec::new();
        let map = hamlet();
        let mut owner = 0;
        for prop in map.props.iter().filter(|prop| !PROPS[prop.model].colliders.is_empty()) {
            if prop.pos.x.abs().max(prop.pos.z.abs()) > PLAY {
                continue;
            }
            let turn = Quat::from_rotation_y(prop.yaw);
            for [x0, y0, z0, x1, y1, z1] in PROPS[prop.model].colliders {
                // Floors and foundations reach under the ground: leave them out.
                if *y1 <= 0.3 {
                    continue;
                }
                let a = turn * (Vec3::new(*x0, y0.max(0.0), *z0) * prop.scale) + prop.pos;
                let b = turn * (Vec3::new(*x1, *y1, *z1) * prop.scale) + prop.pos;
                boxes.push((owner, a.min(b), a.max(b)));
            }
            owner += 1;
        }
        for &(x0, z0, x1, z1, _, _) in WALLS {
            boxes.push((owner, Vec3::new(x0, -10.0, z0), Vec3::new(x1, 10.0, z1)));
            owner += 1;
        }
        let into = |a: &(usize, Vec3, Vec3), b: &(usize, Vec3, Vec3)| {
            let overlap = a.2.min(b.2) - a.1.max(b.1);
            overlap.min_element() > 0.05
        };
        for (index, a) in boxes.iter().enumerate() {
            for b in &boxes[index + 1..] {
                assert!(a.0 == b.0 || !into(a, b), "{a:?} stands in {b:?}");
            }
        }
        let ground = |x: f32, z: f32| map.ground(x, z).unwrap();
        for prop in map.props.iter().filter(|prop| prop.model <= TENT && prop.pos.x.abs() < PLAY) {
            let half = footprint(prop.model, prop.yaw) - Vec2::splat(0.5);
            for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)] {
                let (x, z) = (prop.pos.x + dx * half.x, prop.pos.z + dz * half.y);
                let poke = ground(x, z) - prop.pos.y;
                assert!(poke < 0.1, "{} at {}: ground {poke:.2} m above its floor", PROPS[prop.model].name, prop.pos);
            }
        }
    }

    /// Runs from `from` to `to` the way a bot does when nobody is in sight
    /// (see `server.rs`): along the way the nav grid finds, passing
    /// waypoints with `pass_waypoints`, hopping when blocked, and finding
    /// the way again after getting nowhere for a while. Returns where it
    /// ends up and how many seconds it took.
    fn run_like_a_bot(map: &Map, nav: &NavGrid, from: Vec3, to: Vec3) -> (Vec3, f32) {
        let dt = 1.0 / 60.0;
        let mut pos = from + Vec3::Y * PLAYER_HALF.y;
        let (mut vel, mut grounded, mut stuck) = (Vec3::ZERO, true, 0.0);
        let mut path = nav.path(map, from, to).unwrap_or_default();
        for frame in 0..60 * 60 {
            if (to - pos).with_y(0.0).length() < 1.0 {
                return (pos - Vec3::Y * PLAYER_HALF.y, frame as f32 * dt);
            }
            crate::nav::pass_waypoints(map, &mut path, pos);
            let steer = path.first().copied().unwrap_or(to);
            let wish = (steer - pos).with_y(0.0).normalize_or_zero() * 7.0;
            let blend = (8.0 * dt).min(1.0);
            vel.x += (wish.x - vel.x) * blend;
            vel.z += (wish.z - vel.z) * blend;
            vel.y -= 22.0 * dt;
            let wanted = vel.with_y(0.0);
            let was = grounded;
            grounded = map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, dt);
            if was && !grounded && vel.y <= 0.0 && map.snap_to_ground(&mut pos, PLAYER_HALF, STEP_UP.max(7.0 * dt)) {
                grounded = true;
                vel.y = 0.0;
            }
            // Bots also hop now and then for fun; on every 90th frame here.
            if grounded && (vel.with_y(0.0).length() < wanted.length() * 0.5 || frame % 90 == 89) {
                vel.y = 8.0;
            }
            let moving = vel.with_y(0.0).length() > 1.0;
            stuck = if moving { 0.0 } else { stuck + dt };
            if stuck > 1.5 {
                stuck = 0.0;
                path = nav.path(map, pos - Vec3::Y * PLAYER_HALF.y, to).unwrap_or_default();
            }
        }
        (pos - Vec3::Y * PLAYER_HALF.y, f32::INFINITY)
    }

    /// Bots get from every spawn to every other, and without dawdling.
    #[test]
    fn bots_find_their_way_between_every_spawn() {
        let map = hamlet();
        let nav = NavGrid::new(&map);
        let feet: Vec<Vec3> = map.spawns.iter().map(|spawn| spawn.pos - Vec3::Y * (PLAYER_HALF.y + 0.05)).collect();
        for (a, from) in feet.iter().enumerate() {
            for to in &feet[a + 1..] {
                assert!(nav.path(&map, *from, *to).is_some(), "no way from {from} to {to}");
                let (end, seconds) = run_like_a_bot(&map, &nav, *from, *to);
                assert!(seconds.is_finite(), "running from {from} to {to} ended at {end}");
                // Twice as long as running straight there would, plus a bit.
                let straight = from.distance(*to) / 7.0;
                assert!(seconds < straight * 2.5 + 3.0, "{from} to {to} took {seconds:.1} s");
            }
        }
    }

    /// What the town is for: cover everywhere, and no lane long enough to
    /// snipe down. (Depot has 90 spots in the open and lines over 60 m.)
    #[test]
    fn cover_is_never_far_and_sightlines_stay_short() {
        let map = hamlet();
        let survey = survey(&map);
        assert!(survey.open.is_empty(), "out in the open at {:?}", survey.open);
        let (from, to) = survey.sightlines[0];
        assert!(from.distance(to) < 42.0, "{:.1} m from {from} to {to}", from.distance(to));
    }

    #[test]
    fn the_hamlet_builds_quickly_and_the_same_every_time() {
        let started = std::time::Instant::now();
        let a = hamlet();
        let elapsed = started.elapsed();
        let b = hamlet();
        assert_eq!(a.blocks.len(), b.blocks.len());
        assert!(a.props.iter().zip(&b.props).all(|(p, q)| p.pos == q.pos && p.model == q.model));
        assert!(a.spawns.iter().zip(&b.spawns).all(|(p, q)| p.pos == q.pos && p.yaw == q.yaw));
        assert_eq!(a.terrain.as_ref().unwrap().heights, b.terrain.as_ref().unwrap().heights);
        println!("hamlet: {} blocks, {} props, {} spawns, built in {elapsed:?}", a.blocks.len(), a.props.len(), a.spawns.len());
        assert!(a.spawns.len() >= 12);
    }

    struct Survey {
        /// Spots on open ground outdoors with no cover (anything waist high)
        /// within `OPEN` metres.
        open: Vec<Vec3>,
        /// The longest clear lines at eye height between places you can
        /// stand outdoors, longest first.
        sightlines: Vec<(Vec3, Vec3)>,
    }

    const OPEN: f32 = 5.0;

    /// Measures the town on a 1 m grid over the ground outdoors.
    fn survey(map: &Map) -> Survey {
        let mut spots = Vec::new();
        for zi in -33..=33 {
            for xi in -33..=33 {
                let (x, z) = (xi as f32 + 0.5, zi as f32 + 0.5);
                let feet = Vec3::new(x, map.ground(x, z).unwrap_or(0.0), z);
                let roofed = map.raycast(feet + Vec3::Y * 2.4, Vec3::Y, 12.0).is_some();
                if !roofed && map.is_free(feet + Vec3::Y * (PLAYER_HALF.y + 0.05), PLAYER_HALF) {
                    spots.push(feet);
                }
            }
        }
        let open = spots
            .iter()
            .copied()
            .filter(|feet| {
                (0..24).all(|k| {
                    let dir = look_dir(k as f32 * PI / 12.0, 0.0);
                    map.raycast(*feet + Vec3::Y * 0.9, dir, OPEN).is_none()
                })
            })
            .collect();
        // Players standing at two spots (every other metre) who can see each
        // other's eyes, along a line that is wide (not a slit through two
        // windows); keep the longest, only one per pair of ends 4 m apart.
        let eyes: Vec<Vec3> = spots
            .iter()
            .filter(|feet| (feet.x - 0.5).rem_euclid(2.0) < 0.1 && (feet.z - 0.5).rem_euclid(2.0) < 0.1)
            .map(|feet| *feet + Vec3::Y * 1.6)
            .collect();
        let mut lines: Vec<(f32, Vec3, Vec3)> = Vec::new();
        for (index, a) in eyes.iter().enumerate() {
            for b in &eyes[index + 1..] {
                let length = a.distance(*b);
                if length < 30.0 || !map.line_of_sight(*a, *b) {
                    continue;
                }
                let dir = (*b - *a) / length;
                let side = Vec3::new(-dir.z, 0.0, dir.x).normalize() * 0.6;
                if map.line_of_sight(*a + side, *b + side) && map.line_of_sight(*a - side, *b - side) {
                    lines.push((length, *a, *b));
                }
            }
        }
        lines.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut sightlines: Vec<(Vec3, Vec3)> = Vec::new();
        for (_, from, to) in lines {
            let same = sightlines.iter().any(|(a, b)| {
                (a.distance(from) < 4.0 && b.distance(to) < 4.0) || (a.distance(to) < 4.0 && b.distance(from) < 4.0)
            });
            if !same {
                sightlines.push((from, to));
            }
            if sightlines.len() >= 40 {
                break;
            }
        }
        Survey { open, sightlines }
    }

    #[test]
    #[ignore]
    fn print_survey() {
        for map in [Map::depot(), hamlet()] {
            let started = std::time::Instant::now();
            let survey = survey(&map);
            println!("map {}: surveyed in {:?}", map.id, started.elapsed());
            println!("{} open spots", survey.open.len());
            for (from, to) in survey.sightlines.iter().take(15) {
                println!("{:5.1} m from ({:.0}, {:.0}) to ({:.0}, {:.0})", from.distance(*to), from.x, from.z, to.x, to.z);
            }
        }
    }

    /// Draws the map from above into a PPM file (set HAMLET_PREVIEW to its
    /// path): walls dark, chest-high cover orange, waist-high cover yellow,
    /// floors and steps tan, the ground green to brown by height with a
    /// contour every half metre, roofed floor darker, spawns red, armor blue.
    #[test]
    #[ignore]
    fn preview() {
        let Ok(path) = std::env::var("HAMLET_PREVIEW") else {
            return;
        };
        let map = hamlet();
        const PX: f32 = 10.0;
        let half = PLAY + 3.0;
        let size = (half * 2.0 * PX) as usize;
        let mut pixels = vec![[0u8; 3]; size * size];
        let probe = Vec3::splat(0.04);
        for j in 0..size {
            for i in 0..size {
                let x = -half + (i as f32 + 0.5) / PX;
                let z = -half + (j as f32 + 0.5) / PX;
                let g = map.ground(x, z).unwrap();
                let blocked = |h: f32| !map.is_free(Vec3::new(x, g + h, z), probe);
                let roofed = map.raycast(Vec3::new(x, g + 2.4, z), Vec3::Y, 30.0).is_some();
                let color: [f32; 3] = if blocked(1.9) && blocked(0.9) {
                    if blocked(3.0) { [0.18, 0.18, 0.2] } else { [0.32, 0.30, 0.3] }
                } else if blocked(0.9) && blocked(1.4) {
                    [0.85, 0.45, 0.15]
                } else if blocked(0.9) || blocked(0.6) {
                    [0.95, 0.8, 0.25]
                } else if blocked(1.9) {
                    [0.55, 0.65, 0.8]
                } else if blocked(0.25) || blocked(0.1) {
                    if roofed { [0.55, 0.48, 0.38] } else { [0.8, 0.72, 0.58] }
                } else {
                    let t = (g / 3.0).clamp(0.0, 1.0);
                    let base = [0.35 + 0.35 * t, 0.55 - 0.1 * t, 0.3 - 0.1 * t];
                    let contour = (g * 2.0).fract() < 0.06;
                    let k = if roofed { 0.6 } else if contour { 0.8 } else { 1.0 };
                    base.map(|c| c * k)
                };
                let grid = (x.rem_euclid(10.0) < 0.1 || z.rem_euclid(10.0) < 0.1) as u8 as f32 * 0.1;
                pixels[j * size + i] = color.map(|c| ((c - grid).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
        let mut dot = |at: Vec3, r: f32, color: [u8; 3]| {
            let (ci, cj) = ((at.x + half) * PX, (at.z + half) * PX);
            for j in (cj - r * PX) as usize..=(cj + r * PX) as usize {
                for i in (ci - r * PX) as usize..=(ci + r * PX) as usize {
                    if j < size && i < size && ((i as f32 - ci).powi(2) + (j as f32 - cj).powi(2)).sqrt() <= r * PX {
                        pixels[j * size + i] = color;
                    }
                }
            }
        };
        let survey = survey(&map);
        for at in &survey.open {
            dot(*at, 0.3, [230, 40, 230]);
        }
        for (from, to) in survey.sightlines.iter().take(12) {
            let steps = (from.distance(*to) / 0.1) as usize;
            for k in 0..=steps {
                dot(from.lerp(*to, k as f32 / steps as f32), 0.08, [40, 230, 230]);
            }
        }
        for spawn in &map.spawns {
            dot(spawn.pos, 0.6, [220, 30, 30]);
            for k in 1..12 {
                dot(spawn.pos + look_dir(spawn.yaw, 0.0) * k as f32 * 0.15, 0.15, [220, 30, 30]);
            }
        }
        for spot in &map.armor_spots {
            dot(*spot, 0.6, [40, 120, 255]);
        }
        let mut out = format!("P6 {size} {size} 255\n").into_bytes();
        out.extend(pixels.iter().flatten());
        std::fs::write(&path, out).unwrap();
        println!("wrote {path}");
    }
}
