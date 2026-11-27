//! The game modes. Everything the rest of the game needs to know about a
//! mode without knowing which one it is sits in one row of `MODES`: add a
//! mode by adding a row (and, for its rules, a file beside this one).

use bevy::math::Vec3;

use crate::{
    map::Map,
    protocol::{FlagSnap, PlayerId, RunEvent, Winner},
    server::Server,
};

pub mod conquest;
pub mod course;
pub mod ffa;
pub mod hill;
pub mod teams;

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

/// How the players of a mode stand to each other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sides {
    /// Everybody for themselves.
    Solo,
    /// Two teams that cannot hurt their own.
    Teams,
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
    /// The dead may ask to come back before `RESPAWN_DELAY` is up.
    pub early_respawn: bool,
    /// The match ends when the clock runs out. Without it, the leader ends it.
    pub timed: bool,
    /// The score that wins a match, given the lobby's frag limit: kills for
    /// one player, kills for a team, seconds on the hill, tickets. 0 where
    /// it is won on time, not on a score.
    pub score_limit: fn(u32) -> u32,
    pub rules: Rules,
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
    /// Who wins when the clock runs out, or the leader ends a match that has none.
    pub time_up: fn(&Server) -> Winner,
    /// Where this player may come in (place and facing), given where the
    /// people they should keep away from stand. `None`: any of the map's spawns.
    pub spawns: fn(&mut Server, PlayerId, &[Vec3]) -> Option<Vec<(Vec3, f32)>>,
    /// What of the mode's own state goes out with every snapshot.
    pub snap: fn(&Server) -> ModeSnap,
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
        time_up: |_| Winner::Nobody,
        spawns: |_, _, _| None,
        snap: |_| ModeSnap::None,
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
pub enum ModeSnap {
    None,
    /// Who holds the hill (`HILL_*`).
    Hill(u8),
    Conquest(Vec<FlagSnap>),
}

/// What the bots are to do besides fight, this tick.
#[derive(Default)]
pub struct BotBrief {
    /// Flags to take and hold, as they stand.
    pub flags: Vec<conquest::rules::FlagState>,
    /// A place for everybody to go to and stay within this far of.
    pub hold: Option<(Vec3, f32)>,
}

pub static MODES: [ModeInfo; 6] = [
    ModeInfo {
        name: "Free for all",
        key: "ffa",
        sides: Sides::Solo,
        needs: Needs::Arena,
        armed: true,
        bots: true,
        early_respawn: true,
        timed: true,
        score_limit: |frags| frags,
        rules: ffa::RULES,
    },
    ModeInfo {
        name: "Team deathmatch",
        key: "teams",
        sides: Sides::Teams,
        needs: Needs::Arena,
        armed: true,
        bots: true,
        early_respawn: true,
        timed: true,
        score_limit: |frags| frags * 2,
        rules: teams::RULES,
    },
    ModeInfo {
        name: "King of the hill",
        key: "hill",
        sides: Sides::Teams,
        needs: Needs::Hill,
        armed: true,
        bots: true,
        early_respawn: true,
        timed: true,
        score_limit: |_| hill::HILL_SCORE_LIMIT,
        rules: hill::RULES,
    },
    ModeInfo {
        name: "Conquest",
        key: "conquest",
        sides: Sides::Teams,
        needs: Needs::Flags,
        armed: true,
        bots: true,
        // Every death costs the team the whole wait.
        early_respawn: false,
        timed: true,
        score_limit: |_| conquest::CONQUEST_TICKETS,
        rules: conquest::rules::RULES,
    },
    ModeInfo {
        name: "Race",
        key: "race",
        sides: Sides::Solo,
        needs: Needs::Course,
        armed: false,
        bots: false,
        early_respawn: true,
        timed: true,
        score_limit: |_| 0,
        rules: course::rules::RACE,
    },
    ModeInfo {
        name: "Time trial",
        key: "trial",
        sides: Sides::Solo,
        needs: Needs::Course,
        armed: false,
        bots: false,
        early_respawn: true,
        // It goes on until the leader ends it.
        timed: false,
        score_limit: |_| 0,
        rules: course::rules::TRIAL,
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
            [at(MODE_FFA), at(MODE_TEAMS), at(MODE_HILL), at(MODE_CONQUEST), at(MODE_RACE), at(MODE_TRIAL)],
            ["ffa", "teams", "hill", "conquest", "race", "trial"]
        );
        for (mode, info) in MODES.iter().enumerate() {
            assert_eq!(by_key(&info.key.to_uppercase()), Some(mode as u8));
        }
        assert_eq!(by_key("nothing"), None);
        assert_eq!(name(200), "?");
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
