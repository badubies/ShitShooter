//! The game modes. Everything the rest of the game needs to know about a
//! mode without knowing which one it is sits in one row of `MODES`: add a
//! mode by adding a row (and, for its rules, a file beside this one).

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use crate::{
    map::Map,
    movement::MOVES_STANDARD,
    protocol::{FlagSnap, MAX_HEALTH, PlayerId, RunEvent, START_ARMOR, Winner},
    server::Server,
    weapons::{GUN_COUNT, KNIFE, WeaponId},
};

pub mod conquest;
pub mod course;
pub mod ffa;
pub mod hill;
#[cfg(feature = "client")]
pub mod hud;
pub mod sniper;
pub mod teams;

/// Each mode's own part of the game: what it draws and plays on top of the
/// rest. They all run all the time and mind only their own mode.
#[cfg(feature = "client")]
pub struct ClientPlugin;

#[cfg(feature = "client")]
impl bevy::app::Plugin for ClientPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_plugins((conquest::client::ConquestPlugin, course::client::RacePlugin, sniper::client::SniperPlugin));
    }
}

// In the team modes players are split into two teams that cannot hurt each
// other.
pub const MODE_FFA: u8 = 0;
/// Team deathmatch: every kill is a point for the team. (Nothing but the
/// tests asks for it by id: it is the plainest of the team modes.)
#[allow(dead_code)]
pub const MODE_TEAMS: u8 = 1;
/// King of the hill: a team scores a point for every second it alone holds
/// the zone in the middle of the map.
pub const MODE_HILL: u8 = 2;
/// Conquest: each team has tickets. Every death costs one, and the team
/// holding fewer of the map's flags bleeds them. Out of tickets, you lose.
pub const MODE_CONQUEST: u8 = 3;
/// Race: everybody sets off down the course together at "GO", with no guns
/// and no bots, and the first through the finish wins. Once somebody is
/// through, the rest have `RACE_FINISH_GRACE` seconds to follow.
pub const MODE_RACE: u8 = 4;
/// Time trial: the same course, each on your own clock, which starts as you
/// leave the start; as many runs as you like, and the best time wins.
pub const MODE_TRIAL: u8 = 5;
/// Sniper: one player on a platform with a rifle, everybody else running for
/// it from cover to cover. Everybody takes turns up there (see `sniper`).
pub const MODE_SNIPER: u8 = 6;

/// How the players of a mode stand to each other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sides {
    /// Everybody for themselves.
    Solo,
    /// Two teams that cannot hurt their own.
    Teams,
    /// Players are told apart by what they are this round (`PlayerInfo::role`),
    /// and those who are the same cannot hurt each other.
    Roles,
}

impl Sides {
    /// True if two players, each given as (team, role), are on the same
    /// side: they cannot hurt each other, and the dead of them may watch
    /// the living.
    pub fn allies(self, a: (u8, u8), b: (u8, u8)) -> bool {
        match self {
            Sides::Solo => false,
            Sides::Teams => a.0 & 1 == b.0 & 1,
            Sides::Roles => a.1 == b.1,
        }
    }
}

/// What a map must have for a mode to be played on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Needs {
    /// Somewhere to fight: any map that is not a course.
    Arena,
    /// A zone to hold.
    Hill,
    /// Flags to take, and a base for each team.
    Flags,
    /// A start, checkpoints and a finish. Courses are for nothing else.
    Course,
    /// A spawn for the sniper, spawns for the runners, and the finish they
    /// run for.
    Sniper,
}

/// What a map has, which decides the modes it is offered in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Offers {
    pub hill: bool,
    pub flags: bool,
    pub course: bool,
}

impl Offers {
    pub fn has(&self, needs: Needs) -> bool {
        match needs {
            Needs::Arena => !self.course,
            Needs::Hill => self.hill && !self.course,
            Needs::Flags => self.flags && !self.course,
            Needs::Course => self.course,
            // Only a map made in Blender has those, and it says so itself.
            Needs::Sniper => false,
        }
    }
}

pub struct ModeInfo {
    /// As shown in the lobby: "Free for all".
    pub name: &'static str,
    /// As typed after `--mode`: "ffa".
    pub key: &'static str,
    pub sides: Sides,
    pub needs: Needs,
    /// There are guns in it.
    pub armed: bool,
    /// Bots can play it.
    pub bots: bool,
    /// The dead come back by themselves, `RESPAWN_DELAY` after dying. Without
    /// it they are out until the mode puts everybody back in (a new round).
    pub respawns: bool,
    /// The dead may ask to come back before `RESPAWN_DELAY` is up.
    pub early_respawn: bool,
    /// The match ends when the clock runs out. Without it, the leader ends it.
    pub timed: bool,
    /// The score that wins a match, given the lobby's frag limit: kills for
    /// one player, kills for a team, seconds on the hill, tickets. 0 where
    /// it is won on time, not on a score.
    pub score_limit: fn(u32) -> u32,
    /// What the server asks it as a match goes on.
    pub rules: Rules,
    /// What it tells the interface to show.
    #[cfg(feature = "client")]
    pub hud: hud::ModeHud,
}

/// A mode's rules: what the server asks it as a match goes on. Each is a plain
/// function given the server. What a mode keeps between calls (who holds the
/// hill, the flags) is in `Server::state`, which `start` sets.
///
/// A mode fills in the ones it has something to say to and leaves the rest
/// as they are in `Rules::NONE`.
pub struct Rules {
    /// The match is starting: scores are at zero and nobody is in yet.
    pub start: fn(&mut Server),
    /// Every tick of a live match. Returns who just won it, if anybody.
    pub tick: fn(&mut Server, f32) -> Option<Winner>,
    /// Somebody was killed. Returns who just won the match, if anybody.
    pub killed: fn(&mut Server, &Kill) -> Option<Winner>,
    /// A player's game says what happened on their run of the course.
    pub run: fn(&mut Server, PlayerId, RunEvent),
    /// Somebody joined a match already under way.
    pub joined: fn(&mut Server, PlayerId),
    /// Who wins when the clock runs out, or the leader ends a match that has none.
    pub time_up: fn(&Server) -> Winner,
    /// Where this player may come in (place and facing), given where the
    /// people they should keep away from stand. `None`: any of the map's spawns.
    pub spawns: fn(&mut Server, PlayerId, &[Vec3]) -> Option<Vec<(Vec3, f32)>>,
    /// What this player is given on coming in.
    pub kit: fn(&Server, PlayerId) -> Kit,
    /// What of the mode's own state goes out with every snapshot.
    pub snap: fn(&Server) -> ModeSnap,
    /// What the scoreboard shows of this player that only this mode keeps.
    pub stats: fn(&Server, PlayerId) -> ModeStats,
    /// What the bots are to do besides fight.
    pub bot_brief: fn(&Server) -> BotBrief,
    /// Where fights on a map centre in this mode: bots pick their guns for
    /// how far one sees from there.
    pub fight_spots: fn(&Map) -> Vec<Vec3>,
}

impl Rules {
    /// A mode with no rules of its own: nobody ever wins.
    pub const NONE: Rules = Rules {
        start: |_| {},
        tick: |_, _| None,
        killed: |_, _| None,
        run: |_, _, _| {},
        joined: |_, _| {},
        time_up: |_| Winner::Nobody,
        spawns: |_, _, _| None,
        kit: |_, _| Kit::standard(),
        snap: |_| ModeSnap::None,
        stats: |_, _| ModeStats::None,
        bot_brief: |_| BotBrief::default(),
        fight_spots: |map| map.spawns.iter().map(|spawn| spawn.pos).collect(),
    };
}

/// A kill, as the rules are told of it.
pub struct Kill {
    pub killer: PlayerId,
    /// The victim's team (0 or 1).
    pub victim_team: u8,
    /// Killing yourself does not score. Whoever killed somebody else: how
    /// many kills they have now, and their team (0 or 1).
    pub scorer: Option<(u32, usize)>,
}

/// A mode's own part of a snapshot.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum ModeSnap {
    #[default]
    None,
    /// Who holds the hill (`HILL_*`).
    Hill(u8),
    /// The map's flags, in order.
    Conquest(Vec<FlagSnap>),
    Sniper {
        /// Whose turn it is on the platform (0: nobody's).
        sniper: PlayerId,
        /// The turn being played, from 1, of how many.
        turn: u8,
        turns: u8,
        /// This turn: runners shot, and runners through to the platform.
        shot: u8,
        through: u8,
        /// The turn is over, and the next has not begun.
        between: bool,
    },
}

/// A mode's own part of a player's line on the scoreboard.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub enum ModeStats {
    #[default]
    None,
    Course {
        /// A race's finishing time, or the best time of a time trial, once
        /// there is one.
        time: Option<f32>,
        /// How many checkpoints they are through (in a time trial, on the
        /// run they are on).
        gates: u8,
    },
    Sniper {
        /// How many times they made it to the platform.
        reached: u8,
    },
}

/// The guns of a `Kit`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Guns {
    /// The two picked in the player's own loadout.
    Own,
    /// These and no others: one or two.
    Only(Vec<WeaponId>),
    /// Nothing in hand at all, not even the knife.
    None,
}

/// What a player has for one life. The server decides it (`Rules::kit`),
/// tells the player's game with the spawn, and holds them to it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Kit {
    pub guns: Guns,
    /// A knife besides the guns.
    pub knife: bool,
    pub grenades: bool,
    /// Health to start with, and to heal back up to.
    pub health: i32,
    pub armor: i32,
    /// How they move: one of `movement::PRESETS`.
    pub moves: u8,
}

impl Kit {
    /// Their own guns, a knife and grenades.
    pub fn standard() -> Kit {
        Kit {
            guns: Guns::Own,
            knife: true,
            grenades: true,
            health: MAX_HEALTH,
            armor: START_ARMOR,
            moves: MOVES_STANDARD,
        }
    }

    /// Nothing to fight with.
    pub fn unarmed() -> Kit {
        Kit {
            guns: Guns::None,
            knife: false,
            grenades: false,
            ..Kit::standard()
        }
    }

    /// True if somebody with this kit may fire (or stab with) `weapon`.
    pub fn allows(&self, weapon: WeaponId) -> bool {
        match &self.guns {
            Guns::None => false,
            _ if weapon == KNIFE => self.knife,
            Guns::Own => weapon < GUN_COUNT,
            Guns::Only(guns) => guns.contains(&weapon),
        }
    }
}

/// What the bots are to do besides fight, this tick.
#[derive(Default)]
pub struct BotBrief {
    /// Flags to take and hold, as they stand.
    pub flags: Vec<conquest::rules::FlagState>,
    /// A place for everybody to go to and stay within this far of.
    pub hold: Option<(Vec3, f32)>,
}

pub static MODES: [ModeInfo; 7] = [
    ModeInfo {
        name: "Free for all",
        key: "ffa",
        sides: Sides::Solo,
        needs: Needs::Arena,
        armed: true,
        bots: true,
        respawns: true,
        early_respawn: true,
        timed: true,
        score_limit: |frags| frags,
        rules: ffa::RULES,
        #[cfg(feature = "client")]
        hud: hud::ModeHud::KILLS,
    },
    ModeInfo {
        name: "Team deathmatch",
        key: "teams",
        sides: Sides::Teams,
        needs: Needs::Arena,
        armed: true,
        bots: true,
        respawns: true,
        early_respawn: true,
        timed: true,
        score_limit: |frags| frags * 2,
        rules: teams::RULES,
        #[cfg(feature = "client")]
        hud: hud::ModeHud::TEAMS,
    },
    ModeInfo {
        name: "King of the hill",
        key: "hill",
        sides: Sides::Teams,
        needs: Needs::Hill,
        armed: true,
        bots: true,
        respawns: true,
        early_respawn: true,
        timed: true,
        score_limit: |_| hill::HILL_SCORE_LIMIT,
        rules: hill::RULES,
        #[cfg(feature = "client")]
        hud: hill::HUD,
    },
    ModeInfo {
        name: "Conquest",
        key: "conquest",
        sides: Sides::Teams,
        needs: Needs::Flags,
        armed: true,
        bots: true,
        respawns: true,
        // Every death costs the team the whole wait.
        early_respawn: false,
        timed: true,
        score_limit: |_| conquest::CONQUEST_TICKETS,
        rules: conquest::rules::RULES,
        #[cfg(feature = "client")]
        hud: conquest::client::HUD,
    },
    ModeInfo {
        name: "Race",
        key: "race",
        sides: Sides::Solo,
        needs: Needs::Course,
        armed: false,
        bots: false,
        respawns: true,
        early_respawn: true,
        timed: true,
        score_limit: |_| 0,
        rules: course::rules::RACE,
        #[cfg(feature = "client")]
        hud: course::client::RACE_HUD,
    },
    ModeInfo {
        name: "Time trial",
        key: "trial",
        sides: Sides::Solo,
        needs: Needs::Course,
        armed: false,
        bots: false,
        respawns: true,
        early_respawn: true,
        // It goes on until the leader ends it.
        timed: false,
        score_limit: |_| 0,
        rules: course::rules::TRIAL,
        #[cfg(feature = "client")]
        hud: course::client::TRIAL_HUD,
    },
    ModeInfo {
        name: "Sniper",
        key: "sniper",
        sides: Sides::Roles,
        needs: Needs::Sniper,
        armed: true,
        bots: false,
        // Shot, you are out until the next turn.
        respawns: false,
        early_respawn: false,
        timed: true,
        score_limit: |_| 0,
        rules: sniper::rules::RULES,
        #[cfg(feature = "client")]
        hud: sniper::client::HUD,
    },
];

/// How many modes there are: their ids are `0..count()`.
pub fn count() -> u8 {
    MODES.len() as u8
}

/// Mode `mode`, or free for all for a number that is no mode.
pub fn info(mode: u8) -> &'static ModeInfo {
    MODES.get(mode as usize).unwrap_or(&MODES[MODE_FFA as usize])
}

/// Mode `mode`'s name as shown, or "?" for a number that is no mode.
pub fn name(mode: u8) -> &'static str {
    MODES.get(mode as usize).map_or("?", |info| info.name)
}

/// The mode typed as `key` (any case), if there is one.
pub fn by_key(key: &str) -> Option<u8> {
    MODES.iter().position(|info| info.key.eq_ignore_ascii_case(key)).map(|mode| mode as u8)
}

/// Every mode's key, for help and error messages: "ffa, teams, hill".
pub fn keys() -> String {
    MODES.iter().map(|info| info.key).collect::<Vec<_>>().join(", ")
}

pub fn is_team_mode(mode: u8) -> bool {
    info(mode).sides == Sides::Teams
}

/// The racing modes, run on a course.
pub fn is_course_mode(mode: u8) -> bool {
    info(mode).needs == Needs::Course
}

/// The score that wins a match: see `ModeInfo::score_limit`.
pub fn score_limit(mode: u8, frag_limit: u32) -> u32 {
    (info(mode).score_limit)(frag_limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_are_where_their_ids_say() {
        let at = |mode: u8| MODES[mode as usize].key;
        assert_eq!(
            [at(MODE_FFA), at(MODE_TEAMS), at(MODE_HILL), at(MODE_CONQUEST), at(MODE_RACE), at(MODE_TRIAL), at(MODE_SNIPER)],
            ["ffa", "teams", "hill", "conquest", "race", "trial", "sniper"]
        );
        for (mode, info) in MODES.iter().enumerate() {
            assert_eq!(by_key(&info.key.to_uppercase()), Some(mode as u8));
        }
        assert_eq!(by_key("nothing"), None);
        assert_eq!(name(200), "?");
    }

    #[test]
    fn a_kit_allows_only_what_is_in_it() {
        use crate::weapons::{RIFLE, SNIPER};
        let standard = Kit::standard();
        assert!(standard.allows(RIFLE) && standard.allows(SNIPER) && standard.allows(KNIFE));
        assert!(!standard.allows(crate::weapons::GRENADE), "a grenade is thrown, not fired");
        let unarmed = Kit::unarmed();
        assert!(!unarmed.allows(RIFLE) && !unarmed.allows(KNIFE) && !unarmed.grenades);
        let sniper = Kit { guns: Guns::Only(vec![SNIPER]), knife: false, ..Kit::standard() };
        assert!(sniper.allows(SNIPER) && !sniper.allows(RIFLE) && !sniper.allows(KNIFE));
    }

    #[test]
    fn the_score_that_wins_depends_on_the_mode() {
        assert_eq!(score_limit(MODE_FFA, 20), 20);
        assert_eq!(score_limit(MODE_TEAMS, 20), 40);
        assert_eq!(score_limit(MODE_HILL, 20), hill::HILL_SCORE_LIMIT);
        assert_eq!(score_limit(MODE_CONQUEST, 20), conquest::CONQUEST_TICKETS);
        assert_eq!(score_limit(MODE_RACE, 20), 0);
    }
}
