//! The racing modes on the client: telling the server about our run as we
//! time it, the announcements for our checkpoints and finish and for
//! everybody else's, going back to the start or the last checkpoint, and
//! keeping guns out of sight. The timing itself is in `course.rs`, and runs
//! with our movement (`player.rs`).

use bevy::prelude::*;

use crate::{
    AppState,
    audio::{Sound, SoundCue},
    combat::{KillFeed, Notices},
    course::{Run, RunEvent, format_gap, format_time},
    modes::hud::{Column, ModeHud, TopBar},
    net::{Connection, NetMsg, Session},
    player::{LocalPlayer, PlayerSystems, Racing},
    protocol::*,
    remote::Gun,
    settings::{Action, ActionInput},
    ui::{ACCENT, TEXT},
    world::GameMap,
};

/// A time trial puts you back at the start this long after a finish.
pub const BACK_TO_START: f32 = 3.0;

pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BestRun>()
            .add_systems(OnEnter(AppState::Playing), |mut best: ResMut<BestRun>| *best = BestRun::default())
            .add_systems(
                Update,
                (
                    (restart, report_run)
                        .chain()
                        .after(PlayerSystems::Simulate)
                        .before(PlayerSystems::Camera)
                        .run_if(in_state(AppState::Playing))
                        .run_if(resource_exists::<Connection>),
                    announce_runs.run_if(resource_exists::<Connection>),
                    hide_guns,
                ),
            );
    }
}

/// Our best run of this match: its time, and when it went through each
/// checkpoint, to measure the next run's against.
#[derive(Resource, Default)]
pub struct BestRun {
    pub time: Option<f32>,
    pub splits: Vec<f32>,
}

/// "1st", "2nd", "3rd", "4th"...
pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, m) if m != 11 => "st",
        (2, m) if m != 12 => "nd",
        (3, m) if m != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The reload key puts us back: at the start of a time trial, or at the
/// last checkpoint of a race. A time trial goes back to the start by
/// itself a little after the finish.
fn restart(
    time: Res<Time>,
    input: ActionInput,
    map: Res<GameMap>,
    session: Res<Session>,
    mut player: ResMut<LocalPlayer>,
) {
    let Some(course) = &map.0.course else {
        return;
    };
    if player.racing == Racing::No || !player.alive || session.frozen() {
        return;
    }
    if player.run.finished.is_some() {
        player.finished_for += time.delta_secs();
        if player.racing == Racing::Trial && player.finished_for >= BACK_TO_START {
            player.run = Run::default();
            player.back_on_course(course, true);
        }
        return;
    }
    if input.just_pressed(Action::Reload) {
        let trial = player.racing == Racing::Trial;
        player.back_on_course(course, trial);
    }
}

/// Sends the server what happened on our run, and announces our
/// checkpoints and finish with how they compare with our best run.
fn report_run(
    map: Res<GameMap>,
    mut player: ResMut<LocalPlayer>,
    mut connection: ResMut<Connection>,
    mut best: ResMut<BestRun>,
    mut notices: ResMut<Notices>,
    mut sounds: MessageWriter<SoundCue>,
) {
    if player.run_events.is_empty() {
        return;
    }
    let gates = map.0.course.as_ref().map_or(0, |course| course.gates.len());
    let events = std::mem::take(&mut player.run_events);
    for event in events {
        connection.send(&ClientMsg::Run(event));
        match event {
            RunEvent::Gate { gate, time } => {
                let split = match best.splits.get(gate as usize) {
                    Some(then) => format!("   {}", format_gap(time - then)),
                    None => String::new(),
                };
                notices.banner = Some((format!("CHECKPOINT {}/{gates}   {}{split}", gate + 1, format_time(time)), 2.5));
                sounds.write(SoundCue {
                    sound: Sound::Countdown,
                    at: None,
                });
            }
            RunEvent::Finished { time } => {
                notices.kill = Some((format!("FINISHED   {}", format_time(time)), 4.0));
                if player.racing == Racing::Trial {
                    let line = match best.time {
                        Some(then) if time < then => format!("NEW BEST   {}", format_gap(time - then)),
                        Some(then) => format!("BEST {}   {}", format_time(then), format_gap(time - then)),
                        None => "FIRST RUN IN".to_string(),
                    };
                    notices.banner = Some((line, 4.0));
                    if best.time.is_none_or(|then| time < then) {
                        best.time = Some(time);
                        best.splits = player.run.splits.clone();
                    }
                }
                sounds.write(SoundCue {
                    sound: Sound::Go,
                    at: None,
                });
            }
            RunEvent::Started | RunEvent::Abandoned => {}
        }
    }
}

/// Finishes the server took: our place in a race, and in the feed where
/// the others came in (a race) or a best of theirs (a time trial).
fn announce_runs(
    mut messages: MessageReader<NetMsg>,
    session: Res<Session>,
    mut feed: ResMut<KillFeed>,
    mut notices: ResMut<Notices>,
) {
    for NetMsg(msg) in messages.read() {
        let ServerMsg::Run {
            id,
            event: RunEvent::Finished { time },
        } = msg
        else {
            continue;
        };
        // The roster that comes after this has the time; this one not yet.
        let ahead = session
            .roster
            .iter()
            .filter(|p| p.id != *id && p.time().is_some_and(|then| then < *time))
            .count();
        let name = session.name_of(*id);
        if session.mode == MODE_RACE {
            let place = ordinal(ahead + 1);
            if *id == session.my_id {
                notices.banner = Some((format!("{} PLACE", place.to_uppercase()), 4.0));
            } else {
                feed.say(format!("{name} finished {place}   {}", format_time(*time)));
            }
        } else if *id != session.my_id {
            let old = session.roster.iter().find(|p| p.id == *id).and_then(|p| p.time());
            if old.is_none_or(|old| *time < old) {
                let lead = if ahead == 0 { "   the best yet" } else { "" };
                feed.say(format!("{name}: a best of {}{lead}", format_time(*time)));
            }
        }
    }
}

/// On a course nobody carries a gun.
fn hide_guns(session: Res<Session>, mut guns: Query<&mut Visibility, With<Gun>>) {
    let wanted = if is_course_mode(session.mode) {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut visibility in &mut guns {
        visibility.set_if_neq(wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_read_as_ordinals() {
        let names: Vec<_> = [1, 2, 3, 4, 11, 12, 13, 21, 22].into_iter().map(ordinal).collect();
        assert_eq!(names, ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd"]);
    }
}

// ---------------------------------------------------------------- what the interface shows

pub const RACE_HUD: ModeHud = ModeHud {
    top_bar: race_bar,
    columns: &COLUMNS,
    rank: by_time,
    wins: |_| "FIRST THROUGH THE FINISH".to_string(),
    countdown: "RACE STARTS IN",
};

pub const TRIAL_HUD: ModeHud = ModeHud {
    top_bar: trial_bar,
    wins: |_| "BEST TIME".to_string(),
    countdown: "MATCH STARTS IN",
    ..RACE_HUD
};

/// Checkpoints and times instead of kills.
const COLUMNS: [Column; 2] = [
    Column::new("CP", |player| player.gates().to_string()),
    Column::new("TIME", |player| player.time().map_or("-".to_string(), format_time)).wide().bright(),
];

/// The best time, then (those with none yet) the most checkpoints.
fn by_time(a: &PlayerInfo, b: &PlayerInfo) -> std::cmp::Ordering {
    match (a.time(), b.time()) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.gates().cmp(&a.gates()),
    }
}

fn best_time(session: &Session) -> Option<(f32, &PlayerInfo)> {
    session.roster.iter().filter_map(|p| Some((p.time()?, p))).min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Our place, and how many are through the finish.
fn race_bar(session: &Session) -> TopBar {
    let mut order: Vec<_> = session.roster.iter().collect();
    order.sort_by(|a, b| by_time(a, b));
    let place = order.iter().position(|p| p.id == session.my_id).map_or(0, |i| i + 1);
    let finished = session.roster.iter().filter(|p| p.time().is_some()).count();
    let info = match best_time(session) {
        Some((time, p)) => format!("race   |   {} won in {}", p.name, format_time(time)),
        None => "race   |   first through the finish wins".to_string(),
    };
    TopBar {
        labels: ["PLACE".to_string(), "FINISHED".to_string()],
        scores: [ordinal(place), format!("{finished}/{}", session.roster.len())],
        colors: [ACCENT, TEXT],
        info,
    }
}

/// Our best time against the best there is.
fn trial_bar(session: &Session) -> TopBar {
    let me = session.roster.iter().find(|p| p.id == session.my_id);
    let best = best_time(session);
    let time = |time: Option<f32>| time.map_or("-".to_string(), format_time);
    let info = match best {
        Some((_, p)) => format!("time trial   |   {} has the best time", p.name),
        None => "time trial   |   the best time wins".to_string(),
    };
    TopBar {
        labels: ["YOUR BEST".to_string(), "BEST".to_string()],
        scores: [time(me.and_then(|p| p.time())), time(best.map(|(time, _)| time))],
        colors: [ACCENT, TEXT],
        info,
    }
}
