//! The local player: input, movement, and the first-person camera.
//!
//! Movement is client-authoritative (we simulate ourselves and tell the server
//! where we are), which keeps it perfectly responsive at any ping. That is the
//! right trade-off for private lobbies among friends.

use std::f32::consts::{FRAC_PI_2, PI};

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, WindowFocused},
};

use crate::{
    AppState,
    audio::{BOLT_LOCK, Sound, SoundCue, surface_under},
    gun_model::{Fitted, FittedModels, GunHolder, GunPart, PartKind},
    loadout::DeathLoadoutOpen,
    map::{self, CROUCH_EYE, Map, PLAYER_HALF, STAND_EYE},
    net::{Connection, NetMsg, Session},
    protocol::*,
    remote::RemotePlayer,
    settings::{Action, ActionInput, Settings, SettingsMenu},
    weapons::{
        Cycle, DRAW_TIME, IRON_SIGHTS, KNIFE, PISTOL, RIFLE, SightId, Weapon, WeaponId, sight, sight_on, weapon,
    },
    world::{GameMap, MainCamera, ViewModel, ViewModelPart},
};

// --- Movement tuning (metres, seconds). Tweak these to change the feel. ---
const RUN_SPEED: f32 = 9.5;
const GROUND_ACCEL: f32 = 10.0;
const FRICTION: f32 = 6.0;
const STOP_SPEED: f32 = 3.0;
/// Quake-style air strafing: in the air you can only add up to this much speed
/// in the wished direction per step (see `TICK`), which is what makes strafe-jumping gain
/// speed. How fast it is added is the host's air acceleration
/// (`Session::air_accel`), times running speed.
const AIR_WISH_SPEED: f32 = 1.0;
/// Extra air control (m/s²) that only applies below running speed.
const AIR_CONTROL_ACCEL: f32 = 25.0;
pub const GRAVITY: f32 = 22.0;
/// Seconds in the air after which coming down counts as a landing (and is
/// heard), rather than stepping off a kerb.
pub const LANDING_AIR_TIME: f32 = 0.25;
const JUMP_SPEED: f32 = 8.0;
const AIR_JUMPS: u32 = 1;
/// Bunny hopping takes a jump pressed close to landing, not jump held down.
/// Pressed up to this long before touching down (with no air jump left, or
/// the ground right below), the jump goes off on landing.
const BHOP_EARLY: f32 = 0.1;
/// For this long after a landing, speed above running speed is kept rather
/// than lost to friction, so a jump pressed just after landing still carries it.
const BHOP_LATE: f32 = 0.1;
/// A dash brings your speed up to this (it never slows you).
const DASH_SPEED: f32 = 13.0;
/// A dash from the ground hops you up: half the jump's speed, a quarter of its height.
const DASH_LIFT: f32 = JUMP_SPEED * 0.5;
pub const DASH_COOLDOWN: f32 = 1.5;
const MAX_SPEED: f32 = 45.0;
const CROUCH_SPEED: f32 = 4.5;
/// Walking speed while aiming down sights.
const AIM_SPEED: f32 = 6.0;
/// Crouching on the ground at this speed or more starts a slide.
const SLIDE_MIN_SPEED: f32 = 7.0;
/// A slide tops your speed up to this, if `SLIDE_BOOST_COOLDOWN` has passed.
const SLIDE_BOOST_SPEED: f32 = 15.0;
const SLIDE_BOOST_COOLDOWN: f32 = 2.0;
/// Speed lost per second while sliding (m/s²). Running friction is far higher.
const SLIDE_DECEL: f32 = 6.0;
/// Below this speed a slide turns into an ordinary crouch.
const SLIDE_END_SPEED: f32 = 5.0;

/// Looking straight up or down stops just short of vertical.
const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
/// Recoil: the view holds where shots threw it for this long after the last
/// one, then drifts back at this rate (fraction per second, roughly).
const KICK_HOLD: f32 = 0.14;
const KICK_RECOVERY: f32 = 9.0;

/// Field of view of the menu camera, in radians.
pub const BASE_FOV: f32 = 1.3;
/// Which way from the eye a gun held from the hip lies: off to the right and
/// low, per metre forward.
const HIP_DIRECTION: Vec3 = Vec3::new(0.45, -0.4, -1.0);
/// How far in front of the eye a gun's back end is, held from the hip.
const HIP_BACK: f32 = 0.16;
/// Guns shorter than this (a pistol, the SMG) are held out as far as one this
/// long: in the hands, rather than tucked in against the body.
const HIP_SHORTEST: f32 = 0.26;

/// Where the middle of a gun `length` long is held from the hip. A longer gun
/// reaches further forward from the same hands, and each one sits in the same
/// part of the screen.
pub fn hip_position(length: f32) -> Vec3 {
    HIP_DIRECTION * (HIP_BACK + length.max(HIP_SHORTEST) / 2.0)
}
/// Seconds it takes to work the pump after loading an empty shotgun.
pub const RACK_TIME: f32 = 0.4;
/// After each shot the pump is worked, starting this many seconds after it
/// and taking this long.
pub const PUMP_AFTER: f32 = 0.12;
pub const PUMP_TIME: f32 = 0.45;
/// After each shot a bolt is lifted and drawn back this many seconds after
/// it. It goes home again in time for the next one (see `audio::BOLT_LOCK`).
pub const BOLT_BACK: f32 = 0.4;
/// Working a bolt action, in seconds (`Sound::BoltBack` and `Sound::BoltHome`
/// keep to them): the handle is lifted, then the bolt drawn back; it is
/// pushed home, then the handle turned down to lock it, by `BOLT_LOCK`.
pub const BOLT_LIFT_TIME: f32 = 0.05;
pub const BOLT_DRAW_TIME: f32 = 0.11;
pub const BOLT_PUSH_TIME: f32 = 0.075;
/// How far a bolt action's handle turns up about the bore to unlock it
/// (radians), and how far its bolt is drawn back, times the gun's length.
const BOLT_LIFT: f32 = 1.1;
const BOLT_ACTION_TRAVEL: f32 = 0.085;
/// How far through a magazine reload: the magazine is let go and slides
/// out; it has dropped away (and falls to the floor); the new one latches;
/// the slide or bolt is taken hold of and drawn back (cocked), slowly, all
/// the way by `BOLT_PULLED`; it is let go and goes home. The magazine is
/// changed quickly to leave the cocking time.
pub const MAGAZINE_OUT: f32 = 0.1;
pub const MAGAZINE_DROPPED: f32 = 0.24;
pub const MAGAZINE_IN: f32 = 0.5;
pub const BOLT_PULL: f32 = 0.58;
pub const BOLT_PULLED: f32 = 0.8;
pub const BOLT_HOME: f32 = 0.84;
/// How far back a slide or bolt goes when cocked, times the gun's length.
/// A pistol's slide travels further for its size than a long gun's bolt.
const SLIDE_TRAVEL: f32 = 0.25;
const BOLT_TRAVEL: f32 = 0.14;
/// How far back each shot kicks a slide or bolt, times the gun's length.
const SHOT_KICK: f32 = 0.14;
/// How far back an empty gun's slide or bolt is held by its catch, of the
/// way it travels: a little short of where cocking draws it.
const BOLT_CAUGHT: f32 = 0.9;
/// 0 -> 1 as the magazine comes out, 1 -> 0 as the new one goes in, over a
/// magazine reload going from 0 to 1.
pub fn magazine_out(progress: f32) -> f32 {
    smooth((progress - 0.08) / 0.16) * smooth((0.52 - progress) / 0.16)
}
/// 0 -> 1 as the slide or bolt is drawn back to cock the gun, over a
/// magazine reload going from 0 to 1; 0 again once it is let go.
pub fn cocking(progress: f32) -> f32 {
    if progress >= BOLT_HOME { 0.0 } else { smooth((progress - BOLT_PULL) / (BOLT_PULLED - BOLT_PULL)) }
}

/// A bolt action worked after a shot, `since` seconds after it, with the next
/// shot ready `interval` after it: how far its handle is lifted and its bolt
/// drawn back, each 0 -> 1. It stays open between the two.
pub fn bolt_cycle(since: f32, interval: f32) -> (f32, f32) {
    let home = since - (interval - BOLT_LOCK);
    if home >= 0.0 {
        return bolt_home(home);
    }
    let lifted = since - BOLT_BACK;
    (smooth(lifted / BOLT_LIFT_TIME), smooth((lifted - BOLT_LIFT_TIME) / BOLT_DRAW_TIME))
}

/// A bolt action cocked at the end of a magazine reload `progress` through,
/// taking `reload` seconds: lifted and drawn back slowly from `BOLT_PULL` to
/// `BOLT_PULLED`, pushed home and locked from `BOLT_HOME`. As `bolt_cycle`.
pub fn bolt_cocking(progress: f32, reload: f32) -> (f32, f32) {
    if progress >= BOLT_HOME {
        return bolt_home((progress - BOLT_HOME) * reload);
    }
    let lift = BOLT_LIFT_TIME / reload;
    (
        smooth((progress - BOLT_PULL) / lift),
        smooth((progress - BOLT_PULL - lift) / (BOLT_PULLED - BOLT_PULL - lift)),
    )
}

/// A bolt `home` seconds after it starts going home: pushed, then locked.
fn bolt_home(home: f32) -> (f32, f32) {
    (
        1.0 - smooth((home - BOLT_PUSH_TIME) / (BOLT_LOCK - BOLT_PUSH_TIME)),
        1.0 - smooth(home / BOLT_PUSH_TIME),
    )
}

/// Eases 0 -> 1 in and out; clamped.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Seconds a dropped magazine lies on the floor before it is gone.
const DROPPED_MAGAZINE_TIME: f32 = 4.0;
/// `LocalPlayer::slot` when the knife is out (0 and 1 are the gun slots).
pub const KNIFE_SLOT: usize = 2;

/// One of the two guns carried.
#[derive(Clone, Copy)]
pub struct GunSlot {
    pub weapon: WeaponId,
    /// The sight on it (see `weapons::sight_on`).
    pub sight: SightId,
    pub ammo: u32,
    /// Seconds until it can fire again.
    pub cooldown: f32,
}

impl GunSlot {
    fn full(weapon_id: WeaponId, sight: SightId) -> GunSlot {
        GunSlot {
            weapon: weapon_id,
            sight,
            ammo: weapon(weapon_id).magazine,
            cooldown: 0.0,
        }
    }
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalPlayer>()
            .init_resource::<Controls>()
            .add_systems(OnEnter(AppState::Playing), capture_mouse)
            .add_systems(OnExit(AppState::Playing), hide_view_model)
            .add_systems(
                Update,
                (
                    handle_net.run_if(resource_exists::<Connection>),
                    (toggle_pause, look, movement)
                        .chain()
                        .in_set(PlayerSystems::Simulate)
                        .run_if(in_state(AppState::Playing)),
                    while_dead.after(toggle_pause).run_if(in_state(AppState::Playing)),
                    (follow_camera, drop_magazine)
                        .chain()
                        .in_set(PlayerSystems::Camera)
                        .run_if(in_state(AppState::Playing)),
                    fall_magazines.run_if(in_state(AppState::Playing)),
                    orbit_camera.run_if(not(in_state(AppState::Playing))),
                    grab_cursor,
                ),
            )
            .configure_sets(
                Update,
                (PlayerSystems::Simulate, PlayerSystems::Camera).chain(),
            )
            .add_systems(
                FixedUpdate,
                send_state
                    .run_if(in_state(AppState::Playing))
                    .run_if(resource_exists::<Connection>),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayerSystems {
    /// Input and movement.
    Simulate,
    /// The camera is moved to the player's eyes.
    Camera,
}

#[derive(Resource)]
pub struct LocalPlayer {
    /// Centre of the collision box.
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
    /// Seconds since we were last on the ground.
    air_time: f32,
    /// Counts up each time we land after `LANDING_AIR_TIME` in the air.
    /// Bunny hopping is off the ground again the next step, before a frame
    /// ends, so this is how the landings are noticed.
    pub landings: u32,
    pub air_jumps: u32,
    pub dash_cooldown: f32,
    pub alive: bool,
    pub health: i32,
    pub armor: i32,
    pub spawn_seq: u32,
    pub guns: [GunSlot; 2],
    /// What is in hand: a gun slot (0 or 1) or `KNIFE_SLOT`.
    pub slot: usize,
    /// The gun slot held before the knife came out, where switching goes back to.
    pub last_gun: usize,
    /// Seconds until the weapon just switched to is up and usable.
    pub draw: f32,
    pub knife_cooldown: f32,
    /// 1 right after a shot or stab, decays to 0. Drives the gun kick.
    pub recoil: f32,
    /// How far shots have thrown the view (pitch, yaw) that has not yet
    /// drifted back. Pulling the mouse against it pays it off early.
    pub view_kick: Vec2,
    /// Seconds after the last shot before the view starts drifting back.
    pub kick_hold: f32,
    pub killed_by: Option<String>,
    /// What it was done with (`weapons::GRENADE` for a grenade).
    pub killed_with: Option<WeaponId>,
    /// Seconds since we died.
    pub dead_for: f32,
    /// We asked the server to respawn us early.
    pub respawn_asked: bool,
    /// Dead and watching a teammate: the eyes we see through. Our view and
    /// gun are theirs meanwhile (see `see_through`).
    pub pov: Option<Vec3>,
    /// Seconds the hit marker stays visible.
    pub hit_marker: f32,
    /// Seconds the hit marker stays yellow, for a headshot.
    pub headshot_marker: f32,
    /// 1 right after a kill, decays to 0. Turns the hit marker red.
    pub kill_marker: f32,
    /// 1 right after taking damage, decays to 0.
    pub damage_flash: f32,
    /// Where recent hits on us came from, shown around the crosshair.
    pub damage_arcs: Vec<DamageArc>,
    /// Crouching or sliding: the collision box is the short one.
    pub crouching: bool,
    pub sliding: bool,
    slide_boost_cooldown: f32,
    /// Eye height above the feet, eased between standing and crouching.
    view_height: f32,
    /// Seconds of frame time not yet stepped through (less than `TICK`).
    tick_time: f32,
    /// Feet before the last movement step, and the feet drawn this frame.
    last_feet: Vec3,
    view_feet: Vec3,
    /// Jump or dash pressed since the last movement step.
    jump_queued: bool,
    dash_queued: bool,
    /// Seconds a jump pressed in the air waits for the ground (`BHOP_EARLY`).
    jump_buffer: f32,
    /// Seconds left of the landing's grace from friction (`BHOP_LATE`).
    bhop_grace: f32,
    /// The yaw at the end of the last frame's movement.
    frame_yaw: f32,
    /// 0 = hip, 1 = fully aimed down sights.
    pub aim: f32,
    /// Seconds the gun in hand has been held aimed since the last shot, for
    /// a gun with a choke (see `Weapon::choke`).
    pub choked_for: f32,
    /// Seconds until the reload (or, loading round by round, the round being
    /// loaded, or the rack after it) is done; 0 when not reloading.
    pub reload: f32,
    /// The reload started with the gun empty: it ends by working the bolt,
    /// slide or pump.
    pub reload_empty: bool,
    /// Loading round by round: fire was pressed, so it stops after this round.
    pub reload_stop: bool,
    /// Loading round by round: the last step, working the pump.
    pub racking: bool,
    /// 0 -> 1 as the gun is tipped to load it round by round, eased.
    reload_pose: f32,
    /// The kind of grenade carried this life (picked in the loadout).
    pub grenade: GrenadeKind,
    /// How many of it are carried (whole ones can be thrown; the fraction is
    /// the recharge).
    pub grenades: f32,
    /// The grenade in hand, ready to throw instead of the weapon.
    pub hand: Option<InHand>,
    /// 1 right after throwing a grenade, decays to 0. Drives the gun dip.
    pub throw_anim: f32,
    /// 0 -> 1 as a cooked grenade is raised and the gun lowered.
    pub hold: f32,
    /// Seconds of flashbang blindness left.
    pub blind: f32,
}

impl Default for LocalPlayer {
    fn default() -> Self {
        Self {
            pos: Vec3::new(0.0, 10.0, 0.0),
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            grounded: false,
            air_time: 0.0,
            landings: 0,
            air_jumps: AIR_JUMPS,
            dash_cooldown: 0.0,
            alive: false,
            health: 0,
            armor: 0,
            spawn_seq: 0,
            guns: [GunSlot::full(RIFLE, IRON_SIGHTS), GunSlot::full(PISTOL, IRON_SIGHTS)],
            slot: 0,
            last_gun: 0,
            draw: 0.0,
            knife_cooldown: 0.0,
            recoil: 0.0,
            view_kick: Vec2::ZERO,
            kick_hold: 0.0,
            killed_by: None,
            killed_with: None,
            dead_for: 0.0,
            respawn_asked: false,
            pov: None,
            hit_marker: 0.0,
            headshot_marker: 0.0,
            kill_marker: 0.0,
            damage_flash: 0.0,
            damage_arcs: Vec::new(),
            crouching: false,
            sliding: false,
            slide_boost_cooldown: 0.0,
            view_height: STAND_EYE,
            tick_time: 0.0,
            last_feet: Vec3::new(0.0, 10.0, 0.0) - PLAYER_HALF.y * Vec3::Y,
            view_feet: Vec3::new(0.0, 10.0, 0.0) - PLAYER_HALF.y * Vec3::Y,
            jump_queued: false,
            dash_queued: false,
            jump_buffer: 0.0,
            bhop_grace: 0.0,
            frame_yaw: 0.0,
            aim: 0.0,
            choked_for: 0.0,
            reload: 0.0,
            reload_empty: false,
            reload_stop: false,
            racking: false,
            reload_pose: 0.0,
            grenade: FRAG,
            grenades: grenade(FRAG).max,
            hand: None,
            throw_anim: 0.0,
            hold: 0.0,
            blind: 0.0,
        }
    }
}

/// Seconds an arc showing where a hit came from takes to fade.
pub const DAMAGE_ARC_TIME: f32 = 1.0;
/// A frag that went off this recently near us is what just hurt us.
const BLAST_WINDOW: f64 = 0.2;

/// Where a hit on us came from. It stays pointing at that spot as we turn.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct DamageArc {
    /// More hits from the same player (or grenade) move the same arc.
    pub from: DamageSource,
    pub at: Vec3,
    /// Seconds before it has faded out.
    pub left: f32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DamageSource {
    Player(PlayerId),
    /// The id of the grenade that went off.
    Blast(u32),
}

impl LocalPlayer {
    /// Shows where a hit came from: a new arc, or the one already pointing at
    /// the same attacker, turned to where they are now.
    fn hurt_from(&mut self, from: DamageSource, at: Vec3) {
        let arc = DamageArc {
            from,
            at,
            left: DAMAGE_ARC_TIME,
        };
        match self.damage_arcs.iter_mut().find(|arc| arc.from == from) {
            Some(old) => *old = arc,
            None => self.damage_arcs.push(arc),
        }
    }

    /// Which way a spot lies from us, in radians clockwise from straight ahead.
    pub fn bearing(&self, at: Vec3) -> f32 {
        let to = at - self.pos;
        let forward = map::look_dir(self.yaw, 0.0);
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        to.dot(right).atan2(to.dot(forward))
    }
}

/// A grenade in hand.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct InHand {
    pub kind: GrenadeKind,
    /// A frag's fuse once its pin is pulled: seconds left before it goes off.
    pub fuse: Option<f32>,
}

impl LocalPlayer {
    /// Half extents of the collision box right now.
    pub fn half(&self) -> Vec3 {
        map::player_half(self.crouching)
    }

    /// Puts the player at `pos`, standing still, with the view there too
    /// rather than gliding over from where it was.
    pub fn teleport(&mut self, pos: Vec3) {
        self.pos = pos;
        self.vel = Vec3::ZERO;
        self.last_feet = self.feet();
        self.view_feet = self.last_feet;
    }

    pub fn feet(&self) -> Vec3 {
        self.pos - Vec3::Y * self.half().y
    }

    /// Where the eyes are drawn: between the last two movement steps (see
    /// `TICK`), a few milliseconds behind `pos`.
    pub fn eye(&self) -> Vec3 {
        self.view_feet + Vec3::Y * self.view_height
    }

    pub fn reloading(&self) -> bool {
        self.reload > 0.0
    }

    /// How long the current reload step takes: the whole reload, one round,
    /// or the rack after the last round.
    pub fn reload_step(&self) -> f32 {
        if self.racking { RACK_TIME } else { self.weapon().reload.max(0.01) }
    }

    /// 0 -> 1 through the current reload step (see `reload_step`).
    pub fn reload_progress(&self) -> f32 {
        if self.reloading() { (1.0 - self.reload / self.reload_step()).clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The weapon in hand.
    pub fn weapon_id(&self) -> WeaponId {
        self.guns.get(self.slot).map_or(KNIFE, |gun| gun.weapon)
    }

    /// The sight on the weapon in hand.
    pub fn sight_id(&self) -> SightId {
        self.guns.get(self.slot).map_or(IRON_SIGHTS, |gun| sight_on(gun.weapon, gun.sight))
    }

    /// Field of view factor when fully aimed: the sight's, whatever the gun.
    pub fn zoom(&self) -> f32 {
        sight(self.sight_id()).zoom
    }

    /// 0 -> 1 as the view through a scope fades in, near the end of aiming.
    /// Always 0 with other sights.
    pub fn scope_view(&self) -> f32 {
        if (self.alive || self.pov.is_some()) && sight(self.sight_id()).overlay {
            ((self.aim - 0.8) / 0.15).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn weapon(&self) -> &'static Weapon {
        weapon(self.weapon_id())
    }

    /// 0 -> 1 as a gun with a choke closes up (see `Weapon::choke`).
    pub fn choke(&self) -> f32 {
        self.weapon().choke.as_ref().map_or(0.0, |choke| (self.choked_for / choke.time).min(1.0))
    }

    /// Half angle (radians) of the cone the next shot lands in.
    pub fn spread(&self) -> f32 {
        let stats = self.weapon();
        let aimed = match &stats.choke {
            Some(choke) => stats.aim_spread + (choke.spread - stats.aim_spread) * self.choke(),
            None => stats.aim_spread,
        };
        stats.hip_spread + (aimed - stats.hip_spread) * self.aim
    }

    pub fn horizontal_speed(&self) -> f32 {
        self.vel.with_y(0.0).length()
    }

    /// Throws the view by `kick` (pitch, yaw) for a shot just fired.
    pub fn kick_view(&mut self, kick: Vec2) {
        let pitch = (self.pitch + kick.x).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.view_kick += Vec2::new(pitch - self.pitch, kick.y);
        self.pitch = pitch;
        self.yaw += kick.y;
        self.kick_hold = KICK_HOLD;
    }
}

/// Mouse movement against the recoil counts towards bringing the view back.
fn pay_off(owed: &mut f32, moved: f32) {
    if *owed * moved < 0.0 {
        *owed = owed.signum() * (owed.abs() - moved.abs()).max(0.0);
    }
}

#[derive(Resource, Default)]
pub struct Controls {
    /// True while the mouse is grabbed and input drives the player. False = paused.
    pub captured: bool,
}

fn capture_mouse(mut controls: ResMut<Controls>) {
    controls.captured = true;
}

fn hide_view_model(mut view_model: Single<&mut Visibility, With<ViewModel>>) {
    **view_model = Visibility::Hidden;
}

fn handle_net(
    mut messages: MessageReader<NetMsg>,
    time: Res<Time<Real>>,
    session: Res<Session>,
    settings: Res<Settings>,
    mut player: ResMut<LocalPlayer>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    // Frags that went off near us lately: (grenade id, where, when).
    mut blasts: Local<Vec<(u32, Vec3, f64)>>,
) {
    let now = time.elapsed_secs_f64();
    blasts.retain(|(.., when)| now - when < BLAST_WINDOW);
    for NetMsg(msg) in messages.read() {
        match msg {
            // The server says nothing about grenades in a hit, but sends the
            // explosion just before the hits it causes.
            ServerMsg::Explosion { id, kind: FRAG, pos } => {
                let pos = Vec3::from(*pos);
                if pos.distance(player.pos) < GRENADE_RADIUS + PLAYER_HALF.y {
                    blasts.push((*id, pos, now));
                }
            }
            ServerMsg::MatchStarted { .. } => *player = LocalPlayer::default(),
            ServerMsg::Spawn {
                id,
                pos,
                yaw,
                spawn_seq,
                grenade: kind,
            } if *id == session.my_id => {
                *player = LocalPlayer {
                    yaw: *yaw,
                    frame_yaw: *yaw,
                    alive: true,
                    health: MAX_HEALTH,
                    armor: START_ARMOR,
                    spawn_seq: *spawn_seq,
                    // Every life starts with the loadout picked in the menus.
                    guns: [settings.primary, settings.secondary].map(|gun| GunSlot::full(gun, settings.sight_for(gun))),
                    // The server says which grenade: the one in the loadout when it spawned us.
                    grenade: *kind,
                    grenades: grenade(*kind).max,
                    ..default()
                };
                player.teleport(Vec3::from(*pos));
            }
            ServerMsg::Hit {
                victim,
                attacker,
                health,
                armor,
                ..
            } if *victim == session.my_id => {
                player.health = *health;
                player.armor = *armor;
                player.damage_flash = 1.0;
                let attacker_at = || {
                    remotes
                        .iter()
                        .find(|(remote, _)| remote.id == *attacker)
                        .map(|(_, transform)| transform.translation)
                };
                if let Some(&(id, pos, _)) = blasts.last() {
                    player.hurt_from(DamageSource::Blast(id), pos);
                } else if *attacker != session.my_id
                    && let Some(at) = attacker_at()
                {
                    player.hurt_from(DamageSource::Player(*attacker), at);
                }
            }
            ServerMsg::ArmorPickup { id, armor, .. } if *id == session.my_id => player.armor = *armor,
            // Grenade damage is decided by the server, so this is where we learn of it.
            ServerMsg::Hit { attacker, .. } if *attacker == session.my_id => player.hit_marker = 0.15,
            ServerMsg::Kill { killer, victim, weapon } if *victim == session.my_id => {
                player.alive = false;
                player.health = 0;
                player.armor = 0;
                player.killed_by = Some(session.name_of(*killer).to_string());
                player.killed_with = Some(*weapon);
                player.dead_for = 0.0;
                player.respawn_asked = false;
                // A frag being cooked drops (the server throws it down for us).
                player.hand = None;
            }
            _ => {}
        }
    }
}

/// Puts us back in now rather than at the end of `RESPAWN_DELAY`, if we have
/// been dead long enough (see `MIN_RESPAWN_DELAY`) and the mode allows it.
pub fn ask_respawn(player: &mut LocalPlayer, connection: &mut Connection, mode: u8) {
    if early_respawn(mode)
        && !player.alive
        && player.killed_by.is_some()
        && !player.respawn_asked
        && player.dead_for >= MIN_RESPAWN_DELAY
    {
        player.respawn_asked = true;
        connection.send(&ClientMsg::Respawn);
    }
}

impl LocalPlayer {
    /// Showing the death screen: dead, and not yet put back in.
    pub fn on_death_screen(&self) -> bool {
        !self.alive && self.killed_by.is_some()
    }

    /// Dead, we see through a teammate's eyes: the view turns with theirs and
    /// the gun in hand is theirs, aimed, reloaded and fired as they do. Nothing
    /// of it outlasts the death: a spawn starts us afresh.
    #[allow(clippy::too_many_arguments)]
    pub fn see_through(
        &mut self,
        eye: Vec3,
        yaw: f32,
        pitch: f32,
        weapon_id: WeaponId,
        sight: SightId,
        aim: f32,
        reload_for: Option<f32>,
    ) {
        self.pov = Some(eye);
        self.yaw = yaw;
        self.pitch = pitch;
        self.view_kick = Vec2::ZERO;
        self.kick_hold = 0.0;
        self.vel = Vec3::ZERO;
        self.hand = None;
        self.draw = 0.0;
        if weapon_id == KNIFE {
            self.slot = KNIFE_SLOT;
        } else {
            self.slot = 0;
            if self.guns[0].weapon != weapon_id || self.guns[0].sight != sight {
                self.guns[0] = GunSlot::full(weapon_id, sight);
            }
        }
        self.aim = if weapon_id == KNIFE { 0.0 } else { aim };
        // We are only told they are reloading: timed from when that started,
        // round after round for a gun loaded that way.
        let step = weapon(weapon_id).reload.max(0.01);
        self.racking = false;
        self.reload_empty = false;
        self.reload = match reload_for {
            Some(time) if weapon(weapon_id).reload_per_round => step - time % step,
            Some(time) => (step - time).max(0.001),
            None => 0.0,
        };
    }

    /// The teammate we see through just fired.
    pub fn watched_fired(&mut self) {
        self.recoil = 1.0;
        if let Some(gun) = self.guns.get_mut(self.slot) {
            gun.cooldown = weapon(gun.weapon).interval;
        }
    }

    /// Back to our own eyes, where we died, looking the way we were.
    pub fn stop_seeing_through(&mut self, yaw: f32, pitch: f32) {
        self.pov = None;
        self.yaw = yaw;
        self.pitch = pitch;
        self.aim = 0.0;
        self.reload = 0.0;
        self.recoil = 0.0;
    }
}

/// While dead the death screen shows (see `ui.rs`). The weapon and grenade
/// keys step through the loadout we will be handed when we respawn, L opens
/// and closes the whole of it, and jump puts us back in early.
#[allow(clippy::too_many_arguments)]
fn while_dead(
    time: Res<Time>,
    session: Res<Session>,
    keys: Res<ButtonInput<KeyCode>>,
    connection: Option<ResMut<Connection>>,
    mut player: ResMut<LocalPlayer>,
    mut loadout_open: ResMut<DeathLoadoutOpen>,
    mut input: ParamSet<(ActionInput, ResMut<Settings>)>,
) {
    // Before our first spawn there is no death screen.
    if !player.on_death_screen() {
        loadout_open.0 = false;
        return;
    }
    player.dead_for += time.delta_secs();
    let (primary, secondary, grenade, jump, captured) = {
        let input = input.p0();
        (
            input.just_pressed(Action::Primary),
            input.just_pressed(Action::Secondary),
            input.just_pressed(Action::Grenade),
            input.just_pressed(Action::Jump),
            input.live(),
        )
    };
    if captured && keys.just_pressed(KeyCode::KeyL) {
        loadout_open.0 = !loadout_open.0;
    }
    if jump && let Some(mut connection) = connection {
        ask_respawn(&mut player, &mut connection, session.mode);
    }
    if primary || secondary || grenade {
        let mut settings = input.p1();
        if primary {
            settings.cycle_gun(0);
        }
        if secondary {
            settings.cycle_gun(1);
        }
        if grenade {
            settings.cycle_grenade();
        }
        settings.save();
    }
}

/// Escape pauses and resumes; with the settings panel or the loadout screen
/// open it backs out of that instead.
fn toggle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    mut controls: ResMut<Controls>,
    mut menu: ResMut<SettingsMenu>,
    mut loadout: ResMut<crate::loadout::LoadoutScreen>,
    mut death_loadout: ResMut<DeathLoadoutOpen>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        if menu.open {
            menu.back();
        } else if loadout.open {
            loadout.open = false;
        } else if death_loadout.0 && controls.captured {
            death_loadout.0 = false;
        } else {
            controls.captured = !controls.captured;
        }
    }
}

fn look(
    mouse: Res<AccumulatedMouseMotion>,
    controls: Res<Controls>,
    settings: Res<Settings>,
    mut player: ResMut<LocalPlayer>,
) {
    if !controls.captured || !player.alive || mouse.delta == Vec2::ZERO {
        return;
    }
    // Aiming turns the view slower in step with the zoom, so it feels the same on screen.
    let aimed = player.zoom() * settings.ads_sensitivity;
    let sensitivity = settings.sensitivity * 0.001 * (1.0 + (aimed - 1.0) * player.aim);
    let p = &mut *player;
    let turn = -mouse.delta.x * sensitivity;
    let pitch = (p.pitch - mouse.delta.y * sensitivity).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    pay_off(&mut p.view_kick.x, pitch - p.pitch);
    pay_off(&mut p.view_kick.y, turn);
    p.yaw += turn;
    p.pitch = pitch;
}

/// Movement runs in fixed steps of this many seconds, whatever the frame
/// rate, so every player moves the same way: air strafing in particular
/// gains speed per step, and a varying step would hand faster machines more.
pub const TICK: f32 = 1.0 / 128.0;
/// At most this much time is caught up in one frame; below 10 fps the game
/// runs slow rather than stepping a long way at once.
const MAX_CATCH_UP: f32 = 0.1;
/// Feet that move further than this in one step were teleported (a spawn, a
/// respawn): the view jumps there instead of gliding across.
const SNAP_DISTANCE: f32 = 2.0;

/// The movement keys held this frame, for every step run in it.
struct Held {
    /// The wished direction relative to the view: x right, y forward.
    keys: Vec2,
    /// The view's turn over this frame. Each step takes the yaw for its
    /// moment in the frame, so a turn steers every step of a long frame
    /// rather than the first one only.
    yaw_from: f32,
    yaw_to: f32,
    crouch: bool,
    aim: bool,
}

fn movement(
    time: Res<Time>,
    input: ActionInput,
    map: Res<GameMap>,
    session: Res<Session>,
    mut player: ResMut<LocalPlayer>,
) {
    let p = &mut *player;
    // A press is kept until a step uses it, so it is never lost on a frame
    // with no step nor used twice on a frame with several.
    p.jump_queued |= input.just_pressed(Action::Jump);
    p.dash_queued |= input.just_pressed(Action::Dash);

    let axis = |plus: Action, minus: Action| input.pressed(plus) as u8 as f32 - input.pressed(minus) as u8 as f32;
    let held = Held {
        keys: Vec2::new(axis(Action::Right, Action::Left), axis(Action::Forward, Action::Back)),
        yaw_from: p.frame_yaw,
        yaw_to: p.yaw,
        crouch: input.pressed(Action::Crouch),
        aim: input.pressed(Action::Aim),
    };
    advance(p, time.delta_secs(), &held, &map.0, session.air_accel(), session.frozen());
    p.frame_yaw = p.yaw;
}

/// Runs as many movement steps as `frame` seconds hold, keeping the rest for
/// the next frame, then places the view between the last two steps so it
/// moves smoothly at any frame rate.
fn advance(p: &mut LocalPlayer, frame: f32, held: &Held, map: &Map, air_accel: f32, frozen: bool) {
    let before = p.tick_time;
    p.tick_time = (p.tick_time + frame).min(MAX_CATCH_UP);
    let turn = held.yaw_to - held.yaw_from;
    // A big jump is a respawn facing somewhere new, not a turn.
    let turn = if turn.abs() < PI { turn } else { 0.0 };
    let mut into_frame = -before;
    while p.tick_time >= TICK {
        p.tick_time -= TICK;
        into_frame += TICK;
        let yaw = held.yaw_to - turn * (1.0 - into_frame / frame.max(1e-6)).clamp(0.0, 1.0);
        // Wished direction on the ground plane.
        let forward = map::look_dir(yaw, 0.0);
        let right = Vec3::new(-forward.z, 0.0, forward.x);
        let wish = (right * held.keys.x + forward * held.keys.y).normalize_or_zero();
        p.last_feet = p.feet();
        step(p, held, wish, forward, map, air_accel, frozen);
        p.jump_queued = false;
        p.dash_queued = false;
    }
    let feet = p.feet();
    p.view_feet = if p.last_feet.distance_squared(feet) > SNAP_DISTANCE * SNAP_DISTANCE {
        feet
    } else {
        p.last_feet.lerp(feet, p.tick_time / TICK)
    };
}

/// One movement step of `TICK` seconds.
fn step(p: &mut LocalPlayer, held: &Held, wish: Vec3, forward: Vec3, map: &Map, air_accel: f32, frozen: bool) {
    let dt = TICK;
    p.dash_cooldown = (p.dash_cooldown - dt).max(0.0);
    p.slide_boost_cooldown = (p.slide_boost_cooldown - dt).max(0.0);
    p.jump_buffer = (p.jump_buffer - dt).max(0.0);
    p.bhop_grace = (p.bhop_grace - dt).max(0.0);
    // Held in place at the spawn until the countdown is over; looking around is fine.
    if !p.alive || frozen {
        return;
    }

    // Crouching shrinks the box towards the feet; standing up needs headroom.
    let crouch_drop = PLAYER_HALF.y - map::CROUCH_HALF.y;
    if held.crouch {
        if !p.crouching {
            p.crouching = true;
            p.pos.y -= crouch_drop;
            if p.grounded {
                start_slide(p);
            }
        }
    } else if p.crouching {
        let standing = p.pos + Vec3::Y * crouch_drop;
        if map.is_free(standing, PLAYER_HALF) {
            p.crouching = false;
            p.sliding = false;
            p.pos = standing;
        }
    }

    let aiming = held.aim && !p.reloading() && p.slot != KNIFE_SLOT && p.hand.is_none();
    p.aim += (aiming as u8 as f32 - p.aim) * (dt * 14.0).min(1.0);

    let top_speed = if p.crouching {
        CROUCH_SPEED
    } else if aiming {
        AIM_SPEED
    } else {
        RUN_SPEED * p.weapon().mobility
    };
    if p.grounded {
        p.air_jumps = AIR_JUMPS;
        // A jump pressed just before landing (buffered) or just after it hops
        // again before friction takes the speed: that is what lets you keep
        // (and build) speed by bunny hopping. Holding jump does not.
        if p.jump_queued || p.jump_buffer > 0.0 {
            p.jump_buffer = 0.0;
            p.bhop_grace = 0.0;
            p.vel.y = JUMP_SPEED;
            p.grounded = false;
            p.sliding = false;
            air_accelerate(&mut p.vel, wish, air_accel, dt);
        } else if p.sliding {
            // A slide keeps its direction and bleeds speed slowly.
            let speed = p.horizontal_speed();
            let slowed = (speed - SLIDE_DECEL * dt).max(0.0);
            if slowed < SLIDE_END_SPEED {
                p.sliding = false;
            } else {
                p.vel.x *= slowed / speed;
                p.vel.z *= slowed / speed;
            }
        } else if p.bhop_grace > 0.0 && p.horizontal_speed() > top_speed {
            // Just landed with speed to keep: coast, waiting for the jump.
        } else {
            p.bhop_grace = 0.0;
            apply_friction(&mut p.vel, dt);
            accelerate(&mut p.vel, wish, top_speed, GROUND_ACCEL * top_speed, dt);
        }
    } else {
        if p.jump_queued {
            // About to touch down: this press is a bunny hop, not the air jump.
            let fall = (-p.vel.y * BHOP_EARLY + 0.5 * GRAVITY * BHOP_EARLY * BHOP_EARLY).max(0.0);
            let landing = p.vel.y < 0.0 && !map.is_free(p.pos - Vec3::Y * fall, p.half());
            if p.air_jumps > 0 && !landing {
                p.air_jumps -= 1;
                p.vel.y = JUMP_SPEED;
            } else {
                p.jump_buffer = BHOP_EARLY;
            }
        }
        air_accelerate(&mut p.vel, wish, air_accel, dt);
    }

    // Dash: redirect all momentum where you are steering, with a minimum
    // speed, and off the ground a little hop.
    if p.dash_queued && p.dash_cooldown <= 0.0 {
        p.dash_cooldown = DASH_COOLDOWN;
        let dir = if wish == Vec3::ZERO { forward } else { wish };
        let speed = p.vel.with_y(0.0).length().max(DASH_SPEED);
        let lift = if p.grounded { DASH_LIFT } else { 0.0 };
        p.vel = Vec3::new(dir.x * speed, p.vel.y.max(lift), dir.z * speed);
        if p.grounded {
            p.grounded = false;
            p.sliding = false;
        }
    }

    p.vel.y -= GRAVITY * dt;
    let horizontal = p.vel.with_y(0.0).clamp_length_max(MAX_SPEED);
    p.vel = Vec3::new(horizontal.x, p.vel.y.max(-60.0), horizontal.z);

    let was_grounded = p.grounded;
    let half = p.half();
    p.grounded = map.move_and_slide(&mut p.pos, &mut p.vel, half, dt);
    // Down a slope or a stair: stay on the ground rather than skip off it.
    if was_grounded && !p.grounded && p.vel.y <= 0.0 {
        let drop = map::STEP_UP.max(p.horizontal_speed() * dt);
        if map.snap_to_ground(&mut p.pos, half, drop) {
            p.grounded = true;
            p.vel.y = 0.0;
        }
    }
    // Landing with crouch held carries the speed into a slide.
    if p.grounded && !was_grounded && p.crouching {
        start_slide(p);
    }
    if p.grounded {
        if !was_grounded && p.air_time > LANDING_AIR_TIME {
            p.landings = p.landings.wrapping_add(1);
            p.bhop_grace = BHOP_LATE;
        }
        p.air_time = 0.0;
    } else {
        p.air_time += dt;
    }

    if let Some(pad) = map.pad_at(p.feet()) {
        p.vel = pad.launch_velocity(p.vel);
        p.grounded = false;
        p.sliding = false;
    }
    // Safety net; the arena is sealed, so this should never trigger.
    if p.pos.y < -20.0 || !p.pos.is_finite() {
        p.pos = Vec3::new(0.0, 8.0, 0.0);
        p.vel = Vec3::ZERO;
    }
}

/// Called when a crouching player touches the ground: fast enough, and it is a slide.
fn start_slide(p: &mut LocalPlayer) {
    let speed = p.horizontal_speed();
    if p.sliding || speed < SLIDE_MIN_SPEED {
        return;
    }
    p.sliding = true;
    if p.slide_boost_cooldown <= 0.0 && speed < SLIDE_BOOST_SPEED {
        p.slide_boost_cooldown = SLIDE_BOOST_COOLDOWN;
        p.vel.x *= SLIDE_BOOST_SPEED / speed;
        p.vel.z *= SLIDE_BOOST_SPEED / speed;
    }
}

/// Source/Quake acceleration: speed up along `wish_dir` until the velocity's
/// projection onto it reaches `wish_speed`.
fn accelerate(vel: &mut Vec3, wish_dir: Vec3, wish_speed: f32, accel: f32, dt: f32) {
    let add = wish_speed - vel.dot(wish_dir);
    if add > 0.0 {
        *vel += wish_dir * (accel * dt).min(add);
    }
}

fn air_accelerate(vel: &mut Vec3, wish_dir: Vec3, air_accel: f32, dt: f32) {
    // Below running speed you get plain air control, so hopping from a standstill
    // is not a crawl. Above it, only strafing adds speed.
    if vel.with_y(0.0).length() < RUN_SPEED {
        accelerate(vel, wish_dir, RUN_SPEED, AIR_CONTROL_ACCEL, dt);
    }
    accelerate(vel, wish_dir, AIR_WISH_SPEED, air_accel * RUN_SPEED, dt);
}

fn apply_friction(vel: &mut Vec3, dt: f32) {
    let speed = vel.with_y(0.0).length();
    if speed < 0.01 {
        vel.x = 0.0;
        vel.z = 0.0;
        return;
    }
    let drop = speed.max(STOP_SPEED) * FRICTION * dt;
    let scale = (speed - drop).max(0.0) / speed;
    vel.x *= scale;
    vel.z *= scale;
}

fn follow_camera(
    time: Res<Time>,
    settings: Res<Settings>,
    fitted_models: Res<FittedModels>,
    mut player: ResMut<LocalPlayer>,
    camera: Single<(&mut Transform, &mut Projection), With<MainCamera>>,
    view_model: Single<
        (Entity, &mut Transform, &mut Visibility, &mut GunHolder),
        (With<ViewModel>, Without<MainCamera>, Without<ViewModelPart>),
    >,
    mut parts: Query<
        (&ViewModelPart, &mut Transform, &mut Visibility),
        (Without<MainCamera>, Without<ViewModel>, Without<GunPart>),
    >,
    mut gun_parts: Query<
        (&GunPart, &mut Transform, &mut Visibility),
        (Without<MainCamera>, Without<ViewModel>, Without<ViewModelPart>),
    >,
) {
    let dt = time.delta_secs();
    let knife = player.slot == KNIFE_SLOT;
    // A stab lasts longer than a gun's kick.
    player.recoil = (player.recoil - dt * if knife { 4.0 } else { 9.0 }).max(0.0);
    player.throw_anim = (player.throw_anim - dt * 2.5).max(0.0);
    // Once the shooting stops, the view drifts back to where it was aimed.
    player.kick_hold = (player.kick_hold - dt).max(0.0);
    if player.kick_hold <= 0.0 && player.view_kick != Vec2::ZERO {
        let back = player.view_kick * (dt * KICK_RECOVERY).min(1.0);
        player.view_kick -= back;
        player.pitch = (player.pitch - back.x).clamp(-PITCH_LIMIT, PITCH_LIMIT);
        player.yaw -= back.y;
        if player.view_kick.length_squared() < 1e-8 {
            player.view_kick = Vec2::ZERO;
        }
    }
    player.hit_marker = (player.hit_marker - dt).max(0.0);
    player.headshot_marker = (player.headshot_marker - dt).max(0.0);
    player.kill_marker = (player.kill_marker - dt * 1.6).max(0.0);
    player.damage_flash = (player.damage_flash - dt * 2.5).max(0.0);
    if !player.damage_arcs.is_empty() {
        for arc in &mut player.damage_arcs {
            arc.left -= dt;
        }
        player.damage_arcs.retain(|arc| arc.left > 0.0);
    }
    player.blind = (player.blind - dt).max(0.0);
    let holding = player.hand.is_some() as u8 as f32;
    player.hold += (holding - player.hold) * (dt * 12.0).min(1.0);
    let eye_target = if player.crouching { CROUCH_EYE } else { STAND_EYE };
    player.view_height += (eye_target - player.view_height) * (dt * 14.0).min(1.0);

    let (mut transform, mut projection) = camera.into_inner();
    // When dead the camera sees through a teammate's eyes (`remote::spectate`),
    // or else stays where we died.
    if let Some(eye) = player.pov {
        transform.translation = eye;
    } else if player.alive {
        transform.translation = player.eye();
    }
    transform.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);

    // Widen the field of view a little at speed; narrow it when aiming.
    if let Projection::Perspective(perspective) = &mut *projection {
        let boost = ((player.horizontal_speed() - RUN_SPEED) / 25.0).clamp(0.0, 1.0) * 0.22;
        let hip = settings.fov.to_radians() + boost;
        let target = hip * (1.0 + (player.zoom() - 1.0) * player.aim);
        perspective.fov += (target - perspective.fov) * (dt * 14.0).min(1.0);
    }

    // The gun: slides to the middle when aiming, kicks when fired, dips for a
    // grenade throw, and turns over while it is reloaded.
    let stats = player.weapon();
    let [width, height, length] = stats.size;
    // Drawn as its model if it has one (once loaded), else as boxes.
    let model = fitted_models.get(player.weapon_id());
    let per_round = stats.reload_per_round;
    // How far through a magazine reload (see `MAGAZINE_OUT` and the rest).
    let reload = if player.reloading() && !per_round { player.reload_progress() } else { 0.0 };
    // 0 -> 1 as the gun tips over, 1 -> 0 as it comes back up. Loading round by
    // round it stays tipped from the first round to the last.
    let loading = (player.reloading() && per_round) as u8 as f32;
    player.reload_pose += (loading - player.reload_pose) * (dt * 10.0).min(1.0);
    let tipped = if per_round {
        smooth(player.reload_pose)
    } else {
        smooth(reload / 0.18) * smooth((1.0 - reload) / 0.18)
    };
    let magazine_out = magazine_out(reload);
    // Cocking it: the hand on the grip pushes the gun forward against the
    // pull on the slide or bolt, and it springs back once that is let go.
    let cocking = cocking(reload);
    let tug = if reload < BOLT_HOME { cocking } else { smooth((BOLT_HOME + 0.05 - reload) / 0.05) };
    let throw = (player.throw_anim * std::f32::consts::PI).sin();
    let hold = smooth(player.hold);

    // 1 -> 0 as a weapon just switched to comes up from below.
    let drawing = smooth(player.draw / DRAW_TIME);
    // A gun kicks back; the knife lunges forward and across.
    let (kick, kick_pitch, kick_yaw) = if knife {
        let lunge = (player.recoil * std::f32::consts::PI).sin();
        (Vec3::new(-0.14, 0.05, -0.22) * lunge, -0.3 * lunge, 0.7 * lunge)
    } else {
        // Aimed, it only kicks straight back along the line of sight: tipping
        // or lifting it would throw the sight's dot off the crosshair, so it
        // would stop showing where the next bullet goes.
        let loose = 1.0 - player.aim;
        (
            Vec3::new(0.0, 0.01 * loose, 0.07 * (1.0 - 0.6 * player.aim)) * player.recoil,
            player.recoil * 0.07 * loose,
            0.0,
        )
    };
    // Aimed: centred. A sight's dot goes on the crosshair, close to the eye like
    // a real one, and so does the aim point of a model's iron sights (at the
    // rear sight); without one marked, a model's highest point lines up under
    // the crosshair, or the box gun's sight sits just below it.
    let dot = model.and_then(|fitted| fitted.aim_point(player.sight_id()).or(fitted.iron_aim));
    let aimed = match (dot, model) {
        (Some(dot), _) => Vec3::new(-dot.x, -dot.y, -stats.sight_relief - dot.z),
        (None, Some(Fitted { size, .. })) => Vec3::new(0.0, -(size.y / 2.0 + 0.003), -0.31 - length / 2.0),
        (None, None) => Vec3::new(0.0, -(height / 2.0 + 0.048), -0.31 - length / 2.0),
    };

    let (holder_entity, mut gun, mut gun_visibility, mut holder) = view_model.into_inner();
    holder.weapon = player.weapon_id();
    holder.sight = player.sight_id();
    gun.translation = hip_position(length).lerp(aimed, player.aim)
        + kick
        + Vec3::new(-0.05, 0.035, -0.03) * tipped
        + Vec3::new(0.005, 0.0, -0.018) * tug
        + Vec3::new(0.0, -0.16, 0.05) * throw
        + Vec3::new(0.04, -0.14, 0.06) * hold
        + Vec3::new(0.0, -0.3, 0.0) * drawing;
    // Reloading, it comes up towards the middle and rolls its underside
    // towards us, so the magazine (or the loading port) is in view.
    gun.rotation = Quat::from_rotation_x(kick_pitch + tipped * 0.1 - throw * 0.5 - hold * 0.35 - drawing * 0.9)
        * Quat::from_rotation_y(kick_yaw)
        * Quat::from_rotation_z(-tipped * 0.75 - tug * 0.12);

    // The same few boxes make every weapon; only their sizes differ.
    let rear = length / 2.0;
    for (part, mut transform, mut visibility) in &mut parts {
        let (at, size, shown) = match part {
            // The knife is a blade (the front two thirds) and its handle.
            ViewModelPart::Body if knife => (
                Vec3::new(0.0, 0.0, -length / 6.0),
                Vec3::new(width, height, length * 2.0 / 3.0),
                true,
            ),
            ViewModelPart::Grip if knife => (
                Vec3::new(0.0, 0.0, length / 3.0),
                Vec3::new(0.026, 0.03, length / 3.0),
                true,
            ),
            ViewModelPart::Body => (Vec3::ZERO, Vec3::new(width, height, length), true),
            ViewModelPart::Grip => (
                Vec3::new(0.0, -height / 2.0 - 0.035, rear - 0.04),
                Vec3::new(width * 0.8, 0.08, 0.04),
                true,
            ),
            ViewModelPart::Sight => (
                Vec3::new(0.0, height / 2.0 + 0.009, 0.01 - rear),
                Vec3::new(0.006, 0.018, 0.006),
                !knife,
            ),
            ViewModelPart::Magazine => (
                Vec3::new(0.0, -height / 2.0 - 0.045, (rear - 0.13).max(rear - 0.045 - length * 0.3))
                    + Vec3::new(0.0, -0.22, 0.0) * magazine_out,
                Vec3::new(width * 0.7, 0.1, 0.05),
                !knife,
            ),
        };
        transform.translation = at;
        transform.scale = size;
        let wanted = if shown && model.is_none() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    // The parts of the model that move.
    let in_hand = player.guns.get(player.slot).copied();
    let empty = in_hand.is_some_and(|gun| gun.ammo == 0);
    // The slide or bolt: kicked back by each shot and held back while empty.
    // Every magazine reload ends by cocking it: drawn back once the new
    // magazine is in, and let go to go home.
    let held_back = (empty || player.reloading() && player.reload_empty) && !(reload >= BOLT_HOME);
    let caught = if held_back { BOLT_CAUGHT } else { 0.0 };
    let drawn = caught.max(cocking);
    let kicked = SHOT_KICK * player.recoil * player.recoil;
    // A bolt action's bolt is locked when it fires, and worked by hand: lifted
    // and drawn back after each shot, pushed home and locked for the next one,
    // and cocked the same way at the end of a magazine reload. It is not held
    // back when empty, so that reload starts with it closed.
    let bolt_action = stats.cycle == Some(Cycle::Bolt);
    let (lift, opened) = match in_hand {
        _ if !bolt_action => (0.0, 0.0),
        _ if player.reloading() && !per_round => bolt_cocking(reload, stats.reload),
        Some(gun) if gun.cooldown > 0.0 && gun.ammo > 0 => bolt_cycle(stats.interval - gun.cooldown, stats.interval),
        _ => (0.0, 0.0),
    };
    // The pump: worked after each shot, and once more after loading it empty.
    let pump_cycle = match in_hand {
        _ if player.racking => player.reload_progress(),
        Some(gun) if gun.cooldown > 0.0 => (stats.interval - gun.cooldown - PUMP_AFTER) / PUMP_TIME,
        _ => -1.0,
    };
    let pump = if (0.0..1.0).contains(&pump_cycle) { (pump_cycle * std::f32::consts::PI).sin() } else { 0.0 };
    // A shell comes up from under the gun into the loading port and is pushed in.
    let shell = (player.reloading() && per_round && !player.racking).then(|| player.reload_progress());
    for (part, mut transform, mut visibility) in &mut gun_parts {
        if part.holder != holder_entity || part.weapon != player.weapon_id() {
            continue;
        }
        let shown = match part.kind {
            PartKind::Magazine => {
                part.pose_magazine(&mut transform, &mut visibility, magazine_out);
                continue;
            }
            // Turned about the bore (its origin) to lift the handle on the right.
            PartKind::Bolt if bolt_action => {
                part.pose(&mut transform, Vec3::Z * length * BOLT_ACTION_TRAVEL * opened, Vec3::Z, BOLT_LIFT * lift);
                true
            }
            PartKind::Slide | PartKind::Bolt => {
                let travel = if part.kind == PartKind::Slide { SLIDE_TRAVEL } else { BOLT_TRAVEL };
                part.pose(&mut transform, Vec3::Z * length * kicked.max(travel * drawn), Vec3::X, 0.0);
                true
            }
            PartKind::Pump => {
                part.pose(&mut transform, Vec3::Z * 0.085 * length * pump, Vec3::X, 0.0);
                true
            }
            PartKind::Shell => match shell {
                Some(at) if at < 0.8 => {
                    let below = Vec3::new(0.0, -0.13, 0.02) * length;
                    let port = Vec3::new(0.0, -0.025, 0.0) * length;
                    let inside = Vec3::new(0.0, -0.012, -0.1) * length;
                    let offset = if at < 0.5 {
                        below.lerp(port, smooth(at / 0.5))
                    } else {
                        port.lerp(inside, smooth((at - 0.5) / 0.3))
                    };
                    part.pose(&mut transform, offset, Vec3::X, 0.5 * (1.0 - smooth(at / 0.5)));
                    true
                }
                _ => false,
            },
        };
        visibility.set_if_neq(if shown { Visibility::Inherited } else { Visibility::Hidden });
    }

    // Looking through a scope, its lens fills the screen instead.
    let wanted = if (player.alive || player.pov.is_some()) && player.scope_view() < 1.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *gun_visibility != wanted {
        *gun_visibility = wanted;
    }
}

/// A magazine dropped in a reload, falling to the floor. Only for show:
/// nothing collides with it.
#[derive(Component)]
pub struct DroppedMagazine {
    vel: Vec3,
    /// Turning, as an axis scaled by radians per second.
    spin: Vec3,
    age: f32,
    pub landed: bool,
    scale: Vec3,
    /// Whose magazine it is, for the sound it makes landing.
    weapon: WeaponId,
}

/// Once the old magazine has dropped away from the gun in our hands, a copy
/// of it falls from there to the floor.
fn drop_magazine(
    mut commands: Commands,
    player: Res<LocalPlayer>,
    mut last_progress: Local<f32>,
    view_model: Single<Entity, With<ViewModel>>,
    parts: Query<(Entity, &GunPart, &GlobalTransform)>,
    descendants: Query<&Children>,
    meshes: Query<(&Mesh3d, &MeshMaterial3d<StandardMaterial>, &GlobalTransform)>,
) {
    let progress = if player.reloading() && !player.weapon().reload_per_round {
        player.reload_progress()
    } else {
        0.0
    };
    let dropped = *last_progress < MAGAZINE_DROPPED && progress >= MAGAZINE_DROPPED;
    *last_progress = progress;
    if !dropped {
        return;
    }
    let Some((magazine, _, global)) = parts.iter().find(|(_, part, _)| {
        part.holder == *view_model && part.weapon == player.weapon_id() && part.kind == PartKind::Magazine
    }) else {
        return;
    };
    let at = global.compute_transform();
    // It keeps our speed, and is let go downwards and a little forwards.
    let forward = map::look_dir(player.yaw, 0.0);
    commands
        .spawn((
            DroppedMagazine {
                vel: player.vel + forward * 0.6 - Vec3::Y * 1.5,
                spin: forward.cross(Vec3::Y) * 5.0 + Vec3::Y * 1.5,
                age: 0.0,
                landed: false,
                scale: at.scale,
                weapon: player.weapon_id(),
            },
            at,
            Visibility::default(),
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|copy| {
            for part in descendants.iter_descendants(magazine) {
                if let Ok((mesh, material, mesh_global)) = meshes.get(part) {
                    copy.spawn((mesh.clone(), material.clone(), mesh_global.reparented_to(global)));
                }
            }
        });
}

/// Dropped magazines fall until they hit something, lie there a while, then
/// shrink away.
fn fall_magazines(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<GameMap>,
    mut magazines: Query<(Entity, &mut DroppedMagazine, &mut Transform)>,
    mut cues: MessageWriter<SoundCue>,
) {
    let dt = time.delta_secs().min(0.05);
    for (entity, mut magazine, mut transform) in &mut magazines {
        magazine.age += dt;
        let left = DROPPED_MAGAZINE_TIME - magazine.age;
        if left <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        if left < 0.3 {
            transform.scale = magazine.scale * (left / 0.3);
        }
        if magazine.landed {
            continue;
        }
        magazine.vel.y -= GRAVITY * dt;
        let step = magazine.vel * dt;
        let distance = step.length();
        // It stops a little short of what it hits, as it hangs below its origin.
        if distance > 0.0
            && let Some(hit) = map.0.raycast(transform.translation, step / distance, distance + 0.04)
        {
            transform.translation += step / distance * (hit - 0.04).max(0.0);
            magazine.landed = true;
            let at = transform.translation;
            cues.write(SoundCue {
                sound: Sound::MagDrop(surface_under(&map.0, at), magazine.weapon),
                at: Some(at),
            });
            continue;
        }
        transform.translation += step;
        transform.rotation = Quat::from_scaled_axis(magazine.spin * dt) * transform.rotation;
    }
}

/// Slow fly-around of the arena behind the menus.
fn orbit_camera(
    time: Res<Time>,
    map: Res<GameMap>,
    camera: Single<(&mut Transform, &mut Projection), With<MainCamera>>,
) {
    let (mut transform, mut projection) = camera.into_inner();
    let (center, distance, height) = map.0.look.orbit;
    let angle = time.elapsed_secs() * 0.12;
    *transform = Transform::from_translation(center + Vec3::new(angle.cos() * distance, height, angle.sin() * distance))
        .looking_at(center, Vec3::Y);
    if let Projection::Perspective(perspective) = &mut *projection
        && perspective.fov != BASE_FOV
    {
        perspective.fov = BASE_FOV;
    }
}

fn grab_cursor(
    state: Res<State<AppState>>,
    controls: Res<Controls>,
    player: Res<LocalPlayer>,
    args: Res<crate::LaunchArgs>,
    mut focus: MessageReader<WindowFocused>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursor: Single<&mut CursorOptions>,
) {
    // Free on the death screen, to change the loadout. The scripted self-test
    // must not steal the developer's mouse.
    let grab = *state.get() == AppState::Playing
        && controls.captured
        && !player.on_death_screen()
        && args.selftest.is_none();
    let mode = if grab {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    // Windows drops the lock when the window loses focus (alt-tab), but the component still
    // says Locked, so Bevy never applies it again. Touching it makes Bevy grab again: on
    // focus coming back, and on any click in case focus came back mid title-bar drag.
    let refocused = focus.read().fold(false, |any, event| any || event.focused);
    if cursor.grab_mode != mode || cursor.visible == grab {
        cursor.grab_mode = mode;
        cursor.visible = !grab;
    } else if grab && (refocused || mouse.get_just_pressed().next().is_some()) {
        cursor.set_changed();
    }
}

fn send_state(player: Res<LocalPlayer>, mut connection: ResMut<Connection>) {
    if player.alive {
        connection.send_state(&ClientState {
            pos: player.pos.to_array(),
            yaw: player.yaw,
            pitch: player.pitch,
            flags: (player.crouching as u8 * FLAG_CROUCH)
                | (player.reloading() as u8 * FLAG_RELOAD)
                | ((player.aim > 0.5) as u8 * FLAG_AIM),
            weapon: player.weapon_id(),
            sight: player.sight_id(),
            spawn_seq: player.spawn_seq,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Block;

    /// A bolt action's (lift, drawn back), to within rounding.
    fn bolt_is(got: (f32, f32), lift: f32, back: f32) {
        assert!((got.0 - lift).abs() < 1e-3 && (got.1 - back).abs() < 1e-3, "{got:?}, not ({lift}, {back})");
    }

    #[test]
    fn a_bolt_action_opens_after_a_shot_and_is_locked_for_the_next() {
        let interval = weapon(crate::weapons::SNIPER).interval;
        bolt_is(bolt_cycle(0.0, interval), 0.0, 0.0);
        // Lifted before it is drawn back.
        bolt_is(bolt_cycle(BOLT_BACK + BOLT_LIFT_TIME, interval), 1.0, 0.0);
        bolt_is(bolt_cycle(BOLT_BACK + BOLT_LIFT_TIME + BOLT_DRAW_TIME, interval), 1.0, 1.0);
        bolt_is(bolt_cycle(interval - BOLT_LOCK, interval), 1.0, 1.0);
        // Pushed home before it is turned down.
        bolt_is(bolt_cycle(interval - BOLT_LOCK + BOLT_PUSH_TIME, interval), 1.0, 0.0);
        bolt_is(bolt_cycle(interval, interval), 0.0, 0.0);
    }

    #[test]
    fn a_bolt_action_is_cocked_at_the_end_of_a_reload() {
        let reload = weapon(crate::weapons::SNIPER).reload;
        bolt_is(bolt_cocking(0.0, reload), 0.0, 0.0);
        bolt_is(bolt_cocking(MAGAZINE_IN, reload), 0.0, 0.0);
        bolt_is(bolt_cocking(BOLT_PULLED, reload), 1.0, 1.0);
        bolt_is(bolt_cocking(BOLT_HOME, reload), 1.0, 1.0);
        bolt_is(bolt_cocking(1.0, reload), 0.0, 0.0);
    }

    /// A floor at y = 0 and nothing else.
    fn floor() -> Map {
        let mut map = Map::empty(0);
        map.blocks.push(Block {
            min: Vec3::new(-1000.0, -1.0, -1000.0),
            max: Vec3::new(1000.0, 0.0, 1000.0),
            color: [0.0; 3],
            visible: true,
        });
        map.finish()
    }

    fn standing(map: &Map) -> LocalPlayer {
        let mut p = LocalPlayer {
            alive: true,
            grounded: true,
            ..default()
        };
        p.teleport(Vec3::Y * PLAYER_HALF.y);
        // Settle onto the floor.
        advance(&mut p, 0.1, &idle(), map, DEFAULT_AIR_ACCEL.into(), false);
        p
    }

    fn idle() -> Held {
        Held {
            keys: Vec2::ZERO,
            yaw_from: 0.0,
            yaw_to: 0.0,
            crouch: false,
            aim: false,
        }
    }

    /// Runs `seconds` of play at `fps`, calling `input` before each frame
    /// with the time; it may turn the view (`yaw`) and returns the keys held.
    fn play(p: &mut LocalPlayer, map: &Map, fps: f32, seconds: f32, mut input: impl FnMut(&mut LocalPlayer, f32) -> Vec2) {
        let frames = (seconds * fps).round() as u32;
        for frame in 0..frames {
            let yaw_from = p.yaw;
            let keys = input(p, (frame + 1) as f32 / fps);
            let held = Held { keys, yaw_from, yaw_to: p.yaw, ..idle() };
            advance(p, 1.0 / fps, &held, map, DEFAULT_AIR_ACCEL.into(), false);
        }
    }

    /// Jump pressed a little before each landing hops again the step it
    /// lands, so the player is almost never on the ground when a frame ends;
    /// every landing must still be counted (it is what makes the sound).
    #[test]
    fn every_bunny_hop_landing_is_counted() {
        let map = floor();
        // Airborne 2 * JUMP_SPEED / GRAVITY per hop.
        let hops = (4.0 / (2.0 * JUMP_SPEED / GRAVITY)) as u32;
        for fps in [30.0, 60.0, 144.0, 240.0] {
            let mut p = standing(&map);
            let (before, floor_y) = (p.landings, p.feet().y);
            let frames = (4.0 * fps) as u32;
            for _ in 0..frames {
                // Pressed every frame on the ground or coming down onto it.
                if p.grounded || (p.vel.y < 0.0 && p.feet().y < floor_y + 0.4) {
                    p.jump_queued = true;
                }
                advance(&mut p, 1.0 / fps, &idle(), &map, DEFAULT_AIR_ACCEL.into(), false);
            }
            assert_eq!(p.landings - before, hops, "{fps} fps");
        }
    }

    /// Lands from a hop at 15 m/s with jump pressed `press` seconds from
    /// the landing (negative is before it); the speed it carries on.
    fn bhop_speed(press: f32) -> f32 {
        let map = floor();
        let mut p = standing(&map);
        p.air_jumps = 0;
        p.vel = Vec3::new(15.0, JUMP_SPEED, 0.0);
        p.grounded = false;
        let landing = 2.0 * JUMP_SPEED / GRAVITY;
        let fps = 144.0;
        let mut pressed = false;
        play(&mut p, &map, fps, landing + 0.3, |p, t| {
            if !pressed && t >= landing + press {
                pressed = true;
                p.jump_queued = true;
            }
            Vec2::ZERO
        });
        p.horizontal_speed()
    }

    #[test]
    fn a_bunny_hop_must_be_timed() {
        // Pressed a little before or after landing: the speed is kept.
        for press in [-0.08, -0.02, 0.02, 0.07] {
            let speed = bhop_speed(press);
            assert!(speed > 14.5, "pressed {press} s from landing, kept {speed} m/s");
        }
        // Pressed too late: friction has had it.
        let late = bhop_speed(0.25);
        assert!(late < 13.0, "pressed late, kept {late} m/s");
    }

    #[test]
    fn holding_jump_does_not_bunny_hop() {
        let map = floor();
        let mut p = standing(&map);
        let before = p.landings;
        p.jump_queued = true;
        // Held from here on: no new press, so it lands and stays down.
        play(&mut p, &map, 144.0, 1.5, |_, _| Vec2::ZERO);
        assert!(p.grounded);
        assert_eq!(p.landings - before, 1);
    }

    #[test]
    fn a_dash_from_the_ground_hops_a_little() {
        let map = floor();
        let mut p = standing(&map);
        let start = p.feet().y;
        p.dash_queued = true;
        let mut top = start;
        play(&mut p, &map, 144.0, 0.6, |p, _| {
            top = top.max(p.feet().y);
            Vec2::ZERO
        });
        let full = JUMP_SPEED * JUMP_SPEED / (2.0 * GRAVITY);
        let hop = top - start;
        assert!((hop - full / 4.0).abs() < 0.05, "dash hopped {hop} m, a jump {full} m");
    }

    /// On a face of a Surf ramp, going down the hall at 18 m/s with the
    /// view along it, for `seconds`, holding `keys`. Returns the player, and
    /// whether they were ever on the ground.
    fn surf_for(keys: Vec2, seconds: f32) -> (LocalPlayer, bool) {
        let map = crate::surf::surf();
        let mut p = LocalPlayer { alive: true, ..default() };
        // The left wall ramp, whose ridge is 9.14 m up here; feet 3 m under it.
        let feet = 9.14 - 3.0;
        let face = -40.0 + 3.0 / 60f32.to_radians().tan();
        p.teleport(Vec3::new(face + PLAYER_HALF.x + 0.05, feet + PLAYER_HALF.y, 30.0));
        p.vel = Vec3::new(0.0, 0.0, -18.0);
        let mut stood = false;
        play(&mut p, &map, 144.0, seconds, |p, _| {
            stood |= p.grounded;
            keys
        });
        (p, stood)
    }

    /// Holding the key into the ramp keeps you on it, sliding with no
    /// friction; let go and you slide off it into the pit.
    #[test]
    fn strafing_into_a_ramp_surfs_it() {
        let (p, stood) = surf_for(Vec2::NEG_X, 1.5);
        assert!(!stood);
        assert!(p.feet().y > 2.0, "fell off at {}", p.pos);
        assert!(p.pos.x < -32.0, "left the ramp at {}", p.pos);
        assert!(p.vel.length() > 18.0, "slowed to {}", p.vel);
        assert!(p.pos.z < 30.0 - 18.0 * 1.5, "{}", p.pos);

        let (p, _) = surf_for(Vec2::ZERO, 1.5);
        assert!(p.grounded && p.feet().y.abs() < 0.01, "still up at {}", p.pos);
    }

    /// Strafing right in the air at 12 m/s while turning right at a steady
    /// rate, as a mouse does: the speed gained in a second.
    fn strafe_gain(fps: f32) -> f32 {
        let map = Map::empty(0).finish();
        let mut p = LocalPlayer { alive: true, ..default() };
        p.teleport(Vec3::new(0.0, 500.0, 0.0));
        p.vel = map::look_dir(0.0, 0.0) * 12.0;
        play(&mut p, &map, fps, 1.0, |p, t| {
            p.yaw = -2.0 * t;
            Vec2::X
        });
        p.horizontal_speed() - 12.0
    }

    #[test]
    fn air_strafing_gains_the_same_at_any_frame_rate() {
        let reference = strafe_gain(60.0);
        assert!(reference > 1.0, "strafing gained only {reference} m/s");
        for fps in [30.0, 144.0, 240.0, 500.0] {
            let gain = strafe_gain(fps);
            assert!((gain - reference).abs() < 0.05 * reference, "{fps} fps gained {gain} m/s, 60 fps {reference}");
        }
    }

    fn jump_height(fps: f32) -> f32 {
        let map = floor();
        let mut p = standing(&map);
        p.vel.y = JUMP_SPEED;
        p.grounded = false;
        let start = p.feet().y;
        let mut top = start;
        play(&mut p, &map, fps, 1.0, |p, _| {
            top = top.max(p.feet().y);
            Vec2::ZERO
        });
        top - start
    }

    #[test]
    fn jumps_reach_the_same_height_at_any_frame_rate() {
        let reference = jump_height(60.0);
        assert!(reference > 1.0, "jumped only {reference} m");
        for fps in [30.0, 144.0, 300.0] {
            let height = jump_height(fps);
            assert!((height - reference).abs() < 0.03, "{fps} fps jumped {height} m, 60 fps {reference}");
        }
    }

    #[test]
    fn a_dash_pressed_between_steps_still_happens_once() {
        let map = floor();
        // At 1000 fps most frames run no step at all.
        let mut p = standing(&map);
        let (mut dashes, mut cooldown) = (0, 0.0);
        play(&mut p, &map, 1000.0, 0.2, |p, t| {
            if p.dash_cooldown > cooldown {
                dashes += 1;
            }
            cooldown = p.dash_cooldown;
            // Pressed on a frame that runs no step: the 11th of the 8th step.
            if (t * 1000.0).round() == 11.0 {
                p.dash_queued = true;
            }
            Vec2::ZERO
        });
        assert_eq!(dashes, 1);
        assert!(p.dash_cooldown > 0.0 && p.dash_cooldown < DASH_COOLDOWN);
    }

    #[test]
    fn the_view_follows_between_steps_and_jumps_on_a_teleport() {
        let map = floor();
        let mut p = standing(&map);
        p.vel = Vec3::new(10.0, 0.0, 0.0);
        let mut last = p.eye().x;
        play(&mut p, &map, 1000.0, 0.05, |p, _| {
            // Never backwards, never further than one step.
            assert!(p.eye().x >= last - 1e-4 && p.eye().x - last < 10.0 * TICK);
            last = p.eye().x;
            Vec2::ZERO
        });
        assert!(p.pos.x - p.eye().x < 10.0 * TICK + 1e-4);
        p.pos = Vec3::new(50.0, PLAYER_HALF.y, 0.0);
        advance(&mut p, 0.0, &idle(), &map, DEFAULT_AIR_ACCEL.into(), false);
        assert!((p.eye().x - 50.0).abs() < 1e-4);
    }
}
