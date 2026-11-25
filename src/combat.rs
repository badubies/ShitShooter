//! Fighting: switching, reloading and firing weapons, the bullets in the air,
//! the knife, grenade throws, and the kill feed with its streak and revenge
//! announcements.

use std::collections::{HashMap, HashSet};

use bevy::{light::NotShadowCaster, prelude::*};

use crate::{
    AppState,
    audio::{BOLT_LOCK, PUMP_CLACK, Sound, SoundCue},
    gun_model::FittedModels,
    map,
    net::{Connection, NetMsg, Session},
    player::{
        BOLT_BACK, BOLT_HOME, BOLT_PULL, InHand, KNIFE_SLOT, LocalPlayer, MAGAZINE_IN, MAGAZINE_OUT, PUMP_AFTER, PUMP_TIME,
        PlayerSystems, RACK_TIME,
    },
    protocol::*,
    remote::{RemotePlayer, RemoteSystems},
    settings::{Action, ActionInput},
    weapons::{Cycle, DRAW_TIME, KNIFE, KNIFE_RANGE, WeaponId, weapon, weapon_name},
    world::{GameMap, SharedAssets, ViewModel},
};

/// Bullets vanish after this many seconds in the air.
const BULLET_LIFETIME: f32 = 1.5;
const BULLET_WIDTH: f32 = 0.03;
/// Seconds over which a bullet's streak closes from where it is drawn coming
/// out of the gun onto its real path from the eyes.
const BULLET_CONVERGE: f32 = 0.12;
/// Seconds the hit marker stays up after a hit.
pub const HIT_MARKER_TIME: f32 = 0.18;
/// A click on a semi-automatic gun still counts if the gun becomes ready
/// within this many seconds.
const TRIGGER_BUFFER: f32 = 0.2;
/// A choke only starts closing once the gun is this far aimed.
const CHOKE_AIM: f32 = 0.9;
/// Kills in one life that earn an announcement.
const STREAKS: [(u32, &str); 4] = [
    (3, "Killing spree"),
    (5, "Rampage"),
    (7, "Unstoppable"),
    (10, "Godlike"),
];

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KillFeed>()
            .init_resource::<Notices>()
            .add_systems(OnEnter(AppState::Playing), clear_feed)
            .add_systems(
                Update,
                (
                    (fire, fly_bullets)
                        .chain()
                        .after(PlayerSystems::Simulate)
                        .after(RemoteSystems)
                        .before(PlayerSystems::Camera)
                        .run_if(in_state(AppState::Playing))
                        .distributive_run_if(resource_exists::<Connection>),
                    handle_net.run_if(resource_exists::<Connection>),
                    age_feed,
                ),
            );
    }
}

/// Recent kills, newest last, with the seconds each line has left on screen.
#[derive(Resource, Default)]
pub struct KillFeed(pub Vec<(String, f32)>);

impl KillFeed {
    fn push(&mut self, line: String) {
        self.0.push((line, 6.0));
        if self.0.len() > 7 {
            self.0.remove(0);
        }
    }
}

/// The announcements in the middle of the screen, and what is needed to
/// decide them.
#[derive(Resource, Default)]
pub struct Notices {
    /// "FRAGGED somebody", with the seconds it has left on screen.
    pub kill: Option<(String, f32)>,
    /// The line under it: streaks and revenge.
    pub banner: Option<(String, f32)>,
    /// Kills each player has made since they last died.
    streaks: HashMap<PlayerId, u32>,
    /// (killer, victim) pairs that have not been avenged yet.
    grudges: HashSet<(PlayerId, PlayerId)>,
}

/// A bullet in flight. Ours are the real thing: when one reaches somebody we
/// tell the server. Other players' are only for show.
#[derive(Component)]
struct Bullet {
    pos: Vec3,
    vel: Vec3,
    gravity: f32,
    age: f32,
    weapon: WeaponId,
    shooter: PlayerId,
    /// Drawn this far from where it really is as it is fired, so that it seems
    /// to leave the gun's muzzle rather than the eyes. Fades out over
    /// `BULLET_CONVERGE`.
    offset: Vec3,
}

fn clear_feed(mut feed: ResMut<KillFeed>, mut notices: ResMut<Notices>) {
    feed.0.clear();
    *notices = Notices::default();
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    input: ActionInput,
    map: Res<GameMap>,
    assets: Res<SharedAssets>,
    session: Res<Session>,
    mut player: ResMut<LocalPlayer>,
    mut connection: ResMut<Connection>,
    mut sounds: MessageWriter<SoundCue>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    view_model: Query<&Transform, With<ViewModel>>,
    fitted_models: Res<FittedModels>,
    mut rng: Local<u32>,
    mut trigger_buffer: Local<f32>,
    mut fire_held: Local<bool>,
) {
    let dt = time.delta_secs();
    let player = &mut *player;
    let mut own_sound = |sound| {
        sounds.write(SoundCue { sound, at: None });
    };
    let cooled_from = player.guns.map(|gun| gun.cooldown);
    for gun in &mut player.guns {
        gun.cooldown = (gun.cooldown - dt).max(0.0);
    }
    player.knife_cooldown = (player.knife_cooldown - dt).max(0.0);
    player.draw = (player.draw - dt).max(0.0);
    let carried = grenade(player.grenade);
    player.grenades = (player.grenades + dt / carried.recharge).min(carried.max);
    if let Some(InHand { fuse: Some(fuse), .. }) = &mut player.hand {
        *fuse -= dt;
        if *fuse <= 0.0 {
            // Held too long: it goes off in our hand (the server sees to that).
            player.hand = None;
        }
    }
    if !player.alive {
        *trigger_buffer = 0.0;
        player.hand = None;
        return;
    }
    // Nothing to fight with until the countdown is over.
    if session.frozen() {
        *trigger_buffer = 0.0;
        return;
    }

    // The grenade key takes out the grenade in the loadout, or puts it away
    // again (not a frag with its pin pulled: that one has to go).
    if input.just_pressed(Action::Grenade) {
        match player.hand {
            Some(InHand { fuse: None, .. }) => player.hand = None,
            None if player.grenades >= 1.0 => {
                player.hand = Some(InHand {
                    kind: player.grenade,
                    fuse: None,
                });
                // The other hand is busy: a reload is abandoned.
                player.reload = 0.0;
            }
            _ => {}
        }
    }

    // Switching weapons takes a moment and abandons a reload. It also puts
    // away a grenade in hand, unless that is a frag with its pin pulled.
    let cooking = player.hand.is_some_and(|hand| hand.fuse.is_some());
    let slots = [(Action::Primary, 0), (Action::Secondary, 1), (Action::Knife, KNIFE_SLOT)];
    let mut wanted = slots.iter().rev().find(|(action, _)| input.just_pressed(*action)).map(|(_, slot)| *slot);
    // Next and previous go round the three in order.
    let step = input.just_pressed(Action::NextWeapon) as i32 - input.just_pressed(Action::PreviousWeapon) as i32;
    if step != 0 {
        let here = slots.iter().position(|(_, slot)| *slot == player.slot).unwrap_or(0) as i32;
        wanted = Some(slots[(here + step).rem_euclid(slots.len() as i32) as usize].1);
    }
    if let Some(slot) = wanted
        && !cooking
    {
        if player.hand.take().is_some() || player.slot != slot {
            player.slot = slot;
            player.draw = DRAW_TIME;
            player.reload = 0.0;
            own_sound(Sound::Draw);
        }
    }
    let weapon_id = player.weapon_id();
    let stats = weapon(weapon_id);
    let slot = player.slot;

    // Reloading: by hand, or by itself once the magazine is empty. Its sounds
    // come as the hands get to each part of it.
    if player.reload <= 0.0 {
        player.racking = false;
    }
    if player.slot < player.guns.len() {
        if player.reload > 0.0 {
            let before = player.reload_progress();
            player.reload -= dt;
            let after = player.reload_progress();
            let passed = |at: f32| before < at && after >= at;
            if player.racking {
                if passed(0.1) {
                    own_sound(Sound::Pump);
                }
            } else if stats.reload_per_round {
                // Firing stops a round by round reload after the round going in.
                if input.just_pressed(Action::Fire) {
                    player.reload_stop = true;
                }
                if passed(0.55) {
                    own_sound(Sound::ShellIn);
                }
            } else {
                if passed(MAGAZINE_OUT) {
                    own_sound(Sound::MagOut(weapon_id));
                }
                if passed(MAGAZINE_IN) {
                    own_sound(Sound::MagIn(weapon_id));
                }
                if passed(BOLT_PULL) {
                    own_sound(Sound::BoltPull(weapon_id));
                }
                if passed(BOLT_HOME) {
                    // A bolt action's bolt is pushed home and locked by hand,
                    // not let go to slam home.
                    own_sound(if stats.cycle == Some(Cycle::Bolt) { Sound::BoltHome } else { Sound::Bolt(weapon_id) });
                }
            }
            if player.reload <= 0.0 {
                let gun = &mut player.guns[slot];
                player.reload = 0.0;
                if player.racking {
                    player.racking = false;
                } else if stats.reload_per_round {
                    gun.ammo += 1;
                    if gun.ammo < stats.magazine && !player.reload_stop {
                        player.reload = stats.reload;
                    } else if player.reload_empty {
                        // Nothing in the chamber yet: work the pump.
                        player.racking = true;
                        player.reload = RACK_TIME;
                    }
                } else {
                    gun.ammo = stats.magazine;
                }
            }
        } else if player.draw <= 0.0
            && player.hand.is_none()
            && player.guns[slot].ammo < stats.magazine
            && (player.guns[slot].ammo == 0 || input.just_pressed(Action::Reload))
        {
            player.reload = stats.reload;
            player.reload_empty = player.guns[slot].ammo == 0;
            player.reload_stop = false;
            own_sound(Sound::Rustle);
        }
    }

    // A pump or bolt worked between shots, heard as the hands get to it. The
    // bolt locks down just as the next shot is ready: that click says so.
    if let (Some(cycle), Some(gun)) = (stats.cycle, player.guns.get(slot))
        && gun.ammo > 0
        && !player.reloading()
        && player.draw <= 0.0
    {
        let (before, after) = (stats.interval - cooled_from[slot], stats.interval - gun.cooldown);
        let passed = |at: f32| before < at && after >= at;
        match cycle {
            // So that its first clack comes as the pump is all the way back.
            Cycle::Pump if passed(PUMP_AFTER + PUMP_TIME / 2.0 - PUMP_CLACK) => own_sound(Sound::Pump),
            Cycle::Bolt if passed(BOLT_BACK) => own_sound(Sound::BoltBack),
            Cycle::Bolt if passed(stats.interval - BOLT_LOCK) => own_sound(Sound::BoltHome),
            _ => {}
        }
    }

    // A choke closes up while the gun is held aimed and steady.
    if stats.choke.is_some() && player.aim > CHOKE_AIM && player.draw <= 0.0 {
        let was = player.choke();
        player.choked_for += dt;
        if was < 1.0 && player.choke() >= 1.0 {
            own_sound(Sound::Choke);
        }
    } else {
        player.choked_for = 0.0;
    }

    let origin = player.eye();
    let aim_dir = map::look_dir(player.yaw, player.pitch);

    // With a grenade in hand, a click throws it: a frag takes one click to
    // pull the pin (its fuse burns from then on) and another to throw. A
    // right-click lobs it gently, pin and all.
    if let Some(mut hand) = player.hand {
        let lob = input.just_pressed(Action::Aim);
        if input.just_pressed(Action::Fire) || lob {
            *fire_held = true;
            let mut throw = true;
            if hand.kind == FRAG && hand.fuse.is_none() {
                if player.grenades >= 1.0 {
                    player.grenades -= 1.0;
                    hand.fuse = Some(grenade(FRAG).fuse);
                    connection.send(&ClientMsg::Cook);
                    own_sound(Sound::Pin);
                }
                throw = lob && hand.fuse.is_some();
            } else if hand.kind != FRAG {
                throw = player.grenades >= 1.0;
                if throw {
                    player.grenades -= 1.0;
                }
            }
            player.hand = Some(hand);
            if throw {
                player.hand = None;
                player.throw_anim = 1.0;
                // Let go of it a little in front of the face, unless a wall is right there.
                let reach = match map.0.raycast(origin, aim_dir, 0.9) {
                    Some(wall) => (wall - 0.3).max(0.0),
                    None => 0.6,
                };
                // Thrown slightly upwards, and it keeps some of our own momentum.
                let (speed, lift) = if lob {
                    (GRENADE_THROW_SPEED * GRENADE_LOB, 0.3)
                } else {
                    (GRENADE_THROW_SPEED, 0.15)
                };
                let vel = (aim_dir + Vec3::Y * lift).normalize() * speed + player.vel * 0.6;
                connection.send(&ClientMsg::Grenade {
                    kind: hand.kind,
                    origin: (origin + aim_dir * reach).to_array(),
                    vel: vel.to_array(),
                });
                own_sound(Sound::Throw);
            }
        }
    }
    // No shooting with a grenade in hand, nor until the click that threw it is let go.
    if !input.pressed(Action::Fire) {
        *fire_held = false;
    }
    if player.hand.is_some() || *fire_held {
        *trigger_buffer = 0.0;
        return;
    }

    // Automatic weapons fire while the trigger is held; the others once per click.
    if input.just_pressed(Action::Fire) {
        *trigger_buffer = TRIGGER_BUFFER;
    } else if !(player.reload_stop && player.reloading()) {
        // A click that stops a round by round reload fires once it is done.
        *trigger_buffer = (*trigger_buffer - dt).max(0.0);
    }
    let trigger = if stats.auto {
        input.pressed(Action::Fire)
    } else {
        *trigger_buffer > 0.0
    };
    let cooldown = player.guns.get(slot).map_or(player.knife_cooldown, |gun| gun.cooldown);
    if !trigger || player.reloading() || player.draw > 0.0 || cooldown > 0.0 {
        return;
    }
    *trigger_buffer = 0.0;

    let Some(gun) = player.guns.get_mut(slot) else {
        // The knife: it reaches whoever is closest in front of us, right now.
        player.knife_cooldown = stats.interval;
        player.recoil = 1.0;
        connection.send(&ClientMsg::Fire {
            weapon: KNIFE,
            origin: origin.to_array(),
            dirs: Vec::new(),
        });
        own_sound(Sound::Throw);
        let mut nearest: Option<(f32, PlayerId)> = None;
        for (remote, transform) in &remotes {
            if !remote.alive || !session.enemies(session.my_id, remote.id) {
                continue;
            }
            // A knife is wider than a bullet.
            let half = map::hitbox_half(remote.crouching) + Vec3::splat(0.25);
            let center = transform.translation;
            if let Some(reach) = map::ray_box(origin, aim_dir, center - half, center + half)
                && reach <= KNIFE_RANGE
                && nearest.is_none_or(|(best, _)| reach < best)
            {
                nearest = Some((reach, remote.id));
            }
        }
        if let Some((reach, victim)) = nearest
            && map.0.raycast(origin, aim_dir, reach).is_none()
        {
            connection.send(&ClientMsg::Hit {
                victim,
                weapon: KNIFE,
                headshot: false,
            });
            player.hit_marker = HIT_MARKER_TIME;
            own_sound(Sound::Hit);
        }
        return;
    };
    if gun.ammo == 0 {
        return;
    }
    gun.ammo -= 1;
    gun.cooldown = stats.interval;
    player.recoil = 1.0;

    // Every bullet goes to a random point inside the spread cone.
    let rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    let cone = player.spread();
    player.choked_for = 0.0;
    let dirs: Vec<Vec3> = (0..stats.pellets)
        .map(|_| {
            let off_axis = cone * next_random(&mut rng).sqrt();
            let around = next_random(&mut rng) * std::f32::consts::TAU;
            (rotation * Vec3::new(off_axis * around.cos(), off_axis * around.sin(), -1.0)).normalize()
        })
        .collect();
    connection.send(&ClientMsg::Fire {
        weapon: weapon_id,
        origin: origin.to_array(),
        dirs: dirs.iter().map(|dir| dir.to_array()).collect(),
    });
    own_sound(Sound::Gunshot(weapon_id));

    // From the gun's muzzle where it is in the view, hip or aimed, kicked or
    // not; for a gun whose model does not mark it, about where a gun's is.
    let muzzle = match (fitted_models.get(weapon_id).and_then(|fitted| fitted.muzzle), view_model.single()) {
        (Some(muzzle), Ok(view)) => view.transform_point(muzzle),
        _ => Vec3::new(0.2, -0.16, -0.7).lerp(Vec3::new(0.0, -0.06, -0.7), player.aim),
    };
    for dir in dirs {
        spawn_bullet(&mut commands, &assets, weapon_id, session.my_id, origin, dir, rotation * muzzle, true);
    }

    // Then the gun throws the view up and a little to one side. Aiming down
    // sights and crouching steady it.
    let steady = (1.0 - 0.3 * player.aim) * if player.crouching { 0.8 } else { 1.0 };
    let side = (next_random(&mut rng) * 2.0 - 1.0) * stats.kick_side;
    player.kick_view(Vec2::new(stats.kick_up, side) * steady);
}

/// xorshift32, returned as a float in 0..1.
fn next_random(state: &mut u32) -> f32 {
    if *state == 0 {
        *state = 0x9e37_79b9;
    }
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}

fn spawn_bullet(
    commands: &mut Commands,
    assets: &SharedAssets,
    weapon_id: WeaponId,
    shooter: PlayerId,
    origin: Vec3,
    dir: Vec3,
    offset: Vec3,
    mine: bool,
) {
    let stats = weapon(weapon_id);
    let material = if mine { &assets.tracer } else { &assets.enemy_tracer };
    commands.spawn((
        Bullet {
            pos: origin,
            vel: dir * stats.speed,
            gravity: stats.gravity,
            age: 0.0,
            weapon: weapon_id,
            shooter,
            offset,
        },
        Mesh3d(assets.cube.clone()),
        MeshMaterial3d(material.clone()),
        // Invisible until it has moved (see `fly_bullets`).
        Transform::from_translation(origin).with_scale(Vec3::ZERO),
        NotShadowCaster,
        DespawnOnExit(AppState::Playing),
    ));
}

/// Moves every bullet and stops it at the first wall or player in its way.
/// We aim at other players where we see them, so a hit that looks right to
/// the shooter counts.
fn fly_bullets(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<GameMap>,
    session: Res<Session>,
    mut player: ResMut<LocalPlayer>,
    mut connection: ResMut<Connection>,
    mut sounds: MessageWriter<SoundCue>,
    remotes: Query<(&RemotePlayer, &Transform), Without<Bullet>>,
    mut bullets: Query<(Entity, &mut Bullet, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut bullet, mut transform) in &mut bullets {
        let bullet = &mut *bullet;
        let mine = bullet.shooter == session.my_id;
        let step = bullet.vel * dt;
        let distance = step.length();
        if distance > 0.0 {
            let dir = step / distance;
            let wall = map.0.raycast(bullet.pos, dir, distance);
            let mut reach = wall.unwrap_or(distance);
            let mut stopped = wall.is_some();
            let mut victim = None;
            // Bullets pass through the shooter's own team.
            for (remote, body) in &remotes {
                if !remote.alive || !session.enemies(bullet.shooter, remote.id) {
                    continue;
                }
                let half = map::hitbox_half(remote.crouching);
                let center = body.translation;
                if let Some(t) = map::ray_box(bullet.pos, dir, center - half, center + half)
                    && t < reach
                {
                    reach = t;
                    let (head_min, head_max) = map::head_box(center, remote.crouching);
                    let headshot = map::ray_box(bullet.pos, dir, head_min, head_max).is_some();
                    victim = Some((remote.id, headshot));
                }
            }
            // Somebody else's bullet ends at our own body. Whether it hurt is
            // up to their client and the server.
            if !mine && player.alive && session.enemies(bullet.shooter, session.my_id) {
                let half = map::hitbox_half(player.crouching);
                stopped |= map::ray_box(bullet.pos, dir, player.pos - half, player.pos + half).is_some_and(|t| t < reach);
            }
            if let Some((victim, headshot)) = victim {
                stopped = true;
                if mine {
                    connection.send(&ClientMsg::Hit {
                        victim,
                        weapon: bullet.weapon,
                        headshot,
                    });
                    player.hit_marker = HIT_MARKER_TIME;
                    if headshot {
                        player.headshot_marker = HIT_MARKER_TIME;
                    }
                    sounds.write(SoundCue {
                        sound: if headshot { Sound::Headshot } else { Sound::Hit },
                        at: None,
                    });
                }
            }
            if stopped {
                commands.entity(entity).despawn();
                continue;
            }
        }
        bullet.pos += step;
        bullet.vel.y -= bullet.gravity * dt;
        bullet.age += dt;
        if bullet.age > BULLET_LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }

        // Drawn as a streak trailing behind the bullet, never longer than the
        // distance it has covered. Each end is drawn off its real path by
        // what is left of the offset when the bullet was there, so the
        // streak runs out of the muzzle and closes onto the real path.
        let speed = bullet.vel.length();
        let dir = bullet.vel / speed.max(1e-3);
        let length = (speed * 0.018).clamp(0.5, 5.0).min(speed * bullet.age);
        let drawn_off = |age: f32| bullet.offset * (1.0 - age / BULLET_CONVERGE).clamp(0.0, 1.0);
        let head = bullet.pos + drawn_off(bullet.age);
        let tail = bullet.pos - dir * length + drawn_off(bullet.age - length / speed.max(1e-3));
        let along = head - tail;
        let drawn_length = along.length();
        *transform = Transform {
            translation: (head + tail) / 2.0,
            rotation: Quat::from_rotation_arc(Vec3::NEG_Z, along.try_normalize().unwrap_or(dir)),
            scale: Vec3::new(BULLET_WIDTH, BULLET_WIDTH, drawn_length),
        };
    }
}

fn handle_net(
    mut commands: Commands,
    mut messages: MessageReader<NetMsg>,
    session: Res<Session>,
    map: Res<GameMap>,
    assets: Option<Res<SharedAssets>>,
    mut feed: ResMut<KillFeed>,
    mut notices: ResMut<Notices>,
    mut player: ResMut<LocalPlayer>,
    mut sounds: MessageWriter<SoundCue>,
) {
    for NetMsg(msg) in messages.read() {
        match msg {
            ServerMsg::Fire {
                shooter,
                weapon: weapon_id,
                origin,
                dirs,
            } => {
                let origin = Vec3::from(*origin);
                sounds.write(SoundCue {
                    sound: if *weapon_id == KNIFE {
                        Sound::Throw
                    } else {
                        Sound::Gunshot(*weapon_id)
                    },
                    at: Some(origin),
                });
                if let Some(assets) = &assets {
                    // Their gun is roughly this far below their eyes.
                    let offset = Vec3::Y * -0.25;
                    for dir in dirs {
                        let dir = Vec3::from(*dir).normalize_or_zero();
                        spawn_bullet(&mut commands, assets, *weapon_id, *shooter, origin, dir, offset, false);
                    }
                }
            }
            // Armor breaking is loud on both ends: ours shatters with a heavy
            // crunch, the one we break with a bright crack.
            ServerMsg::Hit {
                victim,
                attacker,
                armor_broken: true,
                ..
            } => {
                let sound = if *victim == session.my_id {
                    Sound::ArmorBroken
                } else if *attacker == session.my_id {
                    Sound::ArmorBreak
                } else {
                    continue;
                };
                sounds.write(SoundCue { sound, at: None });
            }
            ServerMsg::ArmorPickup { id, spot, .. } => {
                let at = if *id == session.my_id {
                    None
                } else {
                    let Some(spot) = map.0.armor_spots.get(*spot as usize) else {
                        continue;
                    };
                    Some(*spot)
                };
                sounds.write(SoundCue {
                    sound: Sound::ArmorPickup,
                    at,
                });
            }
            ServerMsg::Kill {
                killer,
                victim,
                weapon: weapon_id,
            } => {
                let me = session.my_id;
                let (killer_name, victim_name) = (session.name_of(*killer), session.name_of(*victim));
                let ended_streak = notices.streaks.remove(victim).unwrap_or(0) >= STREAKS[0].0;
                if killer == victim {
                    feed.push(format!("{victim_name} blew up"));
                    continue;
                }

                // Killing whoever last killed you is revenge.
                let revenge = notices.grudges.remove(&(*victim, *killer));
                notices.grudges.insert((*killer, *victim));
                let streak = notices.streaks.entry(*killer).or_insert(0);
                *streak += 1;
                let streak = *streak;
                let title = STREAKS.iter().find(|(kills, _)| *kills == streak).map(|(_, title)| *title);

                let revenge_tag = if revenge { "  (revenge)" } else { "" };
                feed.push(format!(
                    "{killer_name}  [{}]  {victim_name}{revenge_tag}",
                    weapon_name(*weapon_id)
                ));

                if *killer == me {
                    // Our own kills are announced in the middle of the screen.
                    player.kill_marker = 1.0;
                    sounds.write(SoundCue {
                        sound: Sound::Kill,
                        at: None,
                    });
                    notices.kill = Some((format!("FRAGGED {victim_name}"), 2.5));
                    let mut banner = Vec::new();
                    if revenge {
                        banner.push("REVENGE!".to_string());
                    }
                    if let Some(title) = title {
                        banner.push(format!("{}  -  {streak} in a row", title.to_uppercase()));
                    }
                    if ended_streak {
                        banner.push(format!("ended {victim_name}'s streak"));
                    }
                    if !banner.is_empty() {
                        notices.banner = Some((banner.join("     "), 3.5));
                    }
                } else {
                    // Everybody else's go in the feed.
                    if let Some(title) = title {
                        feed.push(format!("{killer_name} is on a {}!", title.to_lowercase()));
                    }
                    if ended_streak {
                        feed.push(format!("{killer_name} ended {victim_name}'s streak"));
                    }
                    if *victim == me && revenge {
                        notices.banner = Some((format!("{killer_name} got revenge"), 3.5));
                    }
                }
            }
            _ => {}
        }
    }
}

fn age_feed(time: Res<Time>, mut feed: ResMut<KillFeed>, mut notices: ResMut<Notices>) {
    let dt = time.delta_secs();
    if !feed.0.is_empty() {
        for entry in &mut feed.0 {
            entry.1 -= dt;
        }
        feed.0.retain(|entry| entry.1 > 0.0);
    }
    if notices.kill.is_some() || notices.banner.is_some() {
        let notices = &mut *notices;
        for notice in [&mut notices.kill, &mut notices.banner] {
            if let Some((_, left)) = notice {
                *left -= dt;
                if *left <= 0.0 {
                    *notice = None;
                }
            }
        }
    }
}
