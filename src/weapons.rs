//! The weapons. One table shared by the client (which fires them) and the
//! server (which checks fire rates and applies the damage).

pub type WeaponId = u8;

pub struct Weapon {
    pub name: &'static str,
    /// Damage per bullet (per pellet for the shotgun, per stab for the knife).
    pub damage: i32,
    /// Seconds between shots.
    pub interval: f32,
    /// Fires for as long as the trigger is held.
    pub auto: bool,
    /// Bullets per shot.
    pub pellets: usize,
    pub magazine: u32,
    /// Seconds to reload: the whole magazine, or one round for a gun that
    /// loads `reload_per_round`.
    pub reload: f32,
    /// Loads one round at a time until it is full, or until it is fired.
    pub reload_per_round: bool,
    /// Worked by hand after every shot, which is what the wait between
    /// shots is spent on.
    pub cycle: Option<Cycle>,
    /// Bullet speed in m/s.
    pub speed: f32,
    /// How fast the bullet drops (m/s²).
    pub gravity: f32,
    /// Half angle (radians) of the cone shots land in, from the hip and aimed.
    pub hip_spread: f32,
    pub aim_spread: f32,
    /// Running speed factor while it is in your hands.
    pub mobility: f32,
    /// Size of the gun in first person: width, height, length (metres).
    pub size: [f32; 3],
    /// How far in front of the eye a sight's dot is held when aiming: close
    /// for a gun held to the shoulder, further out for a pistol held in front.
    pub sight_relief: f32,
    /// How far each shot throws the view up, and at most sideways (radians).
    pub kick_up: f32,
    pub kick_side: f32,
    /// Damage is multiplied by this when a bullet hits the head.
    pub headshot: f32,
    /// The 3D model drawn for it. Without one it is drawn as a few boxes of `size`.
    pub model: Option<Model>,
    /// Aimed, its shots close up tighter the longer it is held there.
    pub choke: Option<Choke>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cycle {
    /// A pump racked back and forth.
    Pump,
    /// A bolt lifted, drawn back, pushed home and locked down: the lock is
    /// heard just as the next shot is ready.
    Bolt,
}

/// Holding a gun aimed narrows its spread from `aim_spread` down to `spread`
/// over `time` seconds. Letting go of aim or firing opens it up again.
pub struct Choke {
    pub spread: f32,
    pub time: f32,
}

/// A weapon's glTF model, drawn where the box gun would be.
pub struct Model {
    /// Under `assets/`.
    pub path: &'static str,
    pub fit: Fit,
    /// The sights it can carry besides its iron sights, on the file's
    /// `SightMount` node (see `gun_model.rs`). Empty without a mount.
    pub sights: &'static [SightId],
}

/// How a model's file is put in the hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    /// Built for the game (see `art/pistol.py`): in metres, the weapon's
    /// `size` long, the barrel along -Z and up +Y, centred on its origin. It
    /// is placed as it is.
    AsBuilt,
    /// A model from elsewhere, in its own units and axes: it is measured,
    /// scaled to the weapon's `size` length, centred, and turned round if
    /// its barrel points along +Z.
    Scaled { barrel_plus_z: bool },
}

pub type SightId = u8;
/// No sight: the gun's own iron sights.
pub const IRON_SIGHTS: SightId = 0;
pub const RED_DOT: SightId = 1;
pub const SCOPE: SightId = 2;
pub const REFLEX: SightId = 3;
pub const TWO_X: SightId = 4;

pub struct Sight {
    pub name: &'static str,
    /// A glTF model (under `assets/`) in metres, with an empty named `AimPoint`
    /// at its dot. It goes on the gun's `SightMount` node, the gun's nodes named
    /// `IronSight...` are hidden under it, and aiming lines up the dot with the
    /// crosshair. `None` for the iron sights.
    pub path: Option<&'static str>,
    /// Field of view factor when aimed through it; smaller zooms in more. It
    /// is the same on every gun: zoom comes from the sight, not the gun.
    pub zoom: f32,
    /// Fully aimed, the view through it is a scope's: the gun is hidden and a
    /// lens with a reticle fills the screen.
    pub overlay: bool,
}

pub const SIGHTS: [Sight; 5] = [
    Sight {
        name: "Iron sights",
        path: None,
        zoom: 0.85,
        overlay: false,
    },
    Sight {
        name: "Red dot",
        path: Some("models/sights/red_dot.glb"),
        zoom: 0.75,
        overlay: false,
    },
    Sight {
        name: "Scope",
        path: Some("models/sights/scope.glb"),
        zoom: 0.25,
        overlay: true,
    },
    // A short, thin-walled housing round a big window, a dot in a ring. Made by
    // art/sights.py with the 2x, in the same style; small enough for the pistol.
    Sight {
        name: "Reflex",
        path: Some("models/sights/reflex.glb"),
        zoom: 0.75,
        overlay: false,
    },
    // The reflex's housing, a little longer, with a chevron in a ring; it
    // magnifies twice. Made by art/sights.py; small enough for the pistol.
    Sight {
        name: "2x sight",
        path: Some("models/sights/2x.glb"),
        zoom: 0.5,
        overlay: false,
    },
];

/// Unknown ids are the iron sights.
pub fn sight(id: SightId) -> &'static Sight {
    SIGHTS.get(id as usize).unwrap_or(&SIGHTS[IRON_SIGHTS as usize])
}

/// The sights a weapon can carry besides its iron sights.
pub fn sights_for(id: WeaponId) -> &'static [SightId] {
    weapon(id).model.as_ref().map_or(&[], |model| model.sights)
}

/// Whether a weapon can carry a sight other than its iron sights.
pub fn takes_sights(id: WeaponId) -> bool {
    !sights_for(id).is_empty()
}

/// The sight a weapon really carries when `wanted` is asked for: a sight it
/// cannot take (or an unknown one) leaves it with its iron sights.
pub fn sight_on(weapon_id: WeaponId, wanted: SightId) -> SightId {
    if sights_for(weapon_id).contains(&wanted) {
        wanted
    } else {
        IRON_SIGHTS
    }
}

/// How far a knife reaches, in metres.
pub const KNIFE_RANGE: f32 = 2.6;
/// Seconds it takes to bring a weapon up after switching to it.
pub const DRAW_TIME: f32 = 0.35;

pub const RIFLE: WeaponId = 0;
pub const SMG: WeaponId = 1;
pub const SHOTGUN: WeaponId = 2;
pub const SNIPER: WeaponId = 3;
pub const PISTOL: WeaponId = 4;
pub const KNIFE: WeaponId = 5;
/// Not in the table: used only to label grenade kills.
pub const GRENADE: WeaponId = 6;
/// The weapons that can go in a gun slot are `0..GUN_COUNT`.
pub const GUN_COUNT: WeaponId = 5;

pub const WEAPONS: [Weapon; 6] = [
    Weapon {
        name: "Rifle",
        damage: 20,
        interval: 0.11,
        auto: true,
        pellets: 1,
        magazine: 30,
        reload: 1.6,
        reload_per_round: false,
        cycle: None,
        speed: 200.0,
        gravity: 4.0,
        hip_spread: 0.014,
        aim_spread: 0.0,
        mobility: 1.0,
        size: [0.04, 0.055, 0.3],
        sight_relief: 0.08,
        kick_up: 0.014,
        kick_side: 0.006,
        headshot: 2.0,
        model: Some(Model {
            path: "models/weapons/rifle.glb",
            fit: Fit::AsBuilt,
            sights: &[RED_DOT, REFLEX, TWO_X],
        }),
        choke: None,
    },
    Weapon {
        name: "SMG",
        damage: 13,
        interval: 0.065,
        auto: true,
        pellets: 1,
        magazine: 36,
        reload: 1.3,
        reload_per_round: false,
        cycle: None,
        speed: 150.0,
        gravity: 5.0,
        hip_spread: 0.024,
        aim_spread: 0.008,
        mobility: 1.05,
        size: [0.04, 0.06, 0.2],
        sight_relief: 0.08,
        kick_up: 0.018,
        kick_side: 0.007,
        headshot: 1.6,
        model: Some(Model {
            path: "models/weapons/smg.glb",
            fit: Fit::AsBuilt,
            sights: &[RED_DOT, REFLEX, TWO_X],
        }),
        choke: None,
    },
    Weapon {
        name: "Shotgun",
        damage: 12,
        interval: 0.8,
        auto: false,
        pellets: 9,
        magazine: 6,
        reload: 0.4,
        reload_per_round: true,
        cycle: Some(Cycle::Pump),
        speed: 130.0,
        gravity: 6.0,
        hip_spread: 0.06,
        aim_spread: 0.045,
        mobility: 1.0,
        size: [0.05, 0.06, 0.34],
        sight_relief: 0.08,
        kick_up: 0.075,
        kick_side: 0.018,
        headshot: 1.5,
        model: Some(Model {
            path: "models/weapons/Shotgun.glb",
            fit: Fit::Scaled { barrel_plus_z: true },
            sights: &[TWO_X],
        }),
        choke: Some(Choke {
            spread: 0.015,
            time: 0.5,
        }),
    },
    Weapon {
        name: "Sniper",
        damage: 90,
        interval: 1.2,
        auto: false,
        pellets: 1,
        magazine: 5,
        reload: 2.4,
        reload_per_round: false,
        cycle: Some(Cycle::Bolt),
        speed: 400.0,
        gravity: 3.0,
        hip_spread: 0.05,
        aim_spread: 0.0,
        mobility: 0.9,
        size: [0.035, 0.05, 0.44],
        sight_relief: 0.08,
        kick_up: 0.1,
        kick_side: 0.012,
        headshot: 2.5,
        model: Some(Model {
            path: "models/weapons/sniper.glb",
            fit: Fit::AsBuilt,
            sights: &[RED_DOT, REFLEX, TWO_X, SCOPE],
        }),
        choke: None,
    },
    Weapon {
        name: "Pistol",
        damage: 26,
        interval: 0.17,
        auto: false,
        pellets: 1,
        magazine: 12,
        reload: 1.1,
        reload_per_round: false,
        cycle: None,
        speed: 170.0,
        gravity: 5.0,
        hip_spread: 0.012,
        aim_spread: 0.002,
        mobility: 1.05,
        size: [0.035, 0.05, 0.15],
        sight_relief: 0.18,
        kick_up: 0.03,
        kick_side: 0.007,
        headshot: 2.0,
        model: Some(Model {
            path: "models/weapons/pistol.glb",
            fit: Fit::AsBuilt,
            // A red dot is too big for its slide.
            sights: &[REFLEX, TWO_X],
        }),
        choke: None,
    },
    Weapon {
        name: "Knife",
        damage: 150,
        interval: 0.2,
        auto: true,
        pellets: 0,
        magazine: 0,
        reload: 0.0,
        reload_per_round: false,
        cycle: None,
        speed: 0.0,
        gravity: 0.0,
        hip_spread: 0.0,
        aim_spread: 0.0,
        mobility: 1.32,
        // Blade and handle together.
        size: [0.012, 0.04, 0.3],
        sight_relief: 0.08,
        kick_up: 0.0,
        kick_side: 0.0,
        headshot: 1.0,
        model: Some(Model {
            path: "models/weapons/Knife.glb",
            fit: Fit::Scaled { barrel_plus_z: true },
            sights: &[],
        }),
        choke: None,
    },
];

/// Unknown ids are treated as the rifle.
pub fn weapon(id: WeaponId) -> &'static Weapon {
    WEAPONS.get(id as usize).unwrap_or(&WEAPONS[RIFLE as usize])
}

/// Damage one bullet (or stab) of this weapon deals.
pub fn hit_damage(id: WeaponId, headshot: bool) -> i32 {
    let stats = weapon(id);
    if headshot {
        (stats.damage as f32 * stats.headshot).round() as i32
    } else {
        stats.damage
    }
}

/// Name for the kill feed.
pub fn weapon_name(id: WeaponId) -> &'static str {
    if id == GRENADE { "Grenade" } else { weapon(id).name }
}

/// The gun after `current` in the list, skipping the one in the other slot.
pub fn next_gun(current: WeaponId, other_slot: WeaponId) -> WeaponId {
    step_gun(current, other_slot, 1)
}

/// The gun `step` places after `current` in the list (before it, for a
/// negative step), going round, and skipping the one in the other slot.
pub fn step_gun(current: WeaponId, other_slot: WeaponId, step: i32) -> WeaponId {
    let count = GUN_COUNT as i32;
    let mut gun = current as i32;
    for _ in 0..step.unsigned_abs() {
        gun = (gun + step.signum()).rem_euclid(count);
        if gun == other_slot as i32 {
            gun = (gun + step.signum()).rem_euclid(count);
        }
    }
    gun as WeaponId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_consistent() {
        assert_eq!(WEAPONS[KNIFE as usize].name, "Knife");
        assert_eq!(WEAPONS[PISTOL as usize].name, "Pistol");
        for gun in &WEAPONS[..GUN_COUNT as usize] {
            assert!(gun.pellets >= 1 && gun.magazine >= 1 && gun.speed > 50.0, "{}", gun.name);
            assert!(gun.aim_spread <= gun.hip_spread, "{}", gun.name);
            if let Some(choke) = &gun.choke {
                assert!(choke.spread < gun.aim_spread && choke.time > 0.0, "{}", gun.name);
            }
        }
    }

    #[test]
    fn every_gun_takes_the_2x_sight_and_each_sight_zooms_in() {
        for (id, gun) in WEAPONS.iter().enumerate().take(GUN_COUNT as usize) {
            assert!(sights_for(id as WeaponId).contains(&TWO_X), "{}", gun.name);
        }
        assert!(sights_for(KNIFE).is_empty());
        for sight in &SIGHTS {
            assert!(sight.zoom > 0.0 && sight.zoom < 1.0, "{}", sight.name);
        }
    }

    #[test]
    fn stepping_back_undoes_stepping_forward() {
        for other in 0..GUN_COUNT {
            for gun in (0..GUN_COUNT).filter(|&gun| gun != other) {
                assert_eq!(step_gun(step_gun(gun, other, 1), other, -1), gun);
                assert_ne!(step_gun(gun, other, -1), other);
            }
        }
    }

    #[test]
    fn cycling_guns_never_duplicates_a_slot() {
        for other in 0..GUN_COUNT {
            let mut gun = (other + 1) % GUN_COUNT;
            for _ in 0..10 {
                gun = next_gun(gun, other);
                assert!(gun != other && gun < GUN_COUNT);
            }
        }
    }
}
