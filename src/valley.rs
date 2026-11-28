//! Valley: the big outdoor map, made for conquest. A village with a chapel
//! in the middle, a farm and a factory either side of it, two fortified
//! hills, a creek and roads between them all, the teams' bases at the west
//! and east ends, woods and rocks everywhere else, mountains all round.
//!
//! Everything is laid out here in code, the same on the server and every
//! client: the ground's heights and colours, where every model stands, the
//! capture points and the spawns.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::math::{Vec2, Vec3};

use crate::{
    map::{Block, FlagPoint, Look, Map, PLAYER_HALF, Road, Spawn, spawn_ring},
    props::*,
    terrain::{Terrain, fractal},
};

/// Half the side of the square that is played in. Invisible walls stand
/// just outside it; the ground beyond rises into mountains.
const PLAY: f32 = 150.0;
const WALL: f32 = 153.0;
/// The ground grid: 481 m across in 2 m cells.
const GRID: usize = 241;
const CELL: f32 = 2.0;

/// The capture points (x, z).
const FLAGS: [(char, f32, f32); 5] = [
    ('A', -75.0, 48.0),
    ('B', -50.0, -62.0),
    ('C', 0.0, -10.0),
    ('D', 50.0, 62.0),
    ('E', 75.0, -48.0),
];
/// Each team's base (x, z).
const BASES: [(f32, f32); 2] = [(-132.0, 0.0), (132.0, 0.0)];

/// Gentle hills (x, z, height, radius).
const HILLS: [(f32, f32, f32, f32); 8] = [
    (-50.0, -65.0, 13.0, 36.0),
    (50.0, 65.0, 13.0, 36.0),
    (-100.0, -35.0, 6.0, 28.0),
    (100.0, 35.0, 6.0, 28.0),
    (-20.0, 100.0, 5.0, 30.0),
    (20.0, -100.0, 5.0, 30.0),
    (-110.0, 95.0, 8.0, 30.0),
    (110.0, -95.0, 8.0, 30.0),
];

const MAIN_ROAD: [(f32, f32); 12] = [
    (-180.0, 6.0),
    (-130.0, 2.0),
    (-95.0, 10.0),
    (-55.0, 9.0),
    (-25.0, 3.0),
    (0.0, 0.0),
    (12.0, -2.0),
    (38.0, -2.0),
    (55.0, -9.0),
    (95.0, -10.0),
    (130.0, -2.0),
    (180.0, -6.0),
];
const DIRT_ROADS: [&[(f32, f32)]; 6] = [
    &[(-22.0, 3.0), (-40.0, 25.0), (-60.0, 42.0), (-75.0, 48.0), (-105.0, 32.0), (-128.0, 4.0)],
    &[(40.0, -3.0), (50.0, -25.0), (62.0, -42.0), (75.0, -48.0), (105.0, -32.0), (128.0, -4.0)],
    &[(-55.0, 9.0), (-62.0, -15.0), (-55.0, -42.0), (-50.0, -62.0)],
    &[(55.0, -9.0), (62.0, 15.0), (55.0, 42.0), (50.0, 62.0)],
    &[(-75.0, 48.0), (-78.0, 75.0), (-70.0, 100.0)],
    &[(75.0, -48.0), (78.0, -75.0), (70.0, -100.0)],
];
/// The creek's centre line, north to south. It passes under the main road
/// at the bridge.
const CREEK: [(f32, f32); 8] = [
    (20.0, -200.0),
    (28.0, -120.0),
    (18.0, -70.0),
    (26.0, -30.0),
    (25.0, -2.0),
    (16.0, 30.0),
    (26.0, 80.0),
    (20.0, 200.0),
];
const BRIDGE_AT: (f32, f32) = (25.0, -2.0);
/// How far the creek's bed lies below its banks, and its water above the bed.
const CREEK_DEPTH: f32 = 2.2;
const WATER_DEPTH: f32 = 0.55;
/// Wide enough that the water's edges tuck under the banks: the banks reach
/// its surface about 4.8 m out from the middle.
const WATER_WIDTH: f32 = 11.0;

/// Flattened ground: building sites, flags, bases (centre, half size, round).
struct Pad {
    center: Vec2,
    half: Vec2,
    round: bool,
}

/// A building or prop that needs flat ground (model, x, z, yaw).
type Placed = (usize, f32, f32, f32);

const S: f32 = PI;
const E: f32 = FRAC_PI_2;
const W: f32 = -FRAC_PI_2;

/// Buildings. Yaw 0 faces their front south (+z), `S` north, `E` east, `W` west.
const BUILDINGS: &[Placed] = &[
    // The village, along the main road, round the chapel's square.
    (CHAPEL, 0.0, -30.0, 0.0),
    (HOUSE_TALL, -17.0, -14.0, 0.0),
    (HOUSE, -30.0, -12.0, 0.0),
    (HOUSE_WARM, 12.0, -16.0, 0.0),
    (HOUSE_TALL_BRICK, -15.0, 14.0, S),
    (HOUSE, -29.0, 14.0, S),
    (RUIN, 9.0, 14.0, S),
    (SHED, -40.0, -12.0, 0.0),
    // The farm.
    (BARN, -95.0, 57.0, 0.0),
    (HOUSE_WARM, -61.0, 64.0, W),
    (SILO, -96.0, 74.0, 0.0),
    (SHED, -76.0, 72.0, S),
    // Radio hill.
    (RADIO_MAST, -40.0, -70.0, 0.0),
    (BUNKER, -60.0, -70.0, E),
    // Bunker hill.
    (BUNKER, 60.0, 70.0, W),
    (RUIN, 40.0, 73.0, 0.0),
    (SHED, 62.0, 52.0, W),
    // The factory.
    (WAREHOUSE, 90.0, -60.0, 0.0),
    (SMOKESTACK, 100.0, -43.0, 0.0),
    (SILO, 76.0, -74.0, 0.0),
    (SILO, 82.0, -76.0, 0.0),
    // The bases.
    (TENT, -142.0, -12.0, E),
    (TENT, -142.0, 12.0, E),
    (TENT, 142.0, 12.0, W),
    (TENT, 142.0, -12.0, W),
    // Out in the fields.
    (RUIN, -105.0, -70.0, E),
    (RUIN, 105.0, 70.0, W),
    (BARN, -20.0, 62.0, E),
    (HOUSE, 42.0, -84.0, W),
    (SHED, 118.0, 48.0, 0.0),
    (SHED, -118.0, -48.0, S),
];

/// Cover and clutter, standing on whatever ground is there.
const CLUTTER: &[Placed] = &[
    // Village.
    (WELL, -3.0, 9.0, 0.0),
    (CAR, -6.0, 1.5, 0.0),
    (CAR, 46.0, -6.0, E),
    (SANDBAGS, 7.0, -9.0, E),
    (SANDBAGS, -7.0, -11.0, E),
    (CRATES, -22.0, -6.0, 0.0),
    (BARRELS, 17.0, -8.0, 0.0),
    (BARRIER, -38.0, 6.0, 0.0),
    (BARRIER, 4.0, 6.0, 0.0),
    (FENCE, -15.0, 21.0, 0.0),
    (FENCE, -19.0, 21.0, 0.0),
    (FENCE, -27.0, 21.0, 0.0),
    (FENCE, -31.0, 21.0, 0.0),
    // Farm.
    (CRATES, -86.0, 46.0, 0.0),
    (BARRELS, -88.0, 70.0, 0.0),
    (CAR, -68.0, 44.0, E),
    (HAYBALE, -86.0, 66.0, 0.0),
    (HAYBALE, -84.0, 63.0, E),
    (SANDBAGS, -75.0, 41.0, 0.0),
    (SANDBAGS, -68.0, 54.0, E),
    // Radio hill: sandbags round the flag.
    (SANDBAGS, -50.0, -71.0, 0.0),
    (SANDBAGS, -50.0, -53.0, 0.0),
    (SANDBAGS, -41.0, -60.0, E),
    (SANDBAGS, -59.0, -58.0, E),
    (CRATES, -45.0, -55.0, 0.0),
    // Bunker hill.
    (SANDBAGS, 50.0, 71.0, 0.0),
    (SANDBAGS, 50.0, 53.0, 0.0),
    (SANDBAGS, 41.0, 64.0, E),
    (SANDBAGS, 59.0, 62.0, E),
    (BARRELS, 46.0, 56.0, 0.0),
    // Factory.
    (CONTAINER, 68.0, -66.0, 0.0),
    (CONTAINER, 68.0, -62.0, 0.0),
    (CONTAINER, 78.0, -36.0, E),
    (CONTAINER, 104.0, -72.0, E),
    (CRATES, 80.0, -50.0, 0.0),
    (BARRELS, 70.0, -42.0, 0.0),
    (BARRIER, 64.0, -50.0, E),
    (BARRIER, 74.0, -58.0, 0.0),
    // The bases.
    (CONTAINER, -126.0, -18.0, 0.0),
    (CONTAINER, -126.0, 18.0, 0.0),
    (SANDBAGS, -112.0, -8.0, E),
    (SANDBAGS, -112.0, 10.0, E),
    (CRATES, -136.0, -24.0, 0.0),
    (CONTAINER, 126.0, 18.0, 0.0),
    (CONTAINER, 126.0, -18.0, 0.0),
    (SANDBAGS, 112.0, 8.0, E),
    (SANDBAGS, 112.0, -10.0, E),
    (CRATES, 136.0, 24.0, 0.0),
];

pub fn valley() -> Map {
    let mut map = Map::empty(2);
    let roads = roads();
    let creek = Polyline::new(CREEK.iter().map(|&(x, z)| Vec2::new(x, z)).collect());
    let pads = pads();

    // Ground: rolling noise and hills; roads eased into it, building sites
    // and flags levelled, and the creek cut through last.
    let shaped = |x: f32, z: f32| {
        let mut h = base(x, z);
        for road in &roads {
            let (distance, height) = road.line.nearest(Vec2::new(x, z), Some(&road.profile));
            let weight = 1.0 - smoothstep(road.width / 2.0 + 0.5, road.width / 2.0 + 5.0, distance);
            h += (height - h) * weight;
        }
        h
    };
    // Each site is levelled at the height the ground has there once the
    // sites before it are, so a site within another stays level with it.
    let mut pad_heights: Vec<f32> = Vec::new();
    for (index, pad) in pads.iter().enumerate() {
        let mut h = shaped(pad.center.x, pad.center.y);
        for (before, target) in pads[..index].iter().zip(&pad_heights) {
            h += (target - h) * before.weight(pad.center, 5.0);
        }
        pad_heights.push(h);
    }
    let levelled = |x: f32, z: f32| {
        let mut h = shaped(x, z);
        for (pad, target) in pads.iter().zip(&pad_heights) {
            h += (target - h) * pad.weight(Vec2::new(x, z), 5.0);
        }
        h
    };
    // The creek's banks stand at the ground's height along it, smoothed, the
    // same on both sides, so its bed is level across and the water meets
    // both banks even where it runs along a hillside.
    let banks = profile(&creek, levelled);
    let mut terrain = Terrain::generate(GRID, CELL, |x, z| {
        let (distance, bank) = creek.nearest(Vec2::new(x, z), Some(&banks));
        let channel = bank - CREEK_DEPTH * (1.0 - smoothstep(3.5, 7.5, distance));
        channel + (levelled(x, z) - channel) * smoothstep(7.5, 13.0, distance)
    });
    terrain.colors = ground_colors(&terrain, &roads, &creek, &pads);

    map.roads = roads
        .iter()
        .map(|road| Road {
            points: road.line.points.clone(),
            width: road.width,
            paved: road.paved,
        })
        .collect();
    map.water = vec![(water_line(&creek, &banks), WATER_WIDTH)];
    let ground = |x: f32, z: f32| terrain.height(x, z);

    // The bridge sits on the road where the creek crosses it.
    let (bx, bz) = BRIDGE_AT;
    map.add_prop(BRIDGE, Vec3::new(bx, shaped(bx, bz) - 0.15, bz), E, 1.0);
    for &(model, x, z, yaw) in BUILDINGS.iter().chain(CLUTTER) {
        map.add_prop(model, Vec3::new(x, ground(x, z), z), yaw, 1.0);
    }
    for (_, x, z) in FLAGS {
        map.add_prop(FLAGPOLE, Vec3::new(x, ground(x, z), z), 0.0, 1.0);
    }
    // Lamps along the village street, poles along the main road.
    for x in [-36.0, -24.0, -12.0, 0.0, 12.0] {
        map.add_prop(LAMP_POST, Vec3::new(x, ground(x, -5.5), -5.5 + x * -0.02), 0.0, 1.0);
    }
    let main = &roads[0].line;
    let mut along = 20.0;
    while along < main.length() - 20.0 {
        let (point, side) = main.at(along);
        let pole = point + side * 7.0;
        let in_town = pole.x > -50.0 && pole.x < 45.0 && pole.y.abs() < 30.0;
        if !in_town && pole.x.abs() < 175.0 && clear(pole, &roads, &creek, &pads, 0.0, true) {
            map.add_prop(POWER_POLE, Vec3::new(pole.x, ground(pole.x, pole.y), pole.y), 0.0, 1.0);
        }
        along += 32.0;
    }

    scatter(&mut map, &terrain, &roads, &creek, &pads);

    // Invisible walls round the edge.
    for (min, max) in [
        ([-WALL - 5.0, -100.0, -WALL - 5.0], [-WALL, 400.0, WALL + 5.0]),
        ([WALL, -100.0, -WALL - 5.0], [WALL + 5.0, 400.0, WALL + 5.0]),
        ([-WALL, -100.0, -WALL - 5.0], [WALL, 400.0, -WALL]),
        ([-WALL, -100.0, WALL], [WALL, 400.0, WALL + 5.0]),
    ] {
        map.blocks.push(Block {
            min: Vec3::from(min),
            max: Vec3::from(max),
            color: [0.0; 3],
            visible: false,
        });
    }

    map.terrain = Some(terrain);
    map.bounds = [Vec2::splat(-PLAY + 10.0), Vec2::splat(PLAY - 10.0)];
    map.look = Look {
        sky: [0.62, 0.75, 0.90],
        fog: Some((90.0, 460.0)),
        sun_from: Vec3::new(140.0, 190.0, 70.0),
        sun: 10000.0,
        sun_color: [1.0; 3],
        ambient: 420.0,
        orbit: (Vec3::new(0.0, 8.0, -10.0), 95.0, 45.0),
    };
    let mut map = map.finish();

    // Spawns round each flag and in each base, checked against what now stands there.
    let mut flags = Vec::new();
    for (name, x, z) in FLAGS {
        let pos = Vec3::new(x, map.ground(x, z).unwrap_or(0.0), z);
        let spawns = spawn_ring(&map, Vec2::new(x, z), 7.0, 16.0, 8);
        flags.push(FlagPoint { name, pos, radius: 11.0, spawns });
    }
    for (team, (x, z)) in BASES.iter().enumerate() {
        map.bases[team] = spawn_ring(&map, Vec2::new(*x + if team == 0 { 6.0 } else { -6.0 }, *z), 2.0, 14.0, 12);
    }
    map.spawns = flags
        .iter()
        .flat_map(|flag| &flag.spawns)
        .chain(map.bases.iter().flatten())
        .map(|spawn| Spawn { pos: spawn.pos, yaw: spawn.yaw })
        .collect();
    map.hill = flags[2].pos;
    // Armor: by every flag, and up in the chapel's belfry.
    map.armor_spots = flags
        .iter()
        .filter_map(|flag| {
            (0..8).map(|i| i as f32 * PI / 4.0).find_map(|angle| {
                let at = Vec2::new(flag.pos.x, flag.pos.z) + Vec2::from_angle(angle) * 3.5;
                let spot = Vec3::new(at.x, map.ground(at.x, at.y).unwrap_or(0.0), at.y);
                map.is_free(spot + Vec3::Y * (PLAYER_HALF.y + 0.01), PLAYER_HALF).then_some(spot)
            })
        })
        .collect();
    let (_, cx, cz, cyaw) = BUILDINGS[0];
    let chapel = Vec3::new(cx, map.ground(cx, cz).unwrap_or(0.0), cz);
    map.armor_spots.push(Map::prop_point(CHAPEL, "belfry", chapel, cyaw));
    map.flags = flags;
    map
}

/// The rolling ground before roads and sites are levelled into it.
fn base(x: f32, z: f32) -> f32 {
    let mut h = 4.0 * fractal(x / 90.0, z / 90.0, 4, 11) + 1.0 * fractal(x / 23.0, z / 23.0, 2, 23);
    for (cx, cz, height, radius) in HILLS {
        let d2 = ((x - cx).powi(2) + (z - cz).powi(2)) / (radius * radius);
        h += height * (-d2).exp();
    }
    // Mountains all round, starting just inside the walls.
    let edge = x.abs().max(z.abs());
    let rise = ((edge - 138.0) / 60.0).clamp(0.0, 1.5);
    h + rise * rise * 50.0 * (0.8 + 0.4 * fractal(x / 45.0, z / 45.0, 3, 5))
}

struct RoadShape {
    line: Polyline,
    width: f32,
    paved: bool,
    /// Ground height along the road, every `PROFILE_STEP` m, smoothed, so it
    /// climbs gently.
    profile: Vec<f32>,
}

const PROFILE_STEP: f32 = 2.0;

fn roads() -> Vec<RoadShape> {
    let shape = |points: &[(f32, f32)], width: f32, paved: bool| {
        let line = Polyline::new(points.iter().map(|&(x, z)| Vec2::new(x, z)).collect());
        let profile = profile(&line, base);
        RoadShape {
            line,
            width,
            paved,
            profile,
        }
    };
    let mut roads = vec![shape(&MAIN_ROAD, 7.0, true)];
    roads.extend(DIRT_ROADS.iter().map(|points| shape(points, 4.0, false)));
    roads
}

/// The height of the ground along a line, every `PROFILE_STEP` m, averaged
/// over 20 m either way so it climbs gently.
fn profile(line: &Polyline, height: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let samples = (line.length() / PROFILE_STEP).ceil() as usize + 1;
    let raw: Vec<f32> = (0..samples)
        .map(|i| {
            let (p, _) = line.at(i as f32 * PROFILE_STEP);
            height(p.x, p.y)
        })
        .collect();
    (0..samples)
        .map(|i| {
            let lo = i.saturating_sub(10);
            let hi = (i + 10).min(samples - 1);
            raw[lo..=hi].iter().sum::<f32>() / (hi - lo + 1) as f32
        })
        .collect()
}

fn pads() -> Vec<Pad> {
    let mut pads = vec![
        // The village and the farm and factory yards are each one level site.
        Pad { center: Vec2::new(-9.0, -10.0), half: Vec2::new(36.0, 32.0), round: false },
        Pad { center: Vec2::new(-80.0, 60.0), half: Vec2::new(24.0, 18.0), round: false },
        Pad { center: Vec2::new(85.0, -58.0), half: Vec2::new(23.0, 22.0), round: false },
    ];
    for (_, x, z) in FLAGS {
        pads.push(Pad { center: Vec2::new(x, z), half: Vec2::splat(16.0), round: true });
    }
    for (x, z) in BASES {
        pads.push(Pad { center: Vec2::new(x, z), half: Vec2::splat(28.0), round: true });
    }
    // Every building outside those gets a site of its own.
    for &(model, x, z, yaw) in BUILDINGS {
        let center = Vec2::new(x, z);
        if pads.iter().take(3).any(|pad| pad.weight(center, 0.0) > 0.0) {
            continue;
        }
        let half = footprint(model, yaw) + Vec2::splat(2.0);
        pads.push(Pad { center, half, round: false });
    }
    pads
}

/// Half the size of a model's collision boxes seen from above, turned by `yaw`.
fn footprint(model: usize, yaw: f32) -> Vec2 {
    let mut half = Vec2::ZERO;
    for [x0, _, z0, x1, _, z1] in PROPS[model].colliders {
        half = half.max(Vec2::new(x0.abs().max(x1.abs()), z0.abs().max(z1.abs())));
    }
    if (yaw / FRAC_PI_2).round().rem_euclid(2.0) == 1.0 { Vec2::new(half.y, half.x) } else { half }
}

impl Pad {
    /// 1 on the pad, easing to 0 over `margin` metres round it.
    fn weight(&self, at: Vec2, margin: f32) -> f32 {
        let outside = if self.round {
            (at.distance(self.center) - self.half.x).max(0.0)
        } else {
            let d = ((at - self.center).abs() - self.half).max(Vec2::ZERO);
            d.length()
        };
        if margin <= 0.0 {
            return if outside <= 0.0 { 1.0 } else { 0.0 };
        }
        1.0 - smoothstep(0.0, margin, outside)
    }
}

/// True if a tree or rock can stand at `at`: off the roads, out of the
/// creek, clear of every site, `margin` metres to spare.
fn clear(at: Vec2, roads: &[RoadShape], creek: &Polyline, pads: &[Pad], margin: f32, near_roads: bool) -> bool {
    let road_room = if near_roads { 1.5 } else { 3.0 };
    roads.iter().all(|road| road.line.nearest(at, None).0 > road.width / 2.0 + road_room + margin)
        && creek.nearest(at, None).0 > 7.0 + margin
        && pads.iter().all(|pad| pad.weight(at, margin + 3.0) == 0.0)
}

/// Woods, lone trees, bushes and rocks, wherever there is room.
fn scatter(map: &mut Map, terrain: &Terrain, roads: &[RoadShape], creek: &Polyline, pads: &[Pad]) {
    let mut rng = Rng(0x5eed_1234_abcd_ef01);
    let mut taken = std::collections::HashSet::new();
    let reach = 228.0;
    let cell_free = |at: Vec2, size: f32, taken: &mut std::collections::HashSet<(i32, i32)>| {
        let key = ((at.x / size).floor() as i32, (at.y / size).floor() as i32);
        taken.insert(key)
    };

    for _ in 0..9000 {
        let at = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let edge = at.x.abs().max(at.y.abs());
        // Woods where the noise says so, a lone tree here and there elsewhere.
        let wood = fractal(at.x / 55.0, at.y / 55.0, 3, 41);
        let chance = if edge > PLAY { 0.55 } else if wood > 0.12 { 0.5 } else { 0.03 };
        if rng.unit() > chance || !clear(at, roads, creek, pads, 0.0, false) || !cell_free(at, 4.5, &mut taken) {
            continue;
        }
        let high = terrain.height(at.x, at.y) > 7.0 || edge > PLAY;
        let pick = rng.unit();
        let model = if high || wood > 0.3 {
            if pick < 0.6 { PINE } else if pick < 0.85 { PINE_SMALL } else { OAK }
        } else if pick < 0.45 {
            OAK
        } else if pick < 0.75 {
            BIRCH
        } else if pick < 0.9 {
            PINE
        } else {
            PINE_SMALL
        };
        let scale = rng.range(0.8, 1.25);
        let y = terrain.height(at.x, at.y) - 0.15;
        map.add_prop(model, Vec3::new(at.x, y, at.y), rng.range(0.0, 2.0 * PI), scale);
        // Undergrowth beside some of them.
        if rng.unit() < 0.35 {
            let bush = at + Vec2::from_angle(rng.range(0.0, 2.0 * PI)) * rng.range(2.0, 4.0);
            if clear(bush, roads, creek, pads, 0.0, true) {
                let y = terrain.height(bush.x, bush.y) - 0.1;
                map.add_prop(BUSH, Vec3::new(bush.x, y, bush.y), rng.range(0.0, 2.0 * PI), rng.range(0.7, 1.3));
            }
        }
    }

    // Rocks: more on slopes and hilltops, cliffs in the mountains.
    let mut placed = 0;
    for _ in 0..4000 {
        if placed > 260 {
            break;
        }
        let at = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let edge = at.x.abs().max(at.y.abs());
        let steep = 1.0 - terrain.normal(at.x, at.y).y;
        let chance = 0.04 + steep * 3.0 + if edge > PLAY - 5.0 { 0.3 } else { 0.0 };
        if rng.unit() > chance || !clear(at, roads, creek, pads, 1.0, false) || !cell_free(at, 6.0, &mut taken) {
            continue;
        }
        let pick = rng.unit();
        let model = if edge > PLAY + 8.0 && pick < 0.5 {
            CLIFF
        } else if pick < 0.3 {
            ROCK_LARGE
        } else if pick < 0.6 {
            ROCK_MEDIUM
        } else if pick < 0.8 {
            ROCK_FLAT
        } else {
            ROCK_SMALL
        };
        // Sit it on the lowest ground under it, so no side floats.
        let size = if model == CLIFF { 4.0 } else { 1.5 };
        let y = [(0.0, 0.0), (size, 0.0), (-size, 0.0), (0.0, size), (0.0, -size)]
            .iter()
            .map(|(dx, dz)| terrain.height(at.x + dx, at.y + dz))
            .fold(f32::INFINITY, f32::min);
        let quarter = (rng.unit() * 4.0).floor() * FRAC_PI_2;
        map.add_prop(model, Vec3::new(at.x, y, at.y), quarter, rng.range(0.8, 1.3));
        placed += 1;
    }

    // Bushes along the edges of things.
    for _ in 0..700 {
        let at = Vec2::new(rng.range(-PLAY, PLAY), rng.range(-PLAY, PLAY));
        if !clear(at, roads, creek, pads, -1.0, true) || rng.unit() < 0.5 {
            continue;
        }
        let y = terrain.height(at.x, at.y) - 0.1;
        map.add_prop(BUSH, Vec3::new(at.x, y, at.y), rng.range(0.0, 2.0 * PI), rng.range(0.6, 1.2));
    }
}

/// The creek's water: along its centre line, a little above the bed.
fn water_line(creek: &Polyline, banks: &[f32]) -> Vec<Vec3> {
    let mut points = Vec::new();
    let mut along = 0.0;
    while along <= creek.length() {
        let (p, _) = creek.at(along);
        let (_, bank) = creek.nearest(p, Some(banks));
        points.push(Vec3::new(p.x, bank - CREEK_DEPTH + WATER_DEPTH, p.y));
        along += 3.0;
    }
    points
}

/// Colours of the ground (sRGB) at each vertex: grass in patches, rock on
/// steep slopes and up the mountains, dirt on the roads and building sites,
/// mud and sand along the creek.
fn ground_colors(terrain: &Terrain, roads: &[RoadShape], creek: &Polyline, pads: &[Pad]) -> Vec<[f32; 3]> {
    const GRASS: Vec3 = Vec3::new(0.31, 0.42, 0.17);
    const GRASS_DARK: Vec3 = Vec3::new(0.19, 0.30, 0.11);
    const DRY: Vec3 = Vec3::new(0.47, 0.45, 0.25);
    const ROCK: Vec3 = Vec3::new(0.45, 0.43, 0.39);
    const DIRT: Vec3 = Vec3::new(0.47, 0.38, 0.26);
    const GRAVEL: Vec3 = Vec3::new(0.42, 0.41, 0.38);
    const MUD: Vec3 = Vec3::new(0.30, 0.26, 0.20);
    const SAND: Vec3 = Vec3::new(0.58, 0.53, 0.40);
    let mut colors = Vec::with_capacity(terrain.size * terrain.size);
    for j in 0..terrain.size {
        for i in 0..terrain.size {
            let v = terrain.vertex(i, j);
            let at = Vec2::new(v.x, v.z);
            let patch = fractal(at.x / 28.0, at.y / 28.0, 3, 77) * 0.35 + fractal(at.x / 7.0, at.y / 7.0, 2, 78) * 0.25 + 0.5;
            let mut c = GRASS_DARK.lerp(GRASS, patch.clamp(0.0, 1.0));
            c = c.lerp(DRY, smoothstep(0.25, 0.6, fractal(at.x / 70.0, at.y / 70.0, 2, 91)) * 0.7);
            let steep = 1.0 - terrain.normal(at.x, at.y).y;
            let edge = at.x.abs().max(at.y.abs());
            let rocky = smoothstep(0.12, 0.3, steep).max(smoothstep(PLAY + 10.0, PLAY + 60.0, edge) * 0.8);
            c = c.lerp(ROCK, rocky);
            for pad in pads {
                c = c.lerp(DIRT, pad.weight(at, 3.0) * 0.45);
            }
            for road in roads {
                let distance = road.line.nearest(at, None).0;
                let weight = 1.0 - smoothstep(road.width / 2.0 - 0.5, road.width / 2.0 + 1.5, distance);
                c = c.lerp(if road.paved { GRAVEL } else { DIRT }, weight);
            }
            let wet = creek.nearest(at, None).0;
            c = c.lerp(SAND, 1.0 - smoothstep(4.0, 7.5, wet));
            c = c.lerp(MUD, 1.0 - smoothstep(2.5, 4.0, wet));
            colors.push(c.to_array());
        }
    }
    colors
}

/// A line through points on the ground, with its length worked out.
struct Polyline {
    points: Vec<Vec2>,
    /// Distance along the line to each point.
    lengths: Vec<f32>,
}

impl Polyline {
    fn new(points: Vec<Vec2>) -> Polyline {
        let mut lengths = vec![0.0];
        for pair in points.windows(2) {
            lengths.push(lengths.last().unwrap() + pair[0].distance(pair[1]));
        }
        Polyline { points, lengths }
    }

    fn length(&self) -> f32 {
        *self.lengths.last().unwrap()
    }

    /// The point `along` metres down the line, and the direction to its left.
    fn at(&self, along: f32) -> (Vec2, Vec2) {
        let along = along.clamp(0.0, self.length());
        let segment = self.lengths.windows(2).position(|l| along <= l[1]).unwrap_or(self.points.len() - 2);
        let (a, b) = (self.points[segment], self.points[segment + 1]);
        let t = (along - self.lengths[segment]) / (self.lengths[segment + 1] - self.lengths[segment]).max(1e-6);
        let dir = (b - a).normalize_or_zero();
        (a.lerp(b, t), Vec2::new(dir.y, -dir.x))
    }

    /// Distance from `at` to the line, and if a profile (heights every
    /// `PROFILE_STEP` m along it) is given, its height at the nearest point.
    fn nearest(&self, at: Vec2, profile: Option<&[f32]>) -> (f32, f32) {
        let mut best = (f32::INFINITY, 0.0);
        for (segment, pair) in self.points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let ab = b - a;
            let t = ((at - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let distance = at.distance(a + ab * t);
            if distance < best.0 {
                best = (distance, self.lengths[segment] + t * ab.length());
            }
        }
        let (distance, along) = best;
        let height = profile.map_or(0.0, |profile| {
            let f = along / PROFILE_STEP;
            let i = (f.floor() as usize).min(profile.len() - 1);
            let j = (i + 1).min(profile.len() - 1);
            profile[i] + (profile[j] - profile[i]) * (f - i as f32)
        });
        (distance, height)
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{STEP_UP, look_dir};

    /// Walks a player through `waypoints` (x, z), as the player controller
    /// would, and returns where their feet end up.
    fn walk(map: &Map, start: Vec3, waypoints: &[(f32, f32)]) -> Vec3 {
        let dt = 1.0 / 60.0;
        let mut pos = start + Vec3::Y * PLAYER_HALF.y;
        let mut vel = Vec3::ZERO;
        let mut grounded = true;
        for &(x, z) in waypoints {
            for _ in 0..600 {
                let to = Vec2::new(x - pos.x, z - pos.z);
                if to.length() < 0.15 {
                    break;
                }
                let run = to.normalize() * 4.0;
                vel = Vec3::new(run.x, vel.y - 22.0 * dt, run.y);
                let was = grounded;
                grounded = map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, dt);
                if was && !grounded && vel.y <= 0.0 && map.snap_to_ground(&mut pos, PLAYER_HALF, STEP_UP) {
                    grounded = true;
                    vel.y = 0.0;
                }
            }
        }
        pos - Vec3::Y * PLAYER_HALF.y
    }

    /// A flat map with one model standing at the origin.
    fn just(model: usize) -> Map {
        let mut map = Map::empty(0);
        map.terrain = Some(Terrain::generate(41, 2.0, |_, _| 0.0));
        map.add_prop(model, Vec3::ZERO, 0.0, 1.0);
        map.finish()
    }

    /// Blender's (x, y) on a model is the game's (x, -y).
    fn route(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
        points.iter().map(|&(x, y)| (x, -y)).collect()
    }

    #[test]
    fn the_stairs_in_every_building_go_up() {
        // (model, route from outside in and up the stairs, floor at the top)
        let cases: [(usize, &[(f32, f32)], f32); 4] = [
            (HOUSE_TALL, &[(-2.4, -5.0), (-2.4, -2.75), (-3.65, -2.75), (-3.65, 1.6), (0.0, 1.6)], 3.5),
            (BARN, &[(0.0, -9.5), (0.0, -3.6), (-4.25, -3.6), (-4.25, 2.5), (0.0, 4.0)], 3.0),
            (WAREHOUSE, &[(3.0, -12.5), (3.0, -10.3), (6.1, -10.3), (6.1, -5.0), (5.0, 0.0)], 3.4),
            (
                CHAPEL,
                &[(0.0, -13.5), (0.0, -11.0), (-2.0, -11.0), (-2.0, -7.0), (2.0, -7.0), (2.0, -11.0), (-2.0, -11.0), (0.0, -8.4)],
                10.0,
            ),
        ];
        for (model, points, floor) in cases {
            let map = just(model);
            let route = route(points);
            let start = Vec3::new(route[0].0, 0.0, route[0].1);
            let feet = walk(&map, start, &route[1..]);
            let goal = route.last().unwrap();
            assert!(
                (feet.y - floor).abs() < 0.05 && Vec2::new(feet.x - goal.0, feet.z - goal.1).length() < 0.3,
                "{}: ended at {feet}, wanted {goal:?} at {floor}",
                PROPS[model].name,
            );
        }
    }

    #[test]
    fn the_valley_is_sound() {
        let map = valley();
        assert_eq!(map.flags.len(), 5);
        for flag in &map.flags {
            assert!(flag.spawns.len() >= 6, "flag {} has {} spawns", flag.name, flag.spawns.len());
        }
        for (team, base) in map.bases.iter().enumerate() {
            assert!(base.len() >= 10, "base {team} has {} spawns", base.len());
        }
        // Every spawn faces somewhere it can see a little way.
        for spawn in &map.spawns {
            let eye = spawn.pos + Vec3::Y * 0.7;
            let ahead = map.raycast(eye, look_dir(spawn.yaw, 0.0), 2.0);
            assert!(ahead.is_none_or(|t| t > 0.5), "spawn {:?} faces a wall", spawn.pos);
        }
        // Nothing pokes up through a building's floor.
        let ground = |x: f32, z: f32| map.ground(x, z).unwrap();
        for prop in map.props.iter().filter(|prop| prop.model <= TENT) {
            let half = footprint(prop.model, prop.yaw) - Vec2::splat(0.5);
            for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (0.0, 0.0)] {
                let (x, z) = (prop.pos.x + dx * half.x, prop.pos.z + dz * half.y);
                let poke = ground(x, z) - prop.pos.y;
                assert!(poke < 0.15, "{} at {}: ground {poke:.2} m above its floor", PROPS[prop.model].name, prop.pos);
            }
        }
        // The bridge carries the road over the creek.
        let (bx, bz) = BRIDGE_AT;
        let deck = map.raycast(Vec3::new(bx, 30.0, bz), Vec3::NEG_Y, 60.0).unwrap();
        assert!(30.0 - deck > ground(bx, bz) + 1.0, "no bridge over the creek");
    }

    #[test]
    fn the_creek_is_full_to_its_banks() {
        let map = valley();
        let terrain = map.terrain.as_ref().unwrap();
        let (points, width) = &map.water[0];
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if a.z.abs() > PLAY {
                continue;
            }
            let at = Vec2::new(a.x, a.z);
            let along = Vec2::new(b.x - a.x, b.z - a.z).normalize();
            let side = Vec2::new(along.y, -along.x);
            assert!(terrain.height(at.x, at.y) < a.y - 0.3, "the creek is dry at {at}");
            // Both edges of the water are under the bank, so none of the bed shows.
            for edge in [at + side * (width / 2.0), at - side * (width / 2.0)] {
                let bank = terrain.height(edge.x, edge.y);
                assert!(bank > a.y + 0.1, "the water stops short of the bank at {edge}: bank {bank:.2}, water {:.2}", a.y);
            }
        }
    }

    #[test]
    fn the_valley_builds_quickly_and_the_same_every_time() {
        let started = std::time::Instant::now();
        let a = valley();
        let elapsed = started.elapsed();
        let b = valley();
        assert_eq!(a.blocks.len(), b.blocks.len());
        assert_eq!(a.props.len(), b.props.len());
        assert!(a.props.iter().zip(&b.props).all(|(p, q)| p.pos == q.pos && p.model == q.model));
        assert_eq!(a.terrain.as_ref().unwrap().heights, b.terrain.as_ref().unwrap().heights);
        println!("valley: {} blocks, {} props, built in {elapsed:?}", a.blocks.len(), a.props.len());
    }
}

