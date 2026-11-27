//! The game modes. Everything the rest of the game needs to know about a
//! mode without knowing which one it is sits in one row of `MODES`: add a
//! mode by adding a row (and, for its rules, a file beside this one).

// In the team modes players are split into two teams that cannot hurt each
// other.
pub const MODE_FFA: u8 = 0;
/// Team deathmatch: every kill is a point for the team.
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
}

pub const MODES: [ModeInfo; 6] = [
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
        score_limit: |_| crate::protocol::HILL_SCORE_LIMIT,
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
        score_limit: |_| crate::protocol::CONQUEST_TICKETS,
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
    },
];

/// How many modes there are: their ids are `0..count()`.
pub const fn count() -> u8 {
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
        assert_eq!(score_limit(MODE_HILL, 20), crate::protocol::HILL_SCORE_LIMIT);
        assert_eq!(score_limit(MODE_CONQUEST, 20), crate::protocol::CONQUEST_TICKETS);
        assert_eq!(score_limit(MODE_RACE, 20), 0);
    }
}
