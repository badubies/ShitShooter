//! Sniper: one player sits on a platform with a sniper rifle while everybody
//! else, unarmed and on foot, tries to get from far away to underneath it,
//! from cover to cover. Shot, a runner is out until the next turn. Players
//! take turns on the platform until everybody has been up there `LAPS`
//! times; whoever shot the most runners wins, and how often each player
//! made it across is counted too.
//!
//! It is played on a map made for it (`custom.rs`): spawns for the runners,
//! a spawn for the sniper, and a finish under the platform.
//!
//! The numbers to play with are here and in `movement.rs` (`RUNNER`, `SNIPER`).

#[cfg(feature = "client")]
pub mod client;
pub mod rules;

/// What a player is this turn (`PlayerInfo::role`).
pub const ROLE_SNIPER: u8 = 1;
pub const ROLE_RUNNER: u8 = 2;

/// How many times everybody is the sniper.
pub const LAPS: usize = 2;
/// Seconds the runners have to get across.
pub const TURN_TIME: f32 = 150.0;
/// Seconds between two turns, while the last one's result shows.
pub const INTERMISSION: f32 = 6.0;
/// A runner's health. The rifle takes 90 off with a hit to the body, so
/// at 90 any hit is the end of the run; above it, a body shot only hurts.
pub const RUNNER_HEALTH: i32 = 90;
