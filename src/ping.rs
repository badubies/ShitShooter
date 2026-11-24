//! Pings: point out an enemy (or a spot) to your team, as often as you like.
//! The marker stays where a pinged enemy was, seen through walls, for a few seconds. The server passes
//! pings on to the pinger's teammates only, so the other team never sees them.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::{
    AppState,
    audio::{Sound, SoundCue},
    map,
    net::{Connection, NetMsg, Session},
    player::{LocalPlayer, PlayerSystems},
    protocol::*,
    remote::{RemotePlayer, RemoteSystems},
    settings::{Action, ActionInput},
    world::{GameMap, MainCamera},
};

/// Pings reach this far.
const PING_RANGE: f32 = 150.0;
/// An enemy this close to the crosshair (radians) is the one pinged...
const PING_ANGLE: f32 = 0.07;
/// ...or, up close, anywhere within this far of the line of aim (metres).
const PING_REACH: f32 = 0.9;
/// The marker floats this far above a pinged enemy's middle: over their head.
const ABOVE_HEAD: f32 = 1.35;
/// Markers off the screen are held this far in from its edge.
const EDGE_MARGIN: f32 = 40.0;
/// Width of a marker's box; its diamond sits in the middle of the top.
const MARKER_WIDTH: f32 = 160.0;
const DIAMOND: f32 = 12.0;

const ENEMY_COLOR: Color = Color::srgb(1.0, 0.3, 0.25);
const SPOT_COLOR: Color = Color::srgb(1.0, 0.82, 0.25);
/// The diamond is see-through, so it hides little of what it marks.
const DIAMOND_ALPHA: f32 = 0.5;

pub struct PingPlugin;

impl Plugin for PingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pings>()
            .add_systems(OnExit(AppState::Playing), |mut pings: ResMut<Pings>| *pings = Pings::default())
            .add_systems(
                Update,
                (
                    (receive, send, age)
                        .chain()
                        .after(PlayerSystems::Simulate)
                        .after(RemoteSystems)
                        .distributive_run_if(resource_exists::<Connection>),
                    draw_markers.after(PlayerSystems::Camera),
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

/// The pings on screen: at most one per teammate (and one of our own).
#[derive(Resource, Default)]
struct Pings {
    list: Vec<Ping>,
}

struct Ping {
    from: PlayerId,
    /// The enemy pinged, if it was one.
    target: Option<PlayerId>,
    /// Where it is: the spot, or where the enemy was when pinged.
    pos: Vec3,
    /// Seconds it has left on screen.
    left: f32,
}

impl Pings {
    /// A new ping replaces the last one from the same player.
    fn add(&mut self, from: PlayerId, target: Option<PlayerId>, pos: Vec3) {
        self.list.retain(|ping| ping.from != from);
        self.list.push(Ping {
            from,
            target,
            pos,
            left: PING_TIME,
        });
    }
}

/// The marker on screen for one player's ping.
#[derive(Component)]
struct PingMarker(PlayerId);

/// The line of text under a marker's diamond.
#[derive(Component)]
struct PingLabel(PlayerId);

#[derive(Component)]
struct PingDiamond(PlayerId);

fn receive(
    mut messages: MessageReader<NetMsg>,
    session: Res<Session>,
    mut pings: ResMut<Pings>,
    mut sounds: MessageWriter<SoundCue>,
) {
    for NetMsg(msg) in messages.read() {
        match msg {
            // The server only sends us our team's, but never show the other team's.
            ServerMsg::Ping { from, target, pos } if !session.enemies(session.my_id, *from) => {
                pings.add(*from, *target, Vec3::from(*pos));
                sounds.write(SoundCue {
                    sound: Sound::Ping,
                    at: None,
                });
            }
            // A pinged enemy who dies is dealt with.
            ServerMsg::Kill { victim, .. } => pings.list.retain(|ping| ping.target != Some(*victim)),
            _ => {}
        }
    }
}

/// The ping key pings the enemy nearest the crosshair that we can see, or
/// else the spot we are looking at.
fn send(
    input: ActionInput,
    map: Res<GameMap>,
    session: Res<Session>,
    player: Res<LocalPlayer>,
    mut connection: ResMut<Connection>,
    mut pings: ResMut<Pings>,
    mut sounds: MessageWriter<SoundCue>,
    remotes: Query<(&RemotePlayer, &Transform)>,
) {
    if !input.just_pressed(Action::Ping) || !player.alive {
        return;
    }
    let eye = player.eye();
    let aim = map::look_dir(player.yaw, player.pitch);
    let enemy = remotes
        .iter()
        .filter(|(remote, _)| remote.alive && session.enemies(session.my_id, remote.id))
        .filter_map(|(remote, transform)| {
            let center = transform.translation;
            let to = center - eye;
            let distance = to.length();
            let angle = aim.angle_between(to);
            let close_enough = angle < PING_ANGLE || distance * angle.sin() < PING_REACH && aim.dot(to) > 0.0;
            let seen = map.0.line_of_sight(eye, center) || map.0.line_of_sight(eye, center + Vec3::Y * 0.6);
            (distance < PING_RANGE && close_enough && seen).then_some((angle, remote.id, center))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    let (target, pos) = match enemy {
        Some((_, id, center)) => (Some(id), center),
        None => match map.0.raycast(eye, aim, PING_RANGE) {
            // Just short of the wall, so the marker is not inside it.
            Some(wall) => (None, eye + aim * (wall - 0.1).max(0.0)),
            None => return,
        },
    };
    pings.add(session.my_id, target, pos);
    connection.send(&ClientMsg::Ping {
        target,
        pos: pos.to_array(),
    });
    sounds.write(SoundCue {
        sound: Sound::Ping,
        at: None,
    });
}

fn age(time: Res<Time>, mut pings: ResMut<Pings>) {
    for ping in &mut pings.list {
        ping.left -= time.delta_secs();
    }
    pings.list.retain(|ping| ping.left > 0.0);
}

/// Where on the screen (logical pixels, y down) a point in the world is. A
/// point off the screen, or behind us, is pulled in to the edge in its direction.
fn screen_point(camera: &Transform, fov: f32, size: Vec2, world: Vec3) -> Vec2 {
    let local = camera.rotation.inverse() * (world - camera.translation);
    let center = size / 2.0;
    let half_height = (fov / 2.0).tan();
    let half_width = half_height * size.x / size.y.max(1.0);
    let offset = if local.z < -0.01 {
        // In front: -z is forward.
        Vec2::new(local.x / -local.z / half_width, -local.y / -local.z / half_height) * center
    } else {
        // Behind: straight out to the side it is on (below us if dead behind).
        let side = Vec2::new(local.x, -local.y);
        let side = if side.length_squared() < 1e-6 { Vec2::Y } else { side.normalize() };
        side * size.length()
    };
    let room = (center - Vec2::splat(EDGE_MARGIN)).max(Vec2::ONE);
    let over = (offset.abs() / room).max_element();
    center + if over > 1.0 { offset / over } else { offset }
}

/// Where on the screen (logical pixels, y down) a point in the world is, if
/// it is on the screen at all.
pub fn on_screen(camera: &Transform, fov: f32, size: Vec2, world: Vec3) -> Option<Vec2> {
    let local = camera.rotation.inverse() * (world - camera.translation);
    if local.z > -0.05 {
        return None;
    }
    let half_height = (fov / 2.0).tan();
    let half_width = half_height * size.x / size.y.max(1.0);
    let at = Vec2::new(local.x / -local.z / half_width, -local.y / -local.z / half_height);
    (at.abs().max_element() <= 1.0).then(|| (at + Vec2::ONE) * size / 2.0)
}

fn draw_markers(
    mut commands: Commands,
    session: Res<Session>,
    pings: Res<Pings>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Transform, &Projection), With<MainCamera>>,
    mut markers: Query<(Entity, &PingMarker, &mut Node)>,
    mut labels: Query<(&PingLabel, &mut Text, &mut TextColor)>,
    mut diamonds: Query<(&PingDiamond, &mut BackgroundColor)>,
) {
    let (camera, projection) = camera.into_inner();
    let fov = match projection {
        Projection::Perspective(perspective) => perspective.fov,
        _ => 1.0,
    };
    let size = crate::ui::ui_size(&window, &ui_scale);

    for (entity, marker, _) in &markers {
        if !pings.list.iter().any(|ping| ping.from == marker.0) {
            commands.entity(entity).despawn();
        }
    }
    for ping in &pings.list {
        let enemy = ping.target.is_some();
        let world = ping.pos + if enemy { Vec3::Y * ABOVE_HEAD } else { Vec3::ZERO };
        let at = screen_point(camera, fov, size, world);
        let (left, top) = (px(at.x - MARKER_WIDTH / 2.0), px(at.y - DIAMOND / 2.0));
        let color = if enemy { ENEMY_COLOR } else { SPOT_COLOR };
        // Fades out over its last half second.
        let fade = (ping.left / 0.5).min(1.0);
        let color = color.with_alpha(fade);
        let diamond_color = color.with_alpha(fade * DIAMOND_ALPHA);

        // Who pinged it, when it was not us.
        let text = if ping.from != session.my_id {
            session.name_of(ping.from).to_string()
        } else {
            String::new()
        };

        if let Some((_, _, mut node)) = markers.iter_mut().find(|(_, marker, _)| marker.0 == ping.from) {
            if node.left != left || node.top != top {
                node.left = left;
                node.top = top;
            }
            for (label, mut label_text, mut label_color) in &mut labels {
                if label.0 == ping.from {
                    crate::ui::set_text(&mut label_text, text.clone());
                    label_color.set_if_neq(TextColor(color));
                }
            }
            for (diamond, mut background) in &mut diamonds {
                if diamond.0 == ping.from {
                    background.set_if_neq(BackgroundColor(diamond_color));
                }
            }
            continue;
        }
        commands.spawn((
            PingMarker(ping.from),
            Node {
                position_type: PositionType::Absolute,
                left,
                top,
                width: px(MARKER_WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(5),
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(AppState::Playing),
            children![
                (
                    PingDiamond(ping.from),
                    Node {
                        width: px(DIAMOND),
                        height: px(DIAMOND),
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                    BackgroundColor(diamond_color),
                    BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                ),
                (
                    PingLabel(ping.from),
                    crate::ui::label(text, 14.0, color),
                    TextLayout::justify(Justify::Center),
                    TextShadow {
                        offset: Vec2::splat(1.5),
                        color: Color::srgba(0.0, 0.0, 0.0, 0.8),
                    },
                ),
            ],
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_land_on_the_screen_or_its_edge() {
        let camera = Transform::IDENTITY;
        let size = Vec2::new(1600.0, 900.0);
        let fov = 1.2;
        // Straight ahead is the middle.
        assert!(screen_point(&camera, fov, size, Vec3::new(0.0, 0.0, -10.0)).distance(size / 2.0) < 0.01);
        // Up and to the right is up and to the right.
        let up_right = screen_point(&camera, fov, size, Vec3::new(1.0, 1.0, -10.0));
        assert!(up_right.x > 800.0 && up_right.y < 450.0);
        // Behind, to the left: on the left edge.
        let behind = screen_point(&camera, fov, size, Vec3::new(-5.0, 0.0, 10.0));
        assert!((behind.x - EDGE_MARGIN).abs() < 0.01, "{behind}");
        // Far off to the side in front: held at the edge, not off it.
        let side = screen_point(&camera, fov, size, Vec3::new(100.0, 0.0, -1.0));
        assert!((side.x - (size.x - EDGE_MARGIN)).abs() < 0.01, "{side}");
    }

    #[test]
    fn a_new_ping_replaces_the_old_one_from_the_same_player() {
        let mut pings = Pings::default();
        pings.add(1, None, Vec3::ZERO);
        pings.add(2, Some(7), Vec3::ONE);
        pings.add(1, Some(8), Vec3::X);
        assert_eq!(pings.list.len(), 2);
        assert_eq!(pings.list.iter().find(|p| p.from == 1).unwrap().target, Some(8));
    }
}
