//! The racing modes, run on a course with no guns and no bots.
//!
//! Race: everybody sets off together at "GO", and the first through the
//! finish wins. Time trial: each on their own clock, which starts as they
//! leave the start; as many runs as they like, and the best time wins.

pub mod rules;

/// A race goes on for at most this many seconds after the first finish.
pub const RACE_FINISH_GRACE: f32 = 30.0;
/// Once everybody is through the finish, the race ends this many seconds later.
pub const RACE_END_DELAY: f32 = 4.0;
