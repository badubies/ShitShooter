//! The sniper mode on the client: what it shows at the top of the screen and
//! on the scoreboard, the words that come up as a turn goes (what you are
//! this turn, that you made it across, how the turn ended), and the beam
//! from the sniper's rifle that shows the runners where it points.

use bevy::{light::NotShadowCaster, prelude::*};

use super::{INTERMISSION, ROLE_RUNNER, ROLE_SNIPER};
use crate::{
    AppState,
    combat::Notices,
    gun_model::{FittedModels, GunHolder},
    map::{PLAYER_HALF, STAND_EYE, look_dir},
    modes::{
        MODE_SNIPER, ModeSnap, ModeStats,
        hud::{Column, KILLS, ModeHud, TopBar},
    },
    net::{NetMsg, Session},
    player::PlayerSystems,
    protocol::{PlayerInfo, ServerMsg},
    remote::{Gun, RemotePlayer, RemoteSystems},
    ui::{GOOD, RED},
    world::{GameMap, MainCamera},
};

/// The beam reaches this far where nothing is in its way (metres).
const LASER_REACH: f32 = 400.0;
/// The beam is drawn in this many pieces end to end, each as thick as it
/// takes to be seen from where we are: `LASER_WIDTH` at least, and at least
/// `LASER_SEEN` of how far away it is (a pixel or two), so that it does not
/// thin out to nothing down the length of the map.
const LASER_PIECES: usize = 24;
const LASER_WIDTH: f32 = 0.012;
const LASER_SEEN: f32 = 0.0022;
/// The dot where the beam lands, likewise.
const DOT_WIDTH: f32 = 0.09;
const DOT_SEEN: f32 = 0.007;
/// The beam starts at the muzzle of the rifle we draw in the sniper's hands
/// if that is at most this far from their eyes; else from under the eyes.
const MUZZLE_REACH: f32 = 2.0;

pub struct SniperPlugin;

impl Plugin for SniperPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Playing), spawn_laser).add_systems(
            Update,
            (announce, aim_laser.after(RemoteSystems).after(PlayerSystems::Camera)).run_if(in_state(AppState::Playing)),
        );
    }
}

/// A piece of the sniper's beam, counted from the rifle, or the dot at its end.
#[derive(Component)]
enum Laser {
    Piece(usize),
    Dot,
}

fn spawn_laser(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    // Its own colour whatever the light: a glow bright enough to bloom
    // washes out to pink.
    let light = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.05, 0.04),
        unlit: true,
        ..default()
    });
    let piece = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let dot = meshes.add(Sphere::new(0.5).mesh().ico(2).expect("valid sphere"));
    let parts = (0..LASER_PIECES).map(|i| (Laser::Piece(i), piece.clone())).chain([(Laser::Dot, dot)]);
    for (part, mesh) in parts {
        commands.spawn((
            part,
            Mesh3d(mesh),
            MeshMaterial3d(light.clone()),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            DespawnOnExit(AppState::Playing),
        ));
    }
}

/// Draws the beam from the sniper's rifle to what they are looking at, for
/// everybody but the sniper: a runner sees where the rifle points, and so
/// when to stay behind something. It lands where a shot fired now would go
/// (from the eyes, the way they look), not where the barrel we draw points.
fn aim_laser(
    session: Res<Session>,
    map: Res<GameMap>,
    fitted_models: Res<FittedModels>,
    camera: Query<&Transform, (With<MainCamera>, Without<Laser>)>,
    remotes: Query<(&RemotePlayer, &Transform), Without<Laser>>,
    guns: Query<(&Gun, &GunHolder, &GlobalTransform)>,
    mut parts: Query<(&Laser, &mut Transform, &mut Visibility)>,
) {
    let sniper = match session.snap {
        ModeSnap::Sniper { sniper, between: false, .. } if session.mode == MODE_SNIPER => {
            remotes.iter().find(|(remote, _)| remote.id == sniper && remote.alive)
        }
        _ => None,
    };
    let (Some((remote, at)), Ok(camera)) = (sniper, camera.single()) else {
        for (_, _, mut visibility) in &mut parts {
            visibility.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    let eye = at.translation + Vec3::Y * (STAND_EYE - PLAYER_HALF.y);
    let (yaw, pitch) = remote.aim();
    let dir = look_dir(yaw, pitch);
    let hit = map.0.raycast(eye, dir, LASER_REACH);
    let end = eye + dir * hit.unwrap_or(LASER_REACH);
    let from = guns
        .iter()
        .find(|(gun, ..)| gun.0 == remote.id)
        .and_then(|(_, holder, gun)| Some(gun.transform_point(fitted_models.get(holder.weapon)?.muzzle?)))
        .filter(|muzzle| muzzle.distance(eye) <= MUZZLE_REACH)
        .unwrap_or(eye + dir * 0.6 - Vec3::Y * 0.12);
    let along = end - from;
    let turned = Quat::from_rotation_arc(Vec3::Z, along.normalize_or(dir));
    let us = camera.translation;
    for (part, mut transform, mut visibility) in &mut parts {
        match part {
            Laser::Piece(i) => {
                let (a, b) = (*i as f32 / LASER_PIECES as f32, (*i + 1) as f32 / LASER_PIECES as f32);
                // As thick as its nearest point to us asks for.
                let near = (us - from).dot(along) / along.length_squared().max(1e-6);
                let near = from + along * near.clamp(a, b);
                let width = LASER_WIDTH.max(near.distance(us) * LASER_SEEN);
                *transform = Transform {
                    translation: from + along * (a + b) / 2.0,
                    rotation: turned,
                    scale: Vec3::new(width, width, along.length() * (b - a)),
                };
                visibility.set_if_neq(Visibility::Visible);
            }
            // On whatever the beam lands on; nowhere when that is the sky.
            Laser::Dot => {
                let width = DOT_WIDTH.max(end.distance(us) * DOT_SEEN);
                *transform = Transform::from_translation(end).with_scale(Vec3::splat(width));
                visibility.set_if_neq(if hit.is_some() { Visibility::Visible } else { Visibility::Hidden });
            }
        }
    }
}

/// How often `player` has made it across.
fn reached(player: &PlayerInfo) -> u8 {
    match player.stats {
        ModeStats::Sniper { reached } => reached,
        _ => 0,
    }
}

/// What we last knew, to tell when it changes.
#[derive(Default)]
struct Seen {
    reached: u8,
    between: bool,
}

/// The words in the middle of the screen: our part as each turn starts,
/// making it across, and how a turn ended.
fn announce(mut messages: MessageReader<NetMsg>, session: Res<Session>, mut notices: ResMut<Notices>, mut seen: Local<Seen>) {
    if session.mode != MODE_SNIPER {
        messages.clear();
        *seen = Seen::default();
        return;
    }
    let me = session.roster.iter().find(|player| player.id == session.my_id);
    for NetMsg(msg) in messages.read() {
        match msg {
            ServerMsg::MatchStarted { .. } => *seen = Seen::default(),
            // Put in for a turn: the roster that came just before says as what.
            ServerMsg::Spawn { id, .. } if *id == session.my_id => {
                let part = match me.map(|player| player.role) {
                    Some(ROLE_SNIPER) => "YOU ARE THE SNIPER",
                    Some(ROLE_RUNNER) => "GET TO THE PLATFORM",
                    _ => continue,
                };
                notices.banner = Some((part.to_string(), 4.0));
            }
            _ => {}
        }
    }
    let across = me.map_or(0, reached);
    if across > seen.reached {
        notices.banner = Some(("YOU MADE IT ACROSS".to_string(), 4.0));
    }
    seen.reached = across;
    let ModeSnap::Sniper { sniper, shot, through, between, .. } = session.snap else {
        return;
    };
    if between && !seen.between {
        let name = if sniper == session.my_id { "YOU".to_string() } else { session.name_of(sniper).to_uppercase() };
        notices.banner = Some((format!("{name} SHOT {shot}   ·   {through} MADE IT ACROSS"), INTERMISSION));
    }
    seen.between = between;
}

// ---------------------------------------------------------------- what the interface shows

pub const HUD: ModeHud = ModeHud {
    top_bar,
    columns: &[KILLS, ACROSS],
    rank: |a, b| b.kills.cmp(&a.kills).then(reached(b).cmp(&reached(a))),
    wins: |_| "MOST RUNNERS SHOT".to_string(),
    countdown: "TURN STARTS IN",
};

const ACROSS: Column = Column::new("ACROSS", |player| reached(player).to_string());

/// This turn's runners shot and runners across, and whose turn it is.
fn top_bar(session: &Session) -> TopBar {
    let ModeSnap::Sniper { sniper, turn, turns, shot, through, between } = session.snap else {
        return TopBar {
            labels: ["SHOT".to_string(), "ACROSS".to_string()],
            scores: ["0".to_string(), "0".to_string()],
            colors: [RED, GOOD],
            info: String::new(),
        };
    };
    let runners = session.roster.iter().filter(|player| player.role == ROLE_RUNNER).count();
    let running = runners.saturating_sub(shot as usize + through as usize);
    let who = if sniper == session.my_id {
        "you are the sniper".to_string()
    } else {
        format!("{} is the sniper", session.name_of(sniper))
    };
    let info = if between {
        format!("turn {turn} of {turns} is over   |   the next one starts in a moment")
    } else {
        format!("{who}   |   turn {turn} of {turns}   |   {running} still running")
    };
    TopBar {
        labels: ["SHOT".to_string(), "ACROSS".to_string()],
        scores: [shot.to_string(), through.to_string()],
        colors: [RED, GOOD],
        info,
    }
}
