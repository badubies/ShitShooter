//! Ramps: convex solids with sloped faces, the map shapes that are not
//! upright boxes. Long wedges, roofs, rocks and flights of stairs are all
//! ramps. A face gentle enough to walk up is stood on like the ground; a
//! steeper one has no grip, so you slide on it, and air strafing against it
//! is surfing. Pure math, for the server as much as the client.

use bevy::math::Vec3;

/// A face whose normal points up at least this much (slopes up to about 45°)
/// is ground: you stand on it. Steeper faces are surfed.
pub const WALKABLE: f32 = 0.7;
/// Overlaps no deeper than this are only touching.
const EPS: f32 = 1e-4;

/// A convex solid: its corners, and its flat faces between them.
pub struct Ramp {
    pub corners: Vec<Vec3>,
    /// Each face as indices into `corners`, going round its edge.
    pub faces: Vec<Vec<usize>>,
    /// Each face's outward normal, and how far along it the face lies.
    planes: Vec<(Vec3, f32)>,
    /// The directions to test a box against for overlap: every way a face
    /// points, once.
    axes: Vec<Vec3>,
    /// The directions of its edges, each once.
    edges: Vec<Vec3>,
    pub min: Vec3,
    pub max: Vec3,
    pub color: [f32; 3],
    /// Hidden ramps still collide: the roofs, rocks and stairs of models.
    pub visible: bool,
}

/// The faces of a wedge from `Ramp::new`: left slope, right slope, base,
/// start, end.
const FACES: [&[usize]; 5] = [&[0, 3, 4, 1], &[0, 2, 5, 3], &[1, 4, 5, 2], &[0, 1, 2], &[3, 5, 4]];

/// The edges round a face, as pairs of corners.
fn round<'a>(corners: &'a [Vec3], face: &'a [usize]) -> impl Iterator<Item = (Vec3, Vec3)> + 'a {
    (0..face.len()).map(move |i| (corners[face[i]], corners[face[(i + 1) % face.len()]]))
}

/// True if `a` and `b` (both of length 1) point the same way or opposite ways.
fn parallel(a: Vec3, b: Vec3) -> bool {
    a.dot(b).abs() > 1.0 - 1e-5
}

impl Ramp {
    /// A wedge whose ridge runs from `from` to `to`, its sides sloping down
    /// to a level base at height `base`: a prism with a triangle for its
    /// section, cut level at the bottom if the ridge is not. Each side
    /// reaches out this many metres for every metre it drops: `left` on the
    /// left of the ridge (looking from `from` towards `to`) and `right` on
    /// the right. A slope of angle `a` reaches out `1 / tan(a)`; 0 is
    /// upright, for a ramp standing against a wall. Both ends of the ridge
    /// must be above `base`.
    pub fn new(from: Vec3, to: Vec3, base: f32, left: f32, right: f32, color: [f32; 3]) -> Ramp {
        let along = (to - from).with_y(0.0).normalize();
        let side = Vec3::new(-along.z, 0.0, along.x);
        let section = |top: Vec3| {
            let drop = top.y - base;
            [top, top.with_y(base) - side * left * drop, top.with_y(base) + side * right * drop]
        };
        let [a0, a1, a2] = section(from);
        let [b0, b1, b2] = section(to);
        let faces = FACES.iter().map(|face| face.to_vec()).collect();
        Ramp::solid(vec![a0, a1, a2, b0, b1, b2], faces, color)
    }

    /// Any convex solid, from its corners and its faces (each as indices
    /// into `corners`, going round its edge either way).
    pub fn solid(corners: Vec<Vec3>, faces: Vec<Vec<usize>>, color: [f32; 3]) -> Ramp {
        let middle = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
        let planes: Vec<(Vec3, f32)> = faces
            .iter()
            .map(|face| {
                // Newell's normal: sound however many corners the face has.
                let mut normal = round(&corners, face).map(|(p, q)| (p - q).cross(p + q)).sum::<Vec3>().normalize();
                let p = corners[face[0]];
                if normal.dot(p - middle) < 0.0 {
                    normal = -normal;
                }
                (normal, normal.dot(p))
            })
            .collect();
        let mut axes: Vec<Vec3> = Vec::new();
        for (normal, _) in &planes {
            if !axes.iter().any(|axis| parallel(*axis, *normal)) {
                axes.push(*normal);
            }
        }
        let mut edges: Vec<Vec3> = Vec::new();
        for (p, q) in faces.iter().flat_map(|face| round(&corners, face)) {
            let Some(edge) = (q - p).try_normalize() else { continue };
            if !edges.iter().any(|e| parallel(*e, edge)) {
                edges.push(edge);
            }
        }
        let (min, max) = corners
            .iter()
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), c| (min.min(*c), max.max(*c)));
        Ramp { corners, faces, planes, axes, edges, min, max, color, visible: true }
    }

    /// The outward normal of face `index` (in `faces`).
    pub fn normal(&self, index: usize) -> Vec3 {
        self.planes[index].0
    }

    fn near(&self, min: Vec3, max: Vec3) -> bool {
        min.cmplt(self.max).all() && max.cmpgt(self.min).all()
    }

    /// The directions along which a box and the ramp are apart if they are
    /// apart at all (the separating axis test): the ramp's faces' normals,
    /// the box's, and each of the box's edges crossed with each of the
    /// ramp's. Each of length 1.
    fn separating_axes(&self) -> impl Iterator<Item = Vec3> + '_ {
        let boxes = [Vec3::X, Vec3::Y, Vec3::Z];
        let crossed = boxes.into_iter().flat_map(|axis| self.edges.iter().filter_map(move |edge| axis.cross(*edge).try_normalize()));
        self.axes.iter().copied().chain(boxes).chain(crossed)
    }

    /// The ramp's shadow on `axis`: the lowest and highest of its corners along it.
    fn shadow(&self, axis: Vec3) -> (f32, f32) {
        self.corners.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), c| (lo.min(c.dot(axis)), hi.max(c.dot(axis))))
    }

    /// If a box with half extents `half` at `center` overlaps the ramp: the
    /// shortest way out of it, as the direction to push the box and how far.
    pub fn push_out(&self, center: Vec3, half: Vec3) -> Option<(Vec3, f32)> {
        if !self.near(center - half, center + half) {
            return None;
        }
        let mut best: Option<(Vec3, f32)> = None;
        for axis in self.separating_axes() {
            let (middle, reach) = (center.dot(axis), axis.abs().dot(half));
            let (low, high) = self.shadow(axis);
            // How far to push the box along +axis, or along -axis, to clear it.
            let up = high - (middle - reach);
            let down = (middle + reach) - low;
            if up <= EPS || down <= EPS {
                return None;
            }
            let (dir, depth) = if up <= down { (axis, up) } else { (-axis, down) };
            if best.is_none_or(|(_, d)| depth < d) {
                best = Some((dir, depth));
            }
        }
        best
    }

    /// How far a box with half extents `half` clear of the ramp at `center`
    /// moves along `dir` (of length 1), no further than `max`, before it
    /// touches the ramp, and the outward normal of the ramp where it does.
    /// (The box hits when its shadow has met the ramp's on every axis.)
    pub fn sweep(&self, center: Vec3, half: Vec3, dir: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let end = center + dir * max;
        if !self.near(center.min(end) - half, center.max(end) + half) {
            return None;
        }
        let (mut enter, mut leave, mut normal) = (f32::NEG_INFINITY, max, Vec3::ZERO);
        for axis in self.separating_axes() {
            let (middle, reach) = (center.dot(axis), axis.abs().dot(half));
            let (low, high) = self.shadow(axis);
            let speed = dir.dot(axis);
            if speed.abs() < 1e-6 {
                if middle + reach <= low + EPS || middle - reach >= high - EPS {
                    return None;
                }
                continue;
            }
            // When the box's shadow starts and stops overlapping the ramp's.
            let (meet, part, facing) = if speed > 0.0 {
                ((low - (middle + reach)) / speed, (high - (middle - reach)) / speed, -axis)
            } else {
                ((high - (middle - reach)) / speed, (low - (middle + reach)) / speed, axis)
            };
            if meet > enter {
                (enter, normal) = (meet, facing);
            }
            leave = leave.min(part);
            if enter > leave {
                return None;
            }
        }
        (enter >= -EPS).then_some((enter.max(0.0), normal))
    }

    /// Distance along the ray to the ramp, if it is nearer than `max` (0 if
    /// it starts inside).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let (mut enter, mut leave) = (0.0_f32, max);
        for &(normal, offset) in &self.planes {
            let outside = normal.dot(origin) - offset;
            let toward = normal.dot(dir);
            if toward.abs() < 1e-8 {
                if outside > 0.0 {
                    return None;
                }
                continue;
            }
            let t = -outside / toward;
            if toward < 0.0 {
                enter = enter.max(t);
            } else {
                leave = leave.min(t);
            }
            if enter > leave {
                return None;
            }
        }
        (enter < max).then_some(enter)
    }

    /// The height of the ramp's top at (x, z), if it is over the ramp.
    pub fn top(&self, x: f32, z: f32) -> Option<f32> {
        let from = Vec3::new(x, self.max.y + 1.0, z);
        self.raycast(from, Vec3::NEG_Y, self.max.y - self.min.y + 1.0).map(|t| from.y - t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ridge along -z, 6 m high, its left side (towards -x) at 60° and its
    /// right side at 45°.
    fn ramp() -> Ramp {
        let steep = 1.0 / 60f32.to_radians().tan();
        Ramp::new(Vec3::new(0.0, 6.0, 10.0), Vec3::new(0.0, 6.0, -10.0), 0.0, steep, 1.0, [0.5; 3])
    }

    #[test]
    fn its_faces_point_out() {
        let ramp = ramp();
        // Looking down -z, the left is -x.
        assert!(ramp.normal(0).x < -0.8 && ramp.normal(0).y > 0.4);
        assert!(ramp.normal(1).x > 0.6 && (ramp.normal(1).y - ramp.normal(1).x).abs() < 1e-4);
        assert_eq!(ramp.normal(2), Vec3::NEG_Y);
        assert!(ramp.normal(3).z > 0.99 && ramp.normal(4).z < -0.99);
    }

    /// A ridge that falls along its length, over a level base: the sides
    /// keep their slope, so the ramp is wider where it is higher.
    #[test]
    fn a_falling_ridge_keeps_its_slopes() {
        let ramp = Ramp::new(Vec3::new(0.0, 10.0, 10.0), Vec3::new(0.0, 4.0, -10.0), 0.0, 1.0, 1.0, [0.5; 3]);
        assert_eq!(ramp.normal(2), Vec3::NEG_Y);
        assert!(ramp.corners.iter().all(|c| c.y == 0.0 || c.x == 0.0));
        assert!(ramp.normal(0).y > 0.6 && ramp.normal(0).y < 0.75, "{}", ramp.normal(0));
        assert!((ramp.top(-4.0, 9.9).unwrap() - 6.0).abs() < 0.05);
        assert!((ramp.top(-2.0, -9.9).unwrap() - 2.0).abs() < 0.05);
        assert!(ramp.top(-5.0, -9.9).is_none());
        let half = Vec3::new(0.4, 0.9, 0.4);
        assert!(ramp.push_out(Vec3::new(-5.0, 0.9, -9.0), half).is_none());
        assert!(ramp.push_out(Vec3::new(-3.0, 0.9, -9.0), half).is_some());
    }

    #[test]
    fn a_box_on_a_slope_is_pushed_out_along_it() {
        let ramp = ramp();
        let half = Vec3::new(0.4, 0.9, 0.4);
        assert!(ramp.push_out(Vec3::new(-6.0, 4.0, 0.0), half).is_none());
        assert!(ramp.push_out(Vec3::new(0.0, 8.0, 0.0), half).is_none());
        // Its corner dug a little into the steep side.
        let (dir, depth) = ramp.push_out(Vec3::new(-2.0, 3.6, 0.0), half).unwrap();
        assert!((dir - ramp.normal(0)).length() < 1e-4, "{dir}");
        assert!(depth > 0.0 && depth < 0.5);
        // Pushed out, it only touches.
        let out = Vec3::new(-2.0, 3.6, 0.0) + dir * depth;
        assert!(ramp.push_out(out, half).is_none());
        assert!(ramp.push_out(out - dir * 0.01, half).is_some());
        // Beyond the end of the ridge it is free.
        assert!(ramp.push_out(Vec3::new(-2.0, 3.6, 10.5), half).is_none());
    }

    #[test]
    fn rays_hit_the_slopes_and_the_top() {
        let ramp = ramp();
        let t = ramp.raycast(Vec3::new(-10.0, 3.0, 0.0), Vec3::X, 20.0).unwrap();
        let steep = 6.0 / 60f32.to_radians().tan();
        assert!((t - (10.0 - steep / 2.0)).abs() < 1e-3, "{t}");
        assert!(ramp.raycast(Vec3::new(-10.0, 3.0, 0.0), Vec3::X, 5.0).is_none());
        assert!(ramp.raycast(Vec3::new(-10.0, 7.0, 0.0), Vec3::X, 20.0).is_none());
        assert_eq!(ramp.raycast(Vec3::new(0.0, 3.0, 0.0), Vec3::X, 20.0), Some(0.0));
        assert!((ramp.top(3.0, 0.0).unwrap() - 3.0).abs() < 1e-3);
        assert!(ramp.top(7.0, 0.0).is_none());
    }

    /// A box dropped onto the gentle side stops where it touches it, on
    /// that face; one dropped clear of the ramp misses it.
    #[test]
    fn a_falling_box_meets_the_face_under_it() {
        let ramp = ramp();
        let half = Vec3::new(0.4, 0.9, 0.4);
        // Over the 45° side, 3 m out from the ridge.
        let center = Vec3::new(3.0, 8.0, 0.0);
        let (fall, normal) = ramp.sweep(center, half, Vec3::NEG_Y, 10.0).unwrap();
        assert!((normal - ramp.normal(1)).length() < 1e-4, "{normal}");
        // Its corner nearest the ridge comes down on the slope, 2.6 m out.
        assert!((center.y - fall - half.y - 3.4).abs() < 1e-3, "fell {fall}");
        let landed = center - Vec3::Y * fall;
        assert!(ramp.push_out(landed, half).is_none());
        assert!(ramp.push_out(landed - Vec3::Y * 0.01, half).is_some());
        assert!(ramp.sweep(center, half, Vec3::NEG_Y, fall - 0.1).is_none());
        assert!(ramp.sweep(Vec3::new(7.0, 8.0, 0.0), half, Vec3::NEG_Y, 10.0).is_none());
    }

    /// The same wedge given as corners and faces is the same solid.
    #[test]
    fn a_solid_from_its_faces_matches_the_wedge() {
        let wedge = ramp();
        let faces = wedge.faces.iter().map(|face| face.iter().rev().copied().collect()).collect();
        let solid = Ramp::solid(wedge.corners.clone(), faces, [0.5; 3]);
        for index in 0..5 {
            assert!((solid.normal(index) - wedge.normal(index)).length() < 1e-5);
        }
        let half = Vec3::new(0.4, 0.9, 0.4);
        let center = Vec3::new(-2.0, 3.6, 0.0);
        assert_eq!(solid.push_out(center, half), wedge.push_out(center, half));
    }
}
