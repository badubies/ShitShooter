//! Conquest: each team has tickets. Every death costs one, and the team
//! holding fewer of the map's flags bleeds them. Out of tickets, you lose.

#[cfg(feature = "client")]
pub mod client;
pub mod rules;

/// Tickets each team starts a conquest match with.
pub const CONQUEST_TICKETS: u32 = 200;
/// Seconds for one player to take a neutral flag (or to neutralize an
/// enemy one). More players take it faster, up to `CAPTURE_CROWD` times.
pub const CAPTURE_TIME: f32 = 10.0;
pub const CAPTURE_CROWD: i32 = 3;
/// Tickets a second the team holding fewer flags loses, for each flag the
/// other team holds more.
pub const TICKET_BLEED: f32 = 0.25;
/// A player counts as on a flag within its radius along the ground and this
/// far above or below its foot.
pub const FLAG_HEIGHT: f32 = 8.0;
