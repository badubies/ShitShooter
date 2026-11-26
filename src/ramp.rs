//! Ramps: long wedges with sloped sides, the one map shape that is not an
//! upright box. A slope gentle enough to walk up is stood on like the
//! ground; a steeper one has no grip, so you slide on it, and air strafing
//! against it is surfing. Pure math, for the server as much as the client.

use bevy::math::Vec3;

/// A face whose normal points up at least this much (slopes up to about 45°)
/// is ground: you stand on it. Steeper faces are surfed.
pub const WALKABLE: f32 = 0.7;
/// Overlaps no deeper than this are only touching.
const EPS: f32 = 1e-4;

/// A ridge along the top, and a slope down from it on either side to a
/// level base: a prism with a triangle for its section, cut level at the
/// bottom if the ridge is not.
pub struct Ramp {
    /// The section at the start of the ridge (ridge, left foot, right foot),
    /// then the same at its end.
    pub corners: [Vec3; 6],
    /// Each face as indices into `corners`, going round its edge.
    pub faces: [&'static [usize]; 5],
    /// The faces' outward normals, and how far along it each face lies.
    planes: [(Vec3, f32); 5],
    /// The directions of its edges.
    edges: [Vec3; 6],
    pub min: Vec3,
    pub max: Vec3,
    pub color: [f32; 3],
}

const FACES: [&[usize]; 5] = [
    // Left slope, right slope, base, start, end.
    &[0, 3, 4, 1],
    &[0, 2, 5, 3],
    &[1, 4, 5, 2],
    &[0, 1, 2],
    &[3, 5, 4],
];

impl Ramp {
    /// A ramp whose ridge runs from `from` to `to`, its sides sloping down
    /// to a level base at height `base`. Each side reaches out this many
    /// metres for every metre it drops: `left` on the left of the ridge
    /// (looking from `from` towards `to`) and `right` on the right. A slope
    /// of angle `a` reaches out `1 / tan(a)`; 0 is upright, for a ramp
    /// standing against a wall. Both ends of the ridge must be above `base`.
    pub fn new(from: Vec3, to: Vec3, base: f32, left: f32, right: f32, color: [f32; 3]) -> Ramp {
        let along = (to - from).with_y(0.0).normalize();
        let side = Vec3::new(-along.z, 0.0, along.x);
        let section = |top: Vec3| {
            let drop = top.y - base;
            [top, top.with_y(base) - side * left * drop, top.with_y(base) + side * right * drop]
        };
        let [a0, a1, a2] = section(from);
        let [b0, b1, b2] = section(to);
        let corners = [a0, a1, a2, b0, b1, b2];
        let middle = corners.iter().copied().sum::<Vec3>() / 6.0;
        let planes = FACES.map(|face| {
            let [p, q, r] = [corners[face[0]], corners[face[1]], corners[face[2]]];
            let mut normal = (q - p).cross(r - p).normalize();
            if normal.dot(p - middle) < 0.0 {
                normal = -normal;
            }
            (normal, normal.dot(p))
        });
        let edges = [to - from, a1 - a0, a2 - a0, a2 - a1, b1 - a1, b2 - a2].map(Vec3::normalize);
        let (min, max) = corners
            .iter()
            .fold((Vec3::INFINITY, Vec3::NEG_INFINITY), |(min, max), c| (min.min(*c), max.max(*c)));
        Ramp { corners, faces: FACES, planes, edges, min, max, color }
    }

    /// The outward normal of face `index` (in `faces`).
    pub fn normal(&self, index: usize) -> Vec3 {
        self.planes[index].0
    }

    fn near(&self, min: Vec3, max: Vec3) -> bool {
        min.cmplt(self.max).all() && max.cmpgt(self.min).all()
    }

    /// If a box with half extents `half` at `center` overlaps the ramp: the
    /// shortest way out of it, as the direction to push the box and how far.
    /// (The separating axis test: a box and a convex prism are apart if and
    /// only if their shadows on one of these axes are.)
    pub fn push_out(&self, center: Vec3, half: Vec3) -> Option<(Vec3, f32)> {
        if !self.near(center - half, center + half) {
            return None;
        }
        let mut best: Option<(Vec3, f32)> = None;
        let mut test = |axis: Vec3| -> bool {
            let length = axis.length();
            if length < 1e-3 {
                return true;
            }
            let axis = axis / length;
            let middle = center.dot(axis);
            let reach = axis.abs().dot(half);
            let (low, high) = self
                .corners
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), c| (lo.min(c.dot(axis)), hi.max(c.dot(axis))));
            // How far to push the box along +axis, or along -axis, to clear it.
            let up = high - (middle - reach);
            let down = (middle + reach) - low;
            if up <= EPS || down <= EPS {
                return false;
            }
            let (dir, depth) = if up <= down { (axis, up) } else { (-axis, down) };
            if best.is_none_or(|(_, d)| depth < d) {
                best = Some((dir, depth));
            }
            true
        };
        for (normal, _) in self.planes {
            if !test(normal) {
                return None;
            }
        }
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            if !test(axis) {
                return None;
            }
            for edge in self.edges {
                if !test(axis.cross(edge)) {
                    return None;
                }
            }
        }
        best
    }

    /// Distance along the ray to the ramp, if it is nearer than `max` (0 if
    /// it starts inside).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let (mut enter, mut leave) = (0.0_f32, max);
        for (normal, offset) in self.planes {
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
}
