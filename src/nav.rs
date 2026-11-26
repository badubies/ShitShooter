//! Finding the way on foot, for bots: a grid of half-metre cells over the
//! part of the map that is played in, each one walkable or not at ground
//! level, and A* across it. Upper floors and roofs are left out: bots keep
//! to the ground.

use std::{cmp::Reverse, collections::BinaryHeap};

use bevy::math::{Vec2, Vec3};

use crate::map::{Map, PLAYER_HALF, STEP_UP};

const CELL: f32 = 0.5;
/// The box a bot must fit at a cell: a little narrower than a player, so
/// doors are not missed, and starting above knee height, so kerbs, steps and
/// bumps in the ground do not count.
const CLEARANCE: Vec3 = Vec3::new(0.3, 0.6, 0.3);
const ABOVE_GROUND: f32 = 0.55;
/// A* gives up after looking at this many cells (bots then run straight).
const SEARCH_LIMIT: usize = 120_000;

pub struct NavGrid {
    origin: Vec2,
    nx: usize,
    nz: usize,
    walkable: Vec<bool>,
}

impl NavGrid {
    /// Nowhere to walk at all: for a map with no bots on it.
    pub fn empty() -> NavGrid {
        NavGrid {
            origin: Vec2::ZERO,
            nx: 0,
            nz: 0,
            walkable: Vec::new(),
        }
    }

    pub fn new(map: &Map) -> NavGrid {
        let [min, max] = map.bounds;
        let (min, max) = (min - Vec2::splat(8.0), max + Vec2::splat(8.0));
        let nx = ((max.x - min.x) / CELL).ceil() as usize;
        let nz = ((max.y - min.y) / CELL).ceil() as usize;
        let mut grid = NavGrid {
            origin: min,
            nx,
            nz,
            walkable: vec![false; nx * nz],
        };
        for j in 0..nz {
            for i in 0..nx {
                let at = grid.center(map, i, j);
                let body = at + Vec3::Y * (ABOVE_GROUND + CLEARANCE.y);
                let steep = map.terrain.is_some() && !map.walkable_ground(at.x, at.z);
                grid.walkable[j * nx + i] = !steep && map.is_free(body, CLEARANCE);
            }
        }
        grid
    }

    fn center(&self, map: &Map, i: usize, j: usize) -> Vec3 {
        let x = self.origin.x + (i as f32 + 0.5) * CELL;
        let z = self.origin.y + (j as f32 + 0.5) * CELL;
        Vec3::new(x, map.ground(x, z).unwrap_or(0.0), z)
    }

    fn cell(&self, at: Vec3) -> Option<(usize, usize)> {
        let i = ((at.x - self.origin.x) / CELL).floor();
        let j = ((at.z - self.origin.y) / CELL).floor();
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.nx && (j as usize) < self.nz).then_some((i as usize, j as usize))
    }

    fn open(&self, i: usize, j: usize) -> bool {
        self.walkable[j * self.nx + i]
    }

    /// The walkable cell nearest (i, j), looking at most `reach` cells away.
    fn nearest_open(&self, (i, j): (usize, usize), reach: usize) -> Option<(usize, usize)> {
        for r in 0..=reach as i32 {
            for dj in -r..=r {
                for di in -r..=r {
                    if di.abs() != r && dj.abs() != r {
                        continue;
                    }
                    let (a, b) = (i as i32 + di, j as i32 + dj);
                    if a >= 0 && b >= 0 && (a as usize) < self.nx && (b as usize) < self.nz && self.open(a as usize, b as usize) {
                        return Some((a as usize, b as usize));
                    }
                }
            }
        }
        None
    }

    /// True if every cell along the straight line from a to b is walkable.
    fn clear_line(&self, a: (usize, usize), b: (usize, usize)) -> bool {
        let (ax, az) = (a.0 as f32 + 0.5, a.1 as f32 + 0.5);
        let (bx, bz) = (b.0 as f32 + 0.5, b.1 as f32 + 0.5);
        let steps = ((bx - ax).abs().max((bz - az).abs()) * 2.0).ceil().max(1.0) as usize;
        (0..=steps).all(|k| {
            let t = k as f32 / steps as f32;
            let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
            // The cells either side of the line too, so it does not graze corners.
            [(-0.3, -0.3), (0.3, -0.3), (-0.3, 0.3), (0.3, 0.3)].iter().all(|(dx, dz)| {
                let (i, j) = (x + dx, z + dz);
                i >= 0.0 && j >= 0.0 && (i as usize) < self.nx && (j as usize) < self.nz && self.open(i as usize, j as usize)
            })
        })
    }

    /// Waypoints on the ground from `from` to `to`, the first one beyond
    /// `from` and the last one `to` itself, straightened where the way is
    /// clear. `None` if there is no way, or it is too far to find.
    pub fn path(&self, map: &Map, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.nearest_open(self.cell(from)?, 4)?;
        let goal = self.nearest_open(self.cell(to)?, 6)?;
        let index = |(i, j): (usize, usize)| j * self.nx + i;
        let heuristic = |(i, j): (usize, usize)| {
            let (dx, dz) = ((i as f32 - goal.0 as f32).abs(), (j as f32 - goal.1 as f32).abs());
            dx.max(dz) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dz)
        };
        let mut cost = vec![f32::INFINITY; self.walkable.len()];
        let mut came_from = vec![u32::MAX; self.walkable.len()];
        let mut queue = BinaryHeap::new();
        cost[index(start)] = 0.0;
        queue.push(Reverse((heuristic(start).to_bits(), index(start) as u32)));
        let mut searched = 0;
        let mut found = false;
        while let Some(Reverse((_, at))) = queue.pop() {
            let at = at as usize;
            let here = (at % self.nx, at / self.nx);
            if here == goal {
                found = true;
                break;
            }
            searched += 1;
            if searched > SEARCH_LIMIT {
                return None;
            }
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (a, b) = (here.0 as i32 + di, here.1 as i32 + dj);
                if a < 0 || b < 0 || a as usize >= self.nx || b as usize >= self.nz {
                    continue;
                }
                let next = (a as usize, b as usize);
                // No cutting corners: both cells beside a diagonal step must be open.
                if !self.open(next.0, next.1)
                    || (di != 0 && dj != 0 && (!self.open(next.0, here.1) || !self.open(here.0, next.1)))
                {
                    continue;
                }
                let step = if di != 0 && dj != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                let through = cost[at] + step;
                if through < cost[index(next)] {
                    cost[index(next)] = through;
                    came_from[index(next)] = at as u32;
                    queue.push(Reverse(((through + heuristic(next)).to_bits(), index(next) as u32)));
                }
            }
        }
        if !found {
            return None;
        }
        let mut cells = vec![goal];
        let mut at = index(goal);
        while came_from[at] != u32::MAX {
            at = came_from[at] as usize;
            cells.push((at % self.nx, at / self.nx));
        }
        cells.reverse();
        // Straighten: from each kept cell, jump to the farthest one in plain
        // view that a whole player can run to in a straight line (the cells
        // are walkable with a narrower box, so a line through them can still
        // graze a door frame).
        let body = |cell: (usize, usize)| self.center(map, cell.0, cell.1) + Vec3::Y * (PLAYER_HALF.y + 0.02);
        let mut kept = vec![cells[0]];
        let mut k = 0;
        while k < cells.len() - 1 {
            let in_view: Vec<usize> = (k + 2..cells.len()).filter(|m| self.clear_line(cells[k], cells[*m])).collect();
            let far = in_view
                .iter()
                .rev()
                .find(|m| clear_run(map, body(cells[k]), body(cells[**m])))
                .copied()
                .unwrap_or(k + 1);
            kept.push(cells[far]);
            k = far;
        }
        let mut points: Vec<Vec3> = kept.iter().skip(1).map(|&(i, j)| roomy(map, self.center(map, i, j))).collect();
        if let Some(last) = points.last_mut() {
            *last = to;
        } else {
            points.push(to);
        }
        Some(points)
    }
}

/// A waypoint at `feet` moved a little, if need be, to where a whole player
/// fits: cells are walkable with a narrower box, so in a doorway the middle
/// of a cell can be too near the frame for a runner heading there.
fn roomy(map: &Map, feet: Vec3) -> Vec3 {
    let fits = |at: Vec3| {
        let foot = rest(map, at.x, at.z, feet.y);
        map.is_free(Vec3::new(at.x, foot + PLAYER_HALF.y + 0.02, at.z), PLAYER_HALF)
    };
    if fits(feet) {
        return feet;
    }
    for reach in [0.1, 0.2, 0.3] {
        for k in 0..8 {
            let step = Vec2::from_angle(k as f32 * std::f32::consts::FRAC_PI_4) * reach;
            let at = feet + Vec3::new(step.x, 0.0, step.y);
            if fits(at) {
                return at.with_y(map.ground(at.x, at.z).unwrap_or(feet.y));
            }
        }
    }
    feet
}

/// The height the feet of somebody walking to (x, z) with their feet at
/// `foot` come to rest at: the top of whatever is under them there, no
/// more than a step up (a floor, a stair), or the ground.
fn rest(map: &Map, x: f32, z: f32, foot: f32) -> f32 {
    let from = Vec3::new(x, foot + STEP_UP + 0.01, z);
    from.y - map.raycast(from, Vec3::NEG_Y, 50.0).unwrap_or(50.0)
}

/// Drops the waypoints at the front of `path` that a runner whose box is
/// centred at `pos` is done with: the next one once within a metre of it,
/// if the one after it can be run at in a straight line from here, or once
/// right on top of it. So a runner rounds a corner without cutting it into
/// the wall, or the door frame, at its inside.
pub fn pass_waypoints(map: &Map, path: &mut Vec<Vec3>, pos: Vec3) {
    while let Some(next) = path.first() {
        let near = (*next - pos).with_y(0.0).length();
        let cut = near < 1.0 && path.get(1).is_none_or(|after| clear_run(map, pos, *after));
        if near < 0.35 || cut {
            path.remove(0);
        } else {
            break;
        }
    }
}

/// True if a player whose box is centred at `from` can run straight to `to`
/// (seen from above) without bumping into anything they cannot step onto.
fn clear_run(map: &Map, from: Vec3, to: Vec3) -> bool {
    let flat = (to - from).with_y(0.0);
    let steps = (flat.length() / 0.25).ceil().max(1.0) as usize;
    let mut foot = from.y - PLAYER_HALF.y;
    (1..=steps).all(|k| {
        let p = from + flat * (k as f32 / steps as f32);
        foot = rest(map, p.x, p.z, foot);
        map.is_free(Vec3::new(p.x, foot + PLAYER_HALF.y + 0.02, p.z), PLAYER_HALF)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::valley::valley;

    /// Follows a path like a bot does, and returns where it ends up.
    fn follow(map: &Map, from: Vec3, path: &[Vec3]) -> Vec3 {
        let dt = 1.0 / 60.0;
        let mut pos = from + Vec3::Y * PLAYER_HALF.y;
        let (mut vel, mut grounded) = (Vec3::ZERO, true);
        for point in path {
            for _ in 0..2000 {
                let to = (*point - pos).with_y(0.0);
                if to.length() < 0.6 {
                    break;
                }
                let run = to.normalize() * 7.0;
                vel = Vec3::new(run.x, vel.y - 22.0 * dt, run.z);
                let was = grounded;
                grounded = map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, dt);
                if was && !grounded && vel.y <= 0.0 && map.snap_to_ground(&mut pos, PLAYER_HALF, STEP_UP) {
                    grounded = true;
                    vel.y = 0.0;
                }
            }
        }
        pos
    }

    #[test]
    fn bots_find_their_way_between_the_valleys_flags_and_bases() {
        let map = valley();
        let started = std::time::Instant::now();
        let nav = NavGrid::new(&map);
        println!("nav grid {} x {} built in {:?}", nav.nx, nav.nz, started.elapsed());
        let mut places: Vec<Vec3> = map.flags.iter().map(|flag| flag.spawns[0].pos - Vec3::Y * PLAYER_HALF.y).collect();
        places.extend(map.bases.iter().map(|base| base[0].pos - Vec3::Y * PLAYER_HALF.y));
        for (a, from) in places.iter().enumerate() {
            for to in places.iter().skip(a + 1) {
                let started = std::time::Instant::now();
                let path = nav.path(&map, *from, *to).unwrap_or_else(|| panic!("no way from {from} to {to}"));
                let found = started.elapsed();
                let end = follow(&map, *from, &path);
                println!("{from} -> {to}: {} waypoints, found in {found:?}", path.len());
                assert!((end - *to).with_y(0.0).length() < 1.5, "walking from {from} to {to} ended at {end}");
            }
        }
    }

    #[test]
    fn the_way_into_a_house_is_through_its_door() {
        let mut map = Map::empty(0);
        map.terrain = Some(crate::terrain::Terrain::generate(41, 2.0, |_, _| 0.0));
        map.bounds = [Vec2::splat(-20.0), Vec2::splat(20.0)];
        map.add_prop(crate::props::HOUSE, Vec3::ZERO, 0.0, 1.0);
        let map = map.finish();
        let nav = NavGrid::new(&map);
        // From behind the house to its front room: through a door.
        let (from, to) = (Vec3::new(-2.0, 0.0, -8.0), Vec3::new(-2.0, 0.0, 0.5));
        let path = nav.path(&map, from, to).expect("a way in");
        let end = follow(&map, from, &path);
        assert!((end - to).with_y(0.0).length() < 1.0, "ended at {end} by {path:?}");
    }
}
