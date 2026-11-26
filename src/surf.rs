//! Surf: a long hall built for riding ramps. A deck at each end, and between
//! them three ramps that dip towards the middle and rise again, like a
//! half-pipe: drop off your deck onto a slope, ride it down and up the far
//! side, and with enough speed you fly off its end onto the other deck. Fall
//! off and you land in the pit, where jump pads throw you back up.

use bevy::math::{Vec2, Vec3};

use crate::{
    map::{self, Block, JumpPad, Map},
    ramp::Ramp,
};

/// The ramps' colour, and the decks'.
pub(crate) const RAMP: [f32; 3] = [0.34, 0.50, 0.62];
pub(crate) const DECK: [f32; 3] = [0.62, 0.40, 0.24];

/// Half the hall's width (x) and length (z).
const HALF_WIDE: f32 = 40.0;
const HALF_LONG: f32 = 64.0;
/// The walls are seen this high; above them an invisible wall carries on to
/// the ceiling, so nobody flies out.
const WALL: f32 = 24.0;
const CEILING: f32 = 40.0;
/// The decks: from this far along the hall to its end, this high.
const DECK_FROM: f32 = 50.0;
const DECK_TOP: f32 = 16.0;
/// Along each ramp, the height of its ridge at these points (z); it runs
/// straight between them. Each bend is a hollow, never a crest, so a ramp
/// never throws you off it before its end.
const RIDGE: [(f32, f32); 8] = [
    (46.0, 16.0),
    (34.0, 10.0),
    (20.0, 7.0),
    (8.0, 6.0),
    (-8.0, 6.0),
    (-20.0, 7.0),
    (-34.0, 10.0),
    (-46.0, 16.0),
];
/// The slopes' steepness from the level, in degrees: too steep to stand on.
const SLOPE: f32 = 60.0;

pub fn surf() -> Map {
    let mut map = Map::empty(4);
    // How far a slope reaches out for each metre it drops.
    let run = 1.0 / SLOPE.to_radians().tan();
    let mut add = |min: [f32; 3], max: [f32; 3], color: [f32; 3], visible: bool| {
        map.blocks.push(Block {
            min: Vec3::from(min),
            max: Vec3::from(max),
            color,
            visible,
        });
    };
    let (w, l) = (HALF_WIDE, HALF_LONG);
    add([-w - 2.0, -2.0, -l - 2.0], [w + 2.0, 0.0, l + 2.0], map::FLOOR, true);
    for (lo, hi, visible) in [(0.0, WALL, true), (WALL, CEILING, false)] {
        add([-w - 2.0, lo, -l - 2.0], [-w, hi, l + 2.0], map::WALL, visible);
        add([w, lo, -l - 2.0], [w + 2.0, hi, l + 2.0], map::WALL, visible);
        add([-w, lo, -l - 2.0], [w, hi, -l], map::WALL, visible);
        add([-w, lo, l], [w, hi, l + 2.0], map::WALL, visible);
    }
    add([-w - 2.0, CEILING, -l - 2.0], [w + 2.0, CEILING + 2.0, l + 2.0], map::WALL, false);
    for end in [1.0, -1.0] {
        let (z0, z1) = map::minmax(DECK_FROM * end, l * end);
        add([-w, 0.0, z0], [w, DECK_TOP, z1], DECK, true);
    }

    // One ramp down the middle, surfed on both sides, and one against each
    // side wall, surfed on its face. Each is a chain of straight pieces.
    for pair in RIDGE.windows(2) {
        let [(z0, y0), (z1, y1)] = [pair[0], pair[1]];
        let piece = |x: f32, left: f32, right: f32| {
            Ramp::new(Vec3::new(x, y0, z0), Vec3::new(x, y1, z1), 0.0, left, right, RAMP)
        };
        // Looking down the hall towards -z, the left is -x.
        map.ramps.push(piece(0.0, run, run));
        map.ramps.push(piece(-w, 0.0, run));
        map.ramps.push(piece(w, run, 0.0));
    }

    for end in [1.0, -1.0] {
        // Up from the pit onto the deck at that end.
        for x in [-20.0, 20.0] {
            map.pads.push(JumpPad::arrow(Vec3::new(x, 0.0, 42.0 * end), Vec3::new(0.0, 29.0, 7.0 * end)));
        }
        // Spawns along the back of each deck.
        for i in 0..8 {
            let x = -35.0 + 10.0 * i as f32;
            map.spawns.push(map::spawn_facing_center(Vec3::new(x, DECK_TOP, 57.0 * end)));
        }
        map.armor_spots.push(Vec3::new(0.0, DECK_TOP, 54.0 * end));
    }
    // In the middle of the pit: a hop back up onto a slope, keeping your speed.
    for x in [-20.0, 20.0] {
        map.pads.push(JumpPad::plain(Vec3::new(x, 0.0, 0.0), 16.0));
        map.armor_spots.push(Vec3::new(x, 0.0, 12.0));
        map.armor_spots.push(Vec3::new(x, 0.0, -12.0));
    }
    map.bounds = [Vec2::new(-w + 2.0, -l + 2.0), Vec2::new(w - 2.0, l - 2.0)];
    map.look.orbit = (Vec3::new(0.0, 4.0, 0.0), 80.0, 46.0);
    map.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{PLAYER_HALF, STEP_UP};

    const GRAVITY: f32 = 22.0;
    const DT: f32 = 1.0 / 128.0;

    #[test]
    fn spawns_look_down_the_hall() {
        let map = surf();
        for spawn in &map.spawns {
            assert!(map::look_dir(spawn.yaw, 0.0).z * spawn.pos.z < 0.0, "{:?}", spawn.pos);
        }
    }

    /// A body let go against a ramp's face slides down it and never stands on it.
    #[test]
    fn the_ramps_are_too_steep_to_stand_on() {
        let map = surf();
        let run = 1.0 / SLOPE.to_radians().tan();
        // Halfway down the middle ramp's left face, where the ridge is 6 m high.
        let mut pos = Vec3::new(-run * 3.0 - PLAYER_HALF.x, 3.0 + PLAYER_HALF.y, 0.0);
        let mut vel = Vec3::ZERO;
        let start = pos;
        for _ in 0..60 {
            vel.y -= GRAVITY * DT;
            assert!(!map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, DT), "stood at {pos}");
        }
        assert!(pos.y < start.y - 0.05 && pos.x < start.x, "{start} -> {pos}");
        // It slid along the face, not straight down.
        assert!(vel.x < -1.0 && vel.y < -1.0, "{vel}");
        assert!(!map.snap_to_ground(&mut pos, PLAYER_HALF, STEP_UP));
    }

    /// Thrown along a ramp's face, a body keeps all its speed along the
    /// ramp: there is no friction, and it never counts as standing.
    #[test]
    fn a_ramp_has_no_friction() {
        let map = surf();
        let run = 1.0 / SLOPE.to_radians().tan();
        // On the left wall ramp's face, in the middle where its ridge is
        // level at 6 m, feet 3 m up.
        let mut pos = Vec3::new(-HALF_WIDE + 3.0 * run + PLAYER_HALF.x + 0.01, 3.0 + PLAYER_HALF.y, 5.0);
        let mut vel = Vec3::new(0.0, 0.0, -25.0);
        let mut touched = 0;
        for _ in 0..50 {
            vel.y -= GRAVITY * DT;
            let before = vel;
            assert!(!map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, DT), "stood at {pos}");
            touched += (vel != before) as u32;
        }
        assert!(touched > 40, "touched the ramp {touched} times");
        assert!((vel.z + 25.0).abs() < 1e-3 && vel.length() > 25.0, "{vel}");
        assert!(pos.y < 3.0 + PLAYER_HALF.y - 0.1, "did not slide down: {pos}");
    }

    /// The pads in the pit at either end throw you up onto that end's deck.
    #[test]
    fn end_pads_land_on_the_deck() {
        let map = surf();
        for pad in map.pads.iter().filter(|pad| pad.direction().is_some()) {
            let mut pos = pad.center + Vec3::Y * PLAYER_HALF.y;
            let mut vel = pad.launch;
            let mut landed = false;
            for _ in 0..(4.0 / DT) as usize {
                vel.y -= GRAVITY * DT;
                if map.move_and_slide(&mut pos, &mut vel, PLAYER_HALF, DT) && vel.y <= 0.0 {
                    landed = true;
                    break;
                }
            }
            assert!(landed);
            let feet = pos.y - PLAYER_HALF.y;
            assert!((feet - DECK_TOP).abs() < 0.01 && pos.z.abs() > DECK_FROM + 1.0, "landed at {pos}");
        }
    }

    #[test]
    fn bullets_and_grenades_stop_at_the_ramps() {
        let map = surf();
        let eye = Vec3::new(-20.0, 4.0, 0.0);
        let t = map.raycast(eye, Vec3::X, 40.0).unwrap();
        assert!(t < 20.0 && t > 15.0, "{t}");
        assert!(!map.line_of_sight(eye, Vec3::new(20.0, 4.0, 0.0)));

        // Dropped onto a face, a grenade bounces off and rolls to the pit floor.
        let (mut pos, mut vel) = (Vec3::new(-2.5, 8.0, 0.0), Vec3::ZERO);
        let mut hit = false;
        for _ in 0..2000 {
            hit |= map.step_grenade(&mut pos, &mut vel, 1.0 / 60.0) > 0.0;
        }
        assert!(hit);
        assert!(pos.y < 0.5 && pos.x < -3.0, "{pos}");
    }
}
