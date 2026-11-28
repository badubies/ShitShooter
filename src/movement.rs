//! How a player moves: every number the movement code (`player.rs`) runs on,
//! in one struct, so a mode can have its players move differently. The
//! server says which of `PRESETS` a player moves with when it spawns them
//! (`Kit::moves`); the moving itself is done by each player's own game.
//!
//! Tweak `STANDARD` to change the feel of the game, or add a preset for a
//! mode that wants its own.

/// Metres and seconds throughout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveTuning {
    pub run_speed: f32,
    pub ground_accel: f32,
    pub friction: f32,
    pub stop_speed: f32,
    /// Quake-style air strafing: in the air you can only add up to this much
    /// speed in the wished direction per step, which is what makes
    /// strafe-jumping gain speed. How fast it is added is the air
    /// acceleration, times running speed.
    pub air_wish_speed: f32,
    /// Extra air control (m/s²) that only applies below running speed.
    pub air_control_accel: f32,
    /// The air acceleration, or `None` for the one the host picked in the
    /// lobby (see `AIR_ACCEL_CHOICES`).
    pub air_accel: Option<f32>,
    pub gravity: f32,
    /// 0: no jumping at all.
    pub jump_speed: f32,
    /// Jumps off nothing, once in the air.
    pub air_jumps: u32,
    /// Bunny hopping takes a jump pressed close to landing, not jump held
    /// down. Pressed up to this long before touching down (with no air jump
    /// left, or the ground right below), the jump goes off on landing.
    pub bhop_early: f32,
    /// For this long after a landing, speed above running speed is kept
    /// rather than lost to friction, so a jump pressed just after landing
    /// still carries it.
    pub bhop_late: f32,
    /// A dash brings your speed up to this (it never slows you). 0: no dash.
    pub dash_speed: f32,
    /// A dash from the ground hops you up at this speed.
    pub dash_lift: f32,
    pub dash_cooldown: f32,
    pub max_speed: f32,
    pub crouch_speed: f32,
    /// Walking speed while aiming down sights.
    pub aim_speed: f32,
    /// Crouching on the ground at this speed or more starts a slide.
    /// Infinite: no sliding.
    pub slide_min_speed: f32,
    /// A slide tops your speed up to this, if `slide_boost_cooldown` has passed.
    pub slide_boost_speed: f32,
    pub slide_boost_cooldown: f32,
    /// Speed lost per second while sliding (m/s²). Running friction is far higher.
    pub slide_decel: f32,
    /// Below this speed a slide turns into an ordinary crouch.
    pub slide_end_speed: f32,
}

/// The game as it is meant to be played: bunny hops, a double jump, a dash
/// and slides.
pub const STANDARD: MoveTuning = MoveTuning {
    run_speed: 9.5,
    ground_accel: 10.0,
    friction: 6.0,
    stop_speed: 3.0,
    air_wish_speed: 1.0,
    air_control_accel: 25.0,
    air_accel: None,
    gravity: 22.0,
    jump_speed: 8.0,
    air_jumps: 1,
    bhop_early: 0.1,
    bhop_late: 0.1,
    dash_speed: 13.0,
    // Half the jump's speed, a quarter of its height.
    dash_lift: 4.0,
    dash_cooldown: 1.5,
    max_speed: 45.0,
    crouch_speed: 4.5,
    aim_speed: 6.0,
    slide_min_speed: 7.0,
    slide_boost_speed: 15.0,
    slide_boost_cooldown: 2.0,
    slide_decel: 6.0,
    slide_end_speed: 5.0,
};

/// Running for the sniper's platform: on foot and no more. A run two thirds
/// as fast as the game's, a crouch to hide behind things, a small jump; no
/// dash, no second jump, no slide, and nothing to gain from hopping.
pub const RUNNER: MoveTuning = MoveTuning {
    run_speed: 6.5,
    air_accel: Some(10.0),
    air_control_accel: 12.0,
    jump_speed: 6.5,
    air_jumps: 0,
    bhop_early: 0.0,
    bhop_late: 0.0,
    dash_speed: 0.0,
    max_speed: 10.0,
    crouch_speed: 3.5,
    slide_min_speed: f32::INFINITY,
    ..STANDARD
};

/// Up on the platform with the rifle: a walk along it, feet on the ground.
pub const SNIPER: MoveTuning = MoveTuning {
    run_speed: 4.0,
    aim_speed: 2.5,
    crouch_speed: 2.5,
    jump_speed: 0.0,
    air_jumps: 0,
    bhop_early: 0.0,
    bhop_late: 0.0,
    dash_speed: 0.0,
    max_speed: 6.0,
    slide_min_speed: f32::INFINITY,
    ..STANDARD
};

/// Ids of `PRESETS`, as `Kit::moves` names them.
pub const MOVES_STANDARD: u8 = 0;
pub const MOVES_RUNNER: u8 = 1;
pub const MOVES_SNIPER: u8 = 2;

pub static PRESETS: [MoveTuning; 3] = [STANDARD, RUNNER, SNIPER];

/// Preset `id`, or the standard one for a number that is none.
pub fn preset(id: u8) -> &'static MoveTuning {
    PRESETS.get(id as usize).unwrap_or(&PRESETS[MOVES_STANDARD as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The feel of the game is these numbers: changing one is a decision,
    /// not a slip.
    #[test]
    fn the_standard_tuning_is_the_game_as_it_was() {
        let t = STANDARD;
        assert_eq!((t.run_speed, t.ground_accel, t.friction, t.stop_speed), (9.5, 10.0, 6.0, 3.0));
        assert_eq!((t.air_wish_speed, t.air_control_accel, t.air_accel), (1.0, 25.0, None));
        assert_eq!((t.gravity, t.jump_speed, t.air_jumps), (22.0, 8.0, 1));
        assert_eq!((t.bhop_early, t.bhop_late), (0.1, 0.1));
        assert_eq!((t.dash_speed, t.dash_lift, t.dash_cooldown), (13.0, t.jump_speed * 0.5, 1.5));
        assert_eq!((t.max_speed, t.crouch_speed, t.aim_speed), (45.0, 4.5, 6.0));
        assert_eq!((t.slide_min_speed, t.slide_boost_speed, t.slide_boost_cooldown), (7.0, 15.0, 2.0));
        assert_eq!((t.slide_decel, t.slide_end_speed), (6.0, 5.0));
        assert_eq!(*preset(MOVES_STANDARD), STANDARD);
        assert_eq!((*preset(MOVES_RUNNER), *preset(MOVES_SNIPER)), (RUNNER, SNIPER));
        assert_eq!(*preset(200), STANDARD);
    }
}
