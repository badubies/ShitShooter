//! Gauntlet: a surf course high over a pit, for the racing modes. From
//! the start deck you drop into a chute (a V of two steep walls, surfed
//! like a bobsled run: its bends throw you up their outside wall), then
//! ride ridges (a steep face down either side), with gaps to fly between
//! the ramps. A kicker at the end of one ridge throws you over the widest
//! gap onto a long one, and two short ridges after it are hopped one onto
//! the next down to the finish deck. Checkpoints stand in the gaps.
//!
//! Every ramp is a chain of straight pieces, each a prism with the same
//! section as the one before it, so their faces meet without a step.
//! Curves are level and bend in the flat; hills and hollows are straight
//! in the flat. Where a face folds towards the rider (a hollow, or the
//! outside wall of a bend) each piece reaches back a little into the one
//! before it, so a body riding the face never meets the end of the next
//! piece, only its face (see `OVERLAP`).

use std::f32::consts::PI;

use bevy::math::{Vec2, Vec3};

use crate::{
    course::{Course, Respawn, Zone},
    map::{self, Block, Look, Map, PLAYER_HALF, Spawn},
    ramp::Ramp,
};

/// The chutes' colour, the ridges', the decks' and the pit's.
pub(crate) const CHUTE: [f32; 3] = [0.30, 0.52, 0.68];
pub(crate) const RIDGE: [f32; 3] = [0.70, 0.40, 0.28];
pub(crate) const DECK: [f32; 3] = [0.56, 0.56, 0.60];
pub(crate) const PIT: [f32; 3] = [0.12, 0.14, 0.20];
pub(crate) const PILLAR: [f32; 3] = [0.40, 0.42, 0.46];

/// Pillars hold the ramps up from the pit, about this far apart along them,
/// this thick.
const PILLAR_SPACING: f32 = 36.0;
const PILLAR_HALF: f32 = 1.2;

/// The faces are this steep, too steep to stand on.
const SLOPE: f32 = 60.0;
/// The faces are this tall, from a chute's bottom to its walls' tops, or
/// from a ridge's top to its foot. The ridges' are taller, so that the one
/// the last chute opens onto reaches under all of the chute's wall.
const CHUTE_FACE: f32 = 16.0;
const RIDGE_FACE: f32 = 20.0;
/// How far a piece reaches back into the one before it, where its face
/// folds towards the rider.
const OVERLAP: f32 = 0.5;
/// A bend is made of pieces turning at most this much (degrees) each.
const BEND_STEP: f32 = 5.0;
/// After a fall you come back in at the last checkpoint going this fast
/// down the ramp after it.
const RESPAWN_SPEED: f32 = 16.0;
/// The start deck: this high, this long, as wide as the chute it opens onto.
const START_TOP: f32 = 150.0;
const START_LONG: f32 = 24.0;

/// What a chain of ramps is.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    /// Two walls facing each other, their faces meeting along the line.
    Chute,
    /// One wedge, its two faces sloping down from its top along the line.
    Ridge,
}

/// A point along the middle of the ramps, for following them (the bottom
/// of a chute's V, the top of a ridge): where it is, the way it goes, and
/// whether a ramp starts or ends there.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub struct Mark {
    pub at: Vec3,
    pub dir: Vec2,
    pub kind: Kind,
    /// Which chain of ramps it is on (they count up along the course).
    pub chain: usize,
    /// Turning this way (radians per metre, to the left), on a bend.
    pub turn: f32,
}

/// How far a face reaches out for each metre it drops.
fn run() -> f32 {
    1.0 / SLOPE.to_radians().tan()
}

/// The way that is left, looking along `dir` (seen from above, x and z).
fn left(dir: Vec2) -> Vec2 {
    Vec2::new(dir.y, -dir.x)
}

/// `dir` turned left by `angle` radians.
fn turned(dir: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(dir.x * cos + dir.y * sin, -dir.x * sin + dir.y * cos)
}

fn flat(v: Vec2) -> Vec3 {
    Vec3::new(v.x, 0.0, v.y)
}

/// The yaw that looks along `dir`.
fn yaw_along(dir: Vec2) -> f32 {
    (-dir.x).atan2(-dir.y)
}

/// The corners of a prism's two ends (each a triangle, in the same order),
/// as its faces: the three sides, then the two ends.
const PRISM: [&[usize]; 5] = [&[0, 1, 4, 3], &[1, 2, 5, 4], &[2, 0, 3, 5], &[0, 1, 2], &[3, 4, 5]];

impl Kind {
    /// How tall its faces are.
    pub fn face(self) -> f32 {
        match self {
            Kind::Chute => CHUTE_FACE,
            Kind::Ridge => RIDGE_FACE,
        }
    }

    /// The solids a piece of this kind is made of, each by its section at
    /// a point of the line, `left` being the way left there: the top of the
    /// face first, then the corner where the face ends, then the third.
    /// The first face of `PRISM` is the one ridden.
    fn sections(self, at: Vec3, left: Vec2) -> Vec<[Vec3; 3]> {
        let reach = flat(left) * self.face() * run();
        let up = Vec3::Y * self.face();
        match self {
            Kind::Chute => [1.0, -1.0]
                .map(|side| [at + reach * side + up, at, at + reach * side])
                .to_vec(),
            Kind::Ridge => vec![[at, at + reach - up, at - reach - up]],
        }
    }
}

/// Lays the course out piece by piece, following its line.
struct Builder {
    map: Map,
    marks: Vec<Mark>,
    gates: Vec<(Zone, Respawn)>,
    /// The lowest a ramp reaches in each part of the course so far: one
    /// part from the start, and one after each checkpoint.
    lows: Vec<f32>,
    kind: Kind,
    color: [f32; 3],
    chain: usize,
    at: Vec3,
    dir: Vec2,
    /// How much the line rises for each metre along, at its end.
    grade: f32,
    /// The ridden faces of the last piece laid, outward, one per solid.
    last: Option<Vec<Vec3>>,
}

impl Builder {
    fn new(kind: Kind, at: Vec3, dir: Vec2) -> Builder {
        let mut builder = Builder {
            map: Map::empty(map::GAUNTLET),
            marks: Vec::new(),
            gates: Vec::new(),
            lows: vec![f32::INFINITY],
            kind,
            color: CHUTE,
            chain: 0,
            at,
            dir: dir.normalize(),
            grade: 0.0,
            last: None,
        };
        builder.begin(kind, at, dir);
        builder
    }

    /// Starts a new chain of ramps at `at`, going along `dir`.
    fn begin(&mut self, kind: Kind, at: Vec3, dir: Vec2) {
        self.kind = kind;
        self.color = if kind == Kind::Chute { CHUTE } else { RIDGE };
        self.at = at;
        self.dir = dir.normalize();
        self.grade = 0.0;
        self.last = None;
        self.mark(0.0);
    }

    fn mark(&mut self, turn: f32) {
        self.marks.push(Mark {
            at: self.at,
            dir: self.dir,
            kind: self.kind,
            chain: self.chain,
            turn,
        });
    }

    /// One straight piece from `from` (its section turned to `from_left`)
    /// to `to` (turned to `to_left`).
    fn piece(&mut self, from: Vec3, from_left: Vec2, to: Vec3, to_left: Vec2) {
        let along = (to - from).normalize();
        let starts = self.kind.sections(from, from_left);
        let ends = self.kind.sections(to, to_left);
        let mut faces = Vec::new();
        for (index, (start, end)) in starts.into_iter().zip(ends).enumerate() {
            // Folding towards the rider: reach back into the last piece.
            let folds_in = self.last.as_ref().is_some_and(|last| last[index].dot(along) > 1e-4);
            let back = if folds_in { along * OVERLAP } else { Vec3::ZERO };
            let corners = start.iter().map(|corner| *corner - back).chain(end).collect();
            let ramp = Ramp::solid(corners, PRISM.iter().map(|face| face.to_vec()).collect(), self.color);
            faces.push(ramp.normal(0));
            let low = self.lows.last_mut().unwrap();
            *low = low.min(ramp.min.y);
            self.map.ramps.push(ramp);
        }
        self.last = Some(faces);
    }

    /// Straight on for `long` metres (seen from above), coming down `drop`
    /// (up, if it is less than 0).
    fn straight(&mut self, long: f32, drop: f32) {
        let to = self.at + flat(self.dir) * long - Vec3::Y * drop;
        let side = left(self.dir);
        self.piece(self.at, side, to, side);
        self.at = to;
        self.grade = -drop / long;
        self.mark(0.0);
    }

    /// A level bend of `degrees` (to the left; to the right if less than 0)
    /// round a circle `radius` across.
    fn bend(&mut self, degrees: f32, radius: f32) {
        let turn = degrees.to_radians();
        let pieces = (degrees.abs() / BEND_STEP).ceil() as usize;
        let step = turn / pieces as f32;
        let center = self.at + flat(left(self.dir)) * radius * turn.signum();
        let start_dir = self.dir;
        for i in 1..=pieces {
            let dir = turned(start_dir, step * i as f32);
            // From the centre out to the line, square to the way it goes.
            let out = flat(left(dir)) * -turn.signum();
            let to = center.with_y(self.at.y) + out * radius;
            self.piece(self.at, left(self.dir), to, left(dir));
            self.at = to;
            self.dir = dir;
            self.grade = 0.0;
            self.mark(turn.signum() / radius);
        }
    }

    /// Ends the chain here; the next starts `long` metres further on,
    /// `aside` metres to the left, as a `kind`, and `drop` lower than the
    /// line would have been had it gone straight on as it was: what you
    /// fall through the gap, flying off the end, is on top of that. With
    /// `gate`, a checkpoint stands in the gap, and a fall after it brings
    /// you back at the start of that next chain.
    fn gap(&mut self, long: f32, drop: f32, aside: f32, kind: Kind, gate: bool) {
        let dir = self.dir;
        let to = self.at + flat(dir) * long + flat(left(dir)) * aside + Vec3::Y * (self.grade * long - drop);
        if gate {
            let middle = (self.at + to) / 2.0;
            // Wide and tall enough for anybody flying through, high or low
            // on the faces.
            let face = self.kind.face();
            let (bottom, top) = match self.kind {
                Kind::Chute => (self.at.y - 10.0, self.at.y + face + 10.0),
                Kind::Ridge => (self.at.y - face - 10.0, self.at.y + 10.0),
            };
            let zone = Zone {
                center: middle.with_y((bottom + top) / 2.0),
                half: Vec3::new(face * run() + aside.abs() / 2.0 + 8.0, (top - bottom) / 2.0, 0.5),
                yaw: yaw_along(dir),
            };
            self.gates.push((zone, self.respawn(kind, to, dir)));
            self.lows.push(f32::INFINITY);
        }
        self.chain += 1;
        self.begin(kind, to, dir);
    }

    /// Where to come back in at the start of a chain of `kind` at `at`,
    /// going along `dir`: in a chute, just over its bottom; on a ridge, on
    /// its left face, a little way down from the top. Moving along it.
    fn respawn(&self, kind: Kind, at: Vec3, dir: Vec2) -> Respawn {
        let ahead = at + flat(dir) * 3.0;
        let pos = match kind {
            Kind::Chute => ahead + Vec3::Y * (PLAYER_HALF.y + 1.6),
            Kind::Ridge => {
                let side = flat(left(dir));
                let down = 3.0;
                let on_face = ahead + side * down * run() - Vec3::Y * down;
                // Out from the face as far as the box reaches towards it.
                let normal = (side * SLOPE.to_radians().sin() + Vec3::Y * SLOPE.to_radians().cos()).normalize();
                let reach = PLAYER_HALF.x * (normal.x.abs() + normal.z.abs()) + PLAYER_HALF.y * normal.y.abs();
                on_face + normal * (reach + 0.05)
            }
        };
        Respawn {
            pos,
            yaw: yaw_along(dir),
            vel: flat(dir) * RESPAWN_SPEED,
        }
    }

    fn block(&mut self, min: Vec3, max: Vec3, color: [f32; 3]) {
        self.map.blocks.push(Block { min, max, color, visible: true });
    }
}

/// Lays out the whole course, and the marks along its line.
fn build() -> (Map, Vec<Mark>) {
    let reach = CHUTE_FACE * run();
    // The start deck, its top level with the tops of the first chute's walls.
    let mut b = Builder::new(Kind::Chute, Vec3::new(0.0, START_TOP - CHUTE_FACE, 0.0), Vec2::Y);
    b.block(Vec3::new(-reach, START_TOP - 4.0, -START_LONG), Vec3::new(reach, START_TOP, 0.0), DECK);
    // Rails down its sides and across its back.
    for x in [-reach - 0.5, reach] {
        b.block(Vec3::new(x, START_TOP, -START_LONG), Vec3::new(x + 0.5, START_TOP + 1.2, 0.0), DECK);
    }
    b.block(
        Vec3::new(-reach - 0.5, START_TOP, -START_LONG - 0.5),
        Vec3::new(reach + 0.5, START_TOP + 1.2, -START_LONG),
        DECK,
    );

    // 1. Down the first chute, round a wide bend to the left, and on down.
    b.straight(80.0, 18.0);
    b.bend(90.0, 55.0);
    b.straight(40.0, 8.0);
    // 2. A second chute that snakes right, then left.
    b.gap(8.0, 3.5, 0.0, Kind::Chute, true);
    b.bend(-75.0, 50.0);
    b.bend(75.0, 50.0);
    b.straight(30.0, 6.0);
    // 3. A third that turns right round a hairpin, which leaves you high
    // on its left wall.
    b.gap(8.0, 3.5, 0.0, Kind::Chute, true);
    b.straight(20.0, 4.0);
    b.bend(-180.0, 42.0);
    b.straight(24.0, 6.0);
    // 4. Off that wall onto the right face of a ridge, which goes on from
    // it a little lower: down it, along, and up the kicker at its end,
    // which throws you over the widest gap.
    b.gap(10.0, 3.5 - CHUTE_FACE, reach, Kind::Ridge, true);
    b.straight(40.0, 14.0);
    b.straight(12.0, 0.0);
    b.straight(14.0, -3.0);
    // 5. A long ridge to come down on, however far the kicker threw you,
    // then two short ones, each a hop down onto the next.
    b.gap(18.0, 10.0, 0.0, Kind::Ridge, true);
    b.straight(70.0, 14.0);
    b.gap(10.0, 3.5, 0.0, Kind::Ridge, false);
    b.straight(30.0, 6.0);
    b.gap(10.0, 3.5, 0.0, Kind::Ridge, false);
    b.straight(30.0, 6.0);

    // The finish deck, low enough to land on from the last ridge, with the
    // finish line across where it starts and a wall at its far end.
    let (end, dir) = (b.at, b.dir);
    let side = flat(left(dir));
    let ahead = flat(dir);
    let deck_top = end.y - RIDGE_FACE - 2.0;
    let deck_from = end + ahead * 10.0;
    let (deck_long, deck_half) = (70.0, 20.0);
    let corners = [deck_from + side * deck_half, deck_from - side * deck_half + ahead * deck_long];
    let (min, max) = (corners[0].min(corners[1]), corners[0].max(corners[1]));
    b.block(min.with_y(deck_top - 4.0), max.with_y(deck_top), DECK);
    let wall = deck_from + ahead * deck_long;
    let wall_corners = [wall + side * deck_half, wall - side * deck_half + ahead * 1.0];
    let (min, max) = (wall_corners[0].min(wall_corners[1]), wall_corners[0].max(wall_corners[1]));
    b.block(min.with_y(deck_top), max.with_y(deck_top + 8.0), DECK);
    let finish = Zone {
        center: deck_from.with_y(deck_top + 15.0),
        half: Vec3::new(deck_half, 15.0, 0.5),
        yaw: yaw_along(dir),
    };
    let finish_at = deck_from + ahead * (deck_long - 12.0);
    let finish_point = Respawn {
        pos: finish_at.with_y(deck_top + PLAYER_HALF.y + 0.05),
        yaw: yaw_along(-dir),
        vel: Vec3::ZERO,
    };
    let last_low = b.lows.last_mut().unwrap();
    *last_low = last_low.min(deck_top - 4.0);

    // Everybody starts on the deck, facing the chute.
    let mut spawns = Vec::new();
    for row in 0..4 {
        for column in 0..4 {
            let x = -6.0 + 4.0 * column as f32;
            let z = -START_LONG + 4.0 + 4.0 * row as f32;
            spawns.push(Spawn {
                pos: Vec3::new(x, START_TOP + PLAYER_HALF.y + 0.05, z),
                yaw: PI,
            });
        }
    }
    let start = Zone {
        center: Vec3::new(0.0, START_TOP + 3.0, -START_LONG / 2.0),
        half: Vec3::new(reach + 0.5, 3.5, START_LONG / 2.0),
        yaw: PI,
    };
    let start_point = Respawn {
        pos: Vec3::new(0.0, START_TOP + PLAYER_HALF.y + 0.05, -8.0),
        yaw: PI,
        vel: Vec3::ZERO,
    };

    let Builder {
        mut map, marks, gates, lows, ..
    } = b;
    // Fallen: well under the lowest ramp of the part of the course you are on.
    let floors = lows.iter().map(|low| low - 4.0).collect();
    map.spawns = spawns;
    map.course = Some(Course {
        start,
        start_point,
        gates,
        finish,
        finish_point,
        floors,
        kills: Vec::new(),
    });

    // Pillars down from under the middle of the ramps to the pit floor.
    let mut since = PILLAR_SPACING / 2.0;
    for pair in marks.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.chain != b.chain {
            since = PILLAR_SPACING / 2.0;
            continue;
        }
        since += (b.at - a.at).with_y(0.0).length();
        if since < PILLAR_SPACING {
            continue;
        }
        since = 0.0;
        let under = match b.kind {
            Kind::Chute => b.at,
            Kind::Ridge => b.at - Vec3::Y * RIDGE_FACE,
        };
        let half = Vec3::new(PILLAR_HALF, 0.0, PILLAR_HALF);
        map.blocks.push(Block {
            min: (under - half).with_y(0.0),
            max: (under + half).with_y(under.y - 0.05),
            color: PILLAR,
            visible: true,
        });
    }

    // The pit, far below, under all of it.
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for ramp in &map.ramps {
        (lo, hi) = (lo.min(ramp.min), hi.max(ramp.max));
    }
    for block in &map.blocks {
        (lo, hi) = (lo.min(block.min), hi.max(block.max));
    }
    let margin = Vec3::new(80.0, 0.0, 80.0);
    map.blocks.push(Block {
        min: (lo - margin).with_y(-2.0),
        max: (hi + margin).with_y(0.0),
        color: PIT,
        visible: true,
    });
    map.bounds = [Vec2::new(lo.x - 10.0, lo.z - 10.0), Vec2::new(hi.x + 10.0, hi.z + 10.0)];
    let middle = (lo + hi) / 2.0;
    map.look = Look {
        sky: [0.55, 0.68, 0.88],
        fog: Some((220.0, 900.0)),
        sun_from: Vec3::new(160.0, 260.0, 90.0),
        sun: 10000.0,
        sun_color: [1.0; 3],
        ambient: 480.0,
        orbit: (middle.with_y(90.0), 210.0, 150.0),
    };
    (map.finish(), marks)
}

pub fn gauntlet() -> Map {
    build().0
}

/// The marks along the course's line, in order (see `Mark`).
#[cfg(test)]
pub fn marks() -> Vec<Mark> {
    build().1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a chain's pieces meet, the end of one is the start of the
    /// next, corner for corner, but for the bit a piece reaches back.
    #[test]
    fn pieces_meet_without_a_step() {
        let (map, _) = build();
        let mut seams = 0;
        // Compare each piece's end with the start of the piece after it of
        // the same solid (the chutes' walls alternate left, right).
        for (index, ramp) in map.ramps.iter().enumerate() {
            let step = if ramp.color == CHUTE { 2 } else { 1 };
            let Some(next) = map.ramps.get(index + step) else { continue };
            if next.color != ramp.color {
                continue;
            }
            let end = &ramp.corners[3..6];
            let start = &next.corners[0..3];
            let along = (next.corners[3] - next.corners[0]).normalize();
            let same = end
                .iter()
                .zip(start)
                .all(|(a, b)| (*a - *b).length() < 1e-3 || (*a - (*b + along * OVERLAP)).length() < 1e-3);
            if same {
                seams += 1;
            }
        }
        assert!(seams > 60, "only {seams} seams matched");
    }

    #[test]
    fn every_face_ridden_is_too_steep_to_stand_on() {
        let map = gauntlet();
        for ramp in &map.ramps {
            let face = ramp.normal(0);
            assert!(face.y < crate::ramp::WALKABLE && face.y > 0.3, "{face}");
        }
    }

    #[test]
    fn checkpoints_and_finish_are_in_order_down_the_course() {
        let map = gauntlet();
        let course = map.course.as_ref().unwrap();
        assert_eq!(course.gates.len(), 4);
        assert_eq!(course.floors.len(), course.gates.len() + 1);
        let mut high = START_TOP;
        for (zone, respawn) in &course.gates {
            assert!(zone.center.y < high, "{zone:?}");
            high = zone.center.y + zone.half.y;
            assert!(map.is_free(respawn.pos, PLAYER_HALF), "comes back in inside a ramp at {}", respawn.pos);
            assert!(zone.contains(zone.center));
        }
        for (passed, floor) in course.floors.iter().enumerate() {
            let at = course.respawn(passed);
            assert!(at.pos.y > floor + 10.0, "after {passed} checkpoints: back in at {} over a floor at {floor}", at.pos);
        }
        assert!(map.is_free(course.finish_point.pos, PLAYER_HALF));
        assert!(course.start.contains(course.start_point.pos));
        for spawn in &map.spawns {
            assert!(course.start.contains(spawn.pos));
        }
    }

    #[test]
    #[ignore]
    fn print_layout() {
        let (map, marks) = build();
        for mark in &marks {
            println!("{:?} chain {} at {:.1} dir {:.2}", mark.kind, mark.chain, mark.at, mark.dir);
        }
        let course = map.course.as_ref().unwrap();
        for (zone, at) in &course.gates {
            println!("gate at {:.1}, back in at {:.1}", zone.center, at.pos);
        }
        println!("floors {:?}, bounds {:?}, {} ramps", course.floors, map.bounds, map.ramps.len());
    }
}

