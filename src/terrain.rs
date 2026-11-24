//! Rolling ground for the outdoor maps: a square grid of heights, each cell
//! split into two triangles along the same diagonal as the drawn mesh, so
//! what you stand on is exactly what you see. Pure data and math, like
//! `map.rs`, so the server uses it too.

use bevy::math::{Vec2, Vec3};

pub struct Terrain {
    /// World x and z of grid vertex (0, 0).
    pub origin: Vec2,
    /// Metres between grid vertices.
    pub cell: f32,
    /// Vertices along each side.
    pub size: usize,
    /// Row by row: vertex (i, j) is at `heights[j * size + i]`, at world
    /// x = origin.x + i * cell, z = origin.y + j * cell.
    pub heights: Vec<f32>,
    /// The highest vertex, so rays above it can be skipped.
    pub max_height: f32,
    /// The colour of the ground at each vertex (sRGB), in the same order as
    /// `heights`; empty for plain grass.
    pub colors: Vec<[f32; 3]>,
}

impl Terrain {
    /// A grid of `size` x `size` vertices centred on the origin, its heights
    /// given by `height(x, z)`.
    pub fn generate(size: usize, cell: f32, height: impl Fn(f32, f32) -> f32) -> Terrain {
        let half = (size - 1) as f32 * cell / 2.0;
        let origin = Vec2::splat(-half);
        let mut heights = Vec::with_capacity(size * size);
        for j in 0..size {
            for i in 0..size {
                heights.push(height(origin.x + i as f32 * cell, origin.y + j as f32 * cell));
            }
        }
        let mut terrain = Terrain {
            origin,
            cell,
            size,
            heights,
            max_height: 0.0,
            colors: Vec::new(),
        };
        terrain.update_max();
        terrain
    }

    /// Call after changing `heights` by hand.
    pub fn update_max(&mut self) {
        self.max_height = self.heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    }

    pub fn at(&self, i: usize, j: usize) -> f32 {
        self.heights[j * self.size + i]
    }

    /// World position of vertex (i, j).
    pub fn vertex(&self, i: usize, j: usize) -> Vec3 {
        Vec3::new(
            self.origin.x + i as f32 * self.cell,
            self.at(i, j),
            self.origin.y + j as f32 * self.cell,
        )
    }

    /// The cell a point is over, and where in it (0..1 on each side). Points
    /// off the grid are taken at its edge.
    fn locate(&self, x: f32, z: f32) -> (usize, usize, f32, f32) {
        let last = (self.size - 2) as f32;
        let gx = ((x - self.origin.x) / self.cell).clamp(0.0, last + 1.0);
        let gz = ((z - self.origin.y) / self.cell).clamp(0.0, last + 1.0);
        let (fx, fz) = (gx.floor().min(last), gz.floor().min(last));
        (fx as usize, fz as usize, gx - fx, gz - fz)
    }

    /// Height of the ground at (x, z).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let (i, j, u, v) = self.locate(x, z);
        let h00 = self.at(i, j);
        let h11 = self.at(i + 1, j + 1);
        // The diagonal runs from (0, 0) to (1, 1).
        if u >= v {
            let h10 = self.at(i + 1, j);
            h00 + (h10 - h00) * u + (h11 - h10) * v
        } else {
            let h01 = self.at(i, j + 1);
            h00 + (h11 - h01) * u + (h01 - h00) * v
        }
    }

    /// The colour of the ground at (x, z): that of the nearest vertex. None
    /// for plain grass.
    pub fn color(&self, x: f32, z: f32) -> Option<[f32; 3]> {
        if self.colors.is_empty() {
            return None;
        }
        let last = (self.size - 1) as f32;
        let i = ((x - self.origin.x) / self.cell).round().clamp(0.0, last) as usize;
        let j = ((z - self.origin.y) / self.cell).round().clamp(0.0, last) as usize;
        Some(self.colors[j * self.size + i])
    }

    /// The up-facing normal of the triangle under (x, z).
    pub fn normal(&self, x: f32, z: f32) -> Vec3 {
        let (i, j, u, v) = self.locate(x, z);
        let h00 = self.at(i, j);
        let h11 = self.at(i + 1, j + 1);
        let (dx, dz) = if u >= v {
            let h10 = self.at(i + 1, j);
            (h10 - h00, h11 - h10)
        } else {
            let h01 = self.at(i, j + 1);
            (h11 - h01, h01 - h00)
        };
        Vec3::new(-dx, self.cell, -dz).normalize()
    }

    /// Distance along the ray to the ground, if it is nearer than `max`.
    /// Marches in half-cell steps, then narrows the crossing down, so a
    /// ray only grazing a ridge thinner than that can slip past it.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<f32> {
        let below = |t: f32| {
            let p = origin + dir * t;
            p.y - self.height(p.x, p.z)
        };
        if below(0.0) < 0.0 {
            return Some(0.0);
        }
        // Skip the part of the ray that is above every hill.
        let mut start = 0.0;
        if origin.y > self.max_height {
            if dir.y >= 0.0 {
                return None;
            }
            start = (origin.y - self.max_height) / -dir.y;
            if start >= max {
                return None;
            }
        }
        let end = if dir.y > 0.0 && origin.y < self.max_height {
            ((self.max_height - origin.y) / dir.y).min(max)
        } else {
            max
        };
        let step = self.cell * 0.5;
        let mut t0 = start;
        while t0 < end {
            let t1 = (t0 + step).min(end);
            if below(t1) < 0.0 {
                let (mut lo, mut hi) = (t0, t1);
                for _ in 0..12 {
                    let mid = (lo + hi) / 2.0;
                    if below(mid) < 0.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(lo);
            }
            t0 = t1;
        }
        None
    }
}

/// Smooth value noise in about -1..1, `seed` picking one of many fields.
pub fn noise(x: f32, z: f32, seed: u32) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let corner = |dx: f32, dz: f32| hash(x0 as i32 + dx as i32, z0 as i32 + dz as i32, seed);
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * sx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * sx;
    top + (bottom - top) * sz
}

/// Several octaves of `noise`, each half as strong and twice as fine.
pub fn fractal(x: f32, z: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for octave in 0..octaves {
        sum += noise(x * freq, z * freq, seed.wrapping_add(octave * 7919)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// A repeatable pseudo-random number in -1..1 for a grid point.
fn hash(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (z as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 32767.5 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slope() -> Terrain {
        // Rises 1 m for every 2 m towards +x, with a bump in the middle.
        Terrain::generate(33, 2.0, |x, z| x * 0.5 + if x.abs() < 1.0 && z.abs() < 1.0 { 3.0 } else { 0.0 })
    }

    #[test]
    fn heights_follow_the_triangles() {
        let terrain = Terrain::generate(9, 2.0, |x, _| x * 0.5);
        for (x, z) in [(0.0, 0.0), (1.3, -2.2), (-7.9, 7.9), (3.0, 3.5)] {
            assert!((terrain.height(x, z) - x * 0.5).abs() < 1e-4, "at {x}, {z}");
        }
        // Off the grid it carries on at the edge height.
        assert!((terrain.height(100.0, 0.0) - 4.0).abs() < 1e-4);
        let normal = terrain.normal(1.0, 1.0);
        assert!((normal - Vec3::new(-0.5, 1.0, 0.0).normalize()).length() < 1e-4, "{normal}");
    }

    #[test]
    fn rays_stop_on_the_ground() {
        let terrain = slope();
        // Straight down from 10 m over x = -10 (ground at -5).
        let t = terrain.raycast(Vec3::new(-10.0, 10.0, 5.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((t - 15.0).abs() < 0.01, "{t}");
        // Level, towards the rising side: the ground reaches y = 2 at x = 4.
        let t = terrain.raycast(Vec3::new(-20.0, 2.0, 6.0), Vec3::X, 100.0).unwrap();
        assert!((t - 24.0).abs() < 0.05, "{t}");
        // Away from it, nothing; and not past `max`.
        assert!(terrain.raycast(Vec3::new(0.0, 2.0, 6.0), Vec3::NEG_X, 100.0).is_none());
        assert!(terrain.raycast(Vec3::new(-20.0, 2.0, 6.0), Vec3::X, 20.0).is_none());
        // Starting underground is a hit right away.
        assert_eq!(terrain.raycast(Vec3::new(10.0, 0.0, 0.0), Vec3::X, 5.0), Some(0.0));
    }

    #[test]
    fn noise_stays_in_range_and_is_repeatable() {
        for i in 0..500 {
            let (x, z) = (i as f32 * 0.37, i as f32 * -0.61);
            let n = fractal(x, z, 4, 3);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, fractal(x, z, 4, 3));
        }
    }
}
