//! Courses, for the racing modes: a start, checkpoints to go through in
//! order, and a finish. Each player's game times its own runs, step by
//! step of its movement, so a time is as exact as the movement itself;
//! the server only checks that what it is told comes in a sensible order.
//! Pure math, for the server as much as the client.

use bevy::math::{Quat, Vec3};
use serde::{Deserialize, Serialize};

/// A box turned about the vertical: a checkpoint's gate, the start, the finish.
#[derive(Clone, Copy, Debug)]
pub struct Zone {
    pub center: Vec3,
    /// Half its size across (x), up (y) and along the way through it (z),
    /// before it is turned.
    pub half: Vec3,
    /// Turned this much about the vertical: its z then points along
    /// `look_dir(yaw, 0)`.
    pub yaw: f32,
}

impl Zone {
    /// A point in the zone's own frame: x across, y up, z along.
    fn local(&self, point: Vec3) -> Vec3 {
        Quat::from_rotation_y(-self.yaw) * (point - self.center) * Vec3::new(-1.0, 1.0, -1.0)
    }

    pub fn contains(&self, point: Vec3) -> bool {
        self.local(point).abs().cmple(self.half).all()
    }

    /// Where the segment from `from` to `to` is inside the zone, as the
    /// fractions of the way along it that it goes in and comes out, if it
    /// is in it at all.
    fn clip(&self, from: Vec3, to: Vec3) -> Option<(f32, f32)> {
        let (a, b) = (self.local(from), self.local(to));
        let d = b - a;
        let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
        for axis in 0..3 {
            if d[axis].abs() < 1e-9 {
                if a[axis].abs() > self.half[axis] {
                    return None;
                }
                continue;
            }
            let t0 = (-self.half[axis] - a[axis]) / d[axis];
            let t1 = (self.half[axis] - a[axis]) / d[axis];
            enter = enter.max(t0.min(t1));
            leave = leave.min(t0.max(t1));
            if enter > leave {
                return None;
            }
        }
        Some((enter, leave))
    }

    /// If moving from `from` to `to` goes into the zone (from outside it):
    /// how far along the way it does, 0 to 1.
    pub fn entered(&self, from: Vec3, to: Vec3) -> Option<f32> {
        if self.contains(from) {
            return None;
        }
        self.clip(from, to).map(|(enter, _)| enter)
    }

    /// If moving from `from` (inside the zone) to `to` leaves it: how far
    /// along the way it does, 0 to 1.
    pub fn left(&self, from: Vec3, to: Vec3) -> Option<f32> {
        if !self.contains(from) || self.contains(to) {
            return None;
        }
        Some(self.clip(from, to).map_or(0.0, |(_, leave)| leave))
    }
}

/// Where somebody who fell off comes back in: their box's centre, the way
/// they face, and how fast they are going (put back in moving, so the
/// ramp ahead carries them on).
#[derive(Clone, Copy, Debug)]
pub struct Respawn {
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: Vec3,
}

pub struct Course {
    /// The start: in a time trial, leaving it starts the clock.
    pub start: Zone,
    /// Where going back to the start puts you.
    pub start_point: Respawn,
    /// The checkpoints, in the order they must be gone through, each with
    /// where a fall after it puts you back.
    pub gates: Vec<(Zone, Respawn)>,
    pub finish: Zone,
    /// Where you wait once you are through the finish of a race.
    pub finish_point: Respawn,
    /// Below these heights you have fallen off: `floors[i]` once through
    /// `i` checkpoints (so one more than there are checkpoints, the last
    /// for after the last one).
    pub floors: Vec<f32>,
    /// Go into one of these and you have fallen off too.
    pub kills: Vec<Zone>,
}

impl Course {
    /// Has a body whose box is centred at `pos`, through `passed`
    /// checkpoints, fallen off?
    pub fn fallen(&self, pos: Vec3, passed: usize) -> bool {
        self.floors.get(passed).or(self.floors.last()).is_some_and(|floor| pos.y < *floor) || self.kills.iter().any(|kill| kill.contains(pos))
    }

    /// Where to come back in, through `passed` checkpoints.
    pub fn respawn(&self, passed: usize) -> Respawn {
        match passed {
            0 => self.start_point,
            n => self.gates.get(n - 1).map_or(self.finish_point, |(_, at)| *at),
        }
    }
}

/// What happened on a run, as the runner's own game timed it. Clients tell
/// the server; the server passes on the ones it takes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum RunEvent {
    /// Time trial: out of the start, and the clock is running.
    Started,
    /// Through checkpoint `gate` (0 is the first), `time` seconds in.
    Gate { gate: u8, time: f32 },
    /// Through the finish, `time` seconds in.
    Finished { time: f32 },
    /// Time trial: back at the start, so this run is over.
    Abandoned,
}

/// One player's run along a course.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// Seconds since it started, while it is on.
    pub clock: Option<f32>,
    /// How many checkpoints it has been through.
    pub passed: usize,
    /// When it went through each of them.
    pub splits: Vec<f32>,
    /// Its time, once through the finish.
    pub finished: Option<f32>,
}

impl Run {
    /// One movement step of `dt` seconds, from `from` to `to` (the box's
    /// centre). In a time trial (`trial`) the clock starts on leaving the
    /// start and stops on coming back into it; in a race it is started for
    /// everybody at once, outside of this. Whatever happened in the step.
    pub fn step(&mut self, course: &Course, from: Vec3, to: Vec3, dt: f32, trial: bool) -> Option<RunEvent> {
        if self.finished.is_some() {
            return None;
        }
        if trial {
            if course.start.contains(to) {
                let was_on = self.clock.is_some() || self.passed > 0;
                *self = Run::default();
                return was_on.then_some(RunEvent::Abandoned);
            }
            if self.clock.is_none() {
                let out = course.start.left(from, to)?;
                self.clock = Some((1.0 - out) * dt);
                return Some(RunEvent::Started);
            }
        }
        let before = self.clock?;
        self.clock = Some(before + dt);
        let at = |along: f32| before + along * dt;
        if let Some((zone, _)) = course.gates.get(self.passed) {
            let along = zone.entered(from, to)?;
            let time = at(along);
            self.passed += 1;
            self.splits.push(time);
            return Some(RunEvent::Gate {
                gate: self.passed as u8 - 1,
                time,
            });
        }
        let time = at(course.finish.entered(from, to)?);
        self.finished = Some(time);
        self.clock = None;
        Some(RunEvent::Finished { time })
    }
}

/// A run's time as it is shown: minutes, seconds and hundredths.
pub fn format_time(seconds: f32) -> String {
    let hundredths = (seconds.max(0.0) * 100.0).round() as u32;
    format!("{}:{:02}.{:02}", hundredths / 6000, hundredths / 100 % 60, hundredths % 100)
}

/// A difference between two times, signed: "-0.31", "+1.20".
pub fn format_gap(seconds: f32) -> String {
    let sign = if seconds < 0.0 { '-' } else { '+' };
    let hundredths = (seconds.abs() * 100.0).round() as u32;
    if hundredths >= 6000 {
        return format!("{sign}{}", format_time(seconds.abs()));
    }
    format!("{sign}{}.{:02}", hundredths / 100, hundredths % 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(z: f32) -> Zone {
        Zone {
            center: Vec3::new(0.0, 5.0, z),
            half: Vec3::new(4.0, 5.0, 0.5),
            yaw: 0.0,
        }
    }

    fn course() -> Course {
        let respawn = Respawn {
            pos: Vec3::ZERO,
            yaw: 0.0,
            vel: Vec3::ZERO,
        };
        Course {
            start: Zone {
                center: Vec3::new(0.0, 5.0, -5.0),
                half: Vec3::new(4.0, 5.0, 5.0),
                yaw: 0.0,
            },
            start_point: respawn,
            gates: vec![(gate(10.0), respawn), (gate(20.0), respawn)],
            finish: gate(30.0),
            finish_point: respawn,
            floors: vec![-1.0, -2.0, -3.0],
            kills: Vec::new(),
        }
    }

    /// Runs straight along +z at `speed` from `z`, until `until`.
    fn run_along(run: &mut Run, course: &Course, z: f32, until: f32, speed: f32, trial: bool) -> Vec<RunEvent> {
        let dt = 1.0 / 128.0;
        let mut events = Vec::new();
        let mut at = Vec3::new(0.0, 2.0, z);
        while at.z < until {
            let next = at + Vec3::Z * speed * dt;
            events.extend(run.step(course, at, next, dt, trial));
            at = next;
        }
        events
    }

    #[test]
    fn a_turned_zone_is_turned_the_way_the_view_turns() {
        let zone = Zone {
            center: Vec3::ZERO,
            half: Vec3::new(4.0, 1.0, 0.5),
            yaw: std::f32::consts::FRAC_PI_2,
        };
        // Its z points along look_dir(yaw), across is the other way.
        let along = crate::map::look_dir(zone.yaw, 0.0);
        assert!(zone.contains(along * 0.4) && !zone.contains(along * 0.6));
        let across = Vec3::new(-along.z, 0.0, along.x);
        assert!(zone.contains(across * 3.9) && !zone.contains(across * 4.1));
    }

    #[test]
    fn a_trial_is_timed_from_leaving_the_start_to_the_finish() {
        let course = course();
        let mut run = Run::default();
        let events = run_along(&mut run, &course, -3.0, 40.0, 10.0, true);
        assert_eq!(events[0], RunEvent::Started);
        // 30 m at 10 m/s from the edge of the start (z = 0) to the finish's
        // near side (29.5), to within rounding.
        let RunEvent::Finished { time } = *events.last().unwrap() else { panic!("{events:?}") };
        assert!((time - 2.95).abs() < 1e-3, "{time}");
        let RunEvent::Gate { gate: 0, time } = events[1] else { panic!("{events:?}") };
        assert!((time - 0.95).abs() < 1e-3, "{time}");
        assert_eq!(events.len(), 4);
        // Nothing more once finished.
        assert!(run_along(&mut run, &course, 40.0, 45.0, 10.0, true).is_empty());
    }

    #[test]
    fn gates_count_only_in_order() {
        let course = course();
        let mut run = Run::default();
        run.clock = Some(0.0);
        // Straight past the first checkpoint's side, then through the second.
        let dt = 1.0 / 128.0;
        let from = Vec3::new(6.0, 2.0, 5.0);
        assert_eq!(run.step(&course, from, Vec3::new(6.0, 2.0, 15.0), dt, false), None);
        assert_eq!(run.step(&course, Vec3::new(0.0, 2.0, 15.0), Vec3::new(0.0, 2.0, 25.0), dt, false), None);
        assert_eq!(run.passed, 0);
    }

    #[test]
    fn going_back_into_the_start_abandons_a_trial() {
        let course = course();
        let mut run = Run::default();
        run_along(&mut run, &course, -3.0, 12.0, 10.0, true);
        assert_eq!(run.passed, 1);
        let dt = 1.0 / 128.0;
        let event = run.step(&course, Vec3::new(0.0, 2.0, 12.0), Vec3::new(0.0, 2.0, -1.0), dt, true);
        assert_eq!(event, Some(RunEvent::Abandoned));
        assert_eq!(run, Run::default());
        // Standing in it is nothing.
        assert_eq!(run.step(&course, Vec3::new(0.0, 2.0, -1.0), Vec3::new(0.0, 2.0, -1.0), dt, true), None);
    }

    #[test]
    fn a_race_runs_from_when_it_is_started() {
        let course = course();
        let mut run = Run::default();
        assert!(run_along(&mut run, &course, -3.0, 40.0, 10.0, false).is_empty());
        let mut run = Run {
            clock: Some(0.0),
            ..Run::default()
        };
        let events = run_along(&mut run, &course, -3.0, 40.0, 10.0, false);
        let RunEvent::Finished { time } = *events.last().unwrap() else { panic!("{events:?}") };
        assert!((time - 3.25).abs() < 1e-3, "{time}");
    }

    #[test]
    fn falling_is_judged_by_the_part_of_the_course_you_are_on() {
        let course = course();
        assert!(!course.fallen(Vec3::new(0.0, -0.5, 0.0), 0));
        assert!(course.fallen(Vec3::new(0.0, -1.5, 0.0), 0));
        assert!(!course.fallen(Vec3::new(0.0, -2.5, 0.0), 2));
        assert!(course.fallen(Vec3::new(0.0, -3.5, 0.0), 5));
    }

    #[test]
    fn times_read_as_minutes_seconds_and_hundredths() {
        assert_eq!(format_time(62.345), "1:02.35");
        assert_eq!(format_time(5.0), "0:05.00");
        assert_eq!(format_gap(-0.314), "-0.31");
        assert_eq!(format_gap(1.2), "+1.20");
        assert_eq!(format_gap(75.0), "+1:15.00");
    }
}
