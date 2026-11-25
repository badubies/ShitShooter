//! Sound. There are no audio files: every effect is synthesised at startup
//! and handed to Bevy as in-memory samples. Other players' sounds are placed
//! around us, so you can hear where shots and footsteps come from: louder in
//! the ear facing them, and duller when they are behind.

use std::{
    collections::HashMap,
    f32::consts::{FRAC_PI_4, TAU},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use bevy::{
    audio::{AddAudioSource, ChannelCount, Decodable, Sample, SampleRate, Source, Volume},
    prelude::*,
    transform::TransformSystems,
};

use crate::{
    AppState,
    map::Map,
    player::{
        BOLT_DRAW_TIME, BOLT_LIFT_TIME, BOLT_PULL, BOLT_PULLED, BOLT_PUSH_TIME, LANDING_AIR_TIME, LocalPlayer,
        PlayerSystems,
    },
    protocol::PlayerId,
    weapons::{DMR, GUN_COUNT, PISTOL, RIFLE, SHOTGUN, SMG, SNIPER, WeaponId, weapon},
    remote::{RemotePlayer, RemoteSystems},
    world::{GameMap, MainCamera},
};

const SAMPLE_RATE: u32 = 44_100;
/// Seconds into `Sound::BoltHome` that the bolt locks: it is started this
/// long before the gun is ready, so the lock is heard as it is.
pub const BOLT_LOCK: f32 = 0.12;
/// Seconds into `Sound::Pump` that the pump reaches the back.
pub const PUMP_CLACK: f32 = 0.09;
/// Different takes of each gun's shot, so automatic fire does not sound
/// like one sound over and over.
const GUNSHOT_TAKES: u32 = 3;
/// Metres walked between two footstep sounds.
const STRIDE: f32 = 2.6;
/// Slower than this is treated as standing still.
const STEP_MIN_SPEED: f32 = 2.0;
/// Spatial sounds are at full volume inside this many metres, then fall off
/// with the square of the distance.
const GUNSHOT_FULL_VOLUME_RANGE: f32 = 10.0;
const EXPLOSION_FULL_VOLUME_RANGE: f32 = 16.0;
const FOOTSTEP_FULL_VOLUME_RANGE: f32 = 4.0;

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Clip>()
            .add_message::<SoundCue>()
            .insert_resource(GlobalVolume::new(Volume::Linear(0.64)))
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    (local_footsteps, remote_footsteps)
                        .after(PlayerSystems::Simulate)
                        .after(RemoteSystems)
                        .run_if(in_state(AppState::Playing)),
                    play_cues,
                )
                    .chain(),
            )
            // Once the camera has moved this frame.
            .add_systems(PostUpdate, place_sounds.after(TransformSystems::Propagate));
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Sound {
    /// Each gun's own shot.
    Gunshot(WeaponId),
    /// The tick of one of our bullets reaching somebody.
    Hit,
    /// The same, sharper and louder, for a headshot.
    Headshot,
    /// The chime of a kill.
    Kill,
    Footstep(Surface),
    /// Hitting the ground after a jump or fall.
    Land(Surface),
    /// A weapon being taken out: a rustle, and the grip taken.
    Draw,
    /// Clothes and gear moving as a reload starts.
    Rustle,
    /// That gun's magazine released and sliding out.
    MagOut(WeaponId),
    /// That gun's magazine pushed in until it latches.
    MagIn(WeaponId),
    /// That gun's bolt or slide drawn back to cock it, near the end of a reload.
    BoltPull(WeaponId),
    /// That gun's bolt or slide snapping forward at the end of a reload.
    Bolt(WeaponId),
    /// A magazine of that gun, dropped in a reload, hitting the ground.
    MagDrop(Surface, WeaponId),
    /// A shell pushed into a shotgun's tube.
    ShellIn,
    /// A shotgun's pump racked back and forth.
    Pump,
    /// A bolt action lifted and drawn back after a shot, throwing out the case.
    BoltBack,
    /// The bolt pushed home and locked down: ready to fire.
    BoltHome,
    /// The click of a grenade's pin being pulled.
    Pin,
    /// The swish of a grenade being thrown.
    Throw,
    /// A grenade hitting something at this speed (m/s).
    Bounce(Surface, f32),
    /// A grenade come down rolling and skidding to a stop, at this speed (m/s).
    Roll(Surface, f32),
    Explosion,
    /// A flashbang's sharp bang.
    Flashbang,
    /// A smoke grenade bursting open with a hiss.
    SmokePop,
    /// The ringing in our ears after being flashed.
    Ringing,
    /// We took somebody's last armor.
    ArmorBreak,
    /// Somebody took our last armor.
    ArmorBroken,
    /// Armor picked up: ours, or heard from where somebody else took it.
    ArmorPickup,
    /// One second of the countdown before a match.
    Countdown,
    /// The countdown is over: go.
    Go,
    /// We or a teammate pinged something.
    Ping,
    /// A soft click: the gun in hand is fully choked.
    Choke,
}

/// Ask for a sound to be played. `at: None` is our own sound (not spatial);
/// `Some(position)` is somebody else's, heard from that point in the world.
#[derive(Message)]
pub struct SoundCue {
    pub sound: Sound,
    pub at: Option<Vec3>,
}

#[derive(Resource)]
struct Sounds {
    /// Indexed by weapon: close by, and far off (see `NEAR_SHOT`).
    gunshots: Vec<Vec<Handle<Clip>>>,
    distant_gunshots: Vec<Vec<Handle<Clip>>>,
    bolt_back: Handle<Clip>,
    bolt_home: Handle<Clip>,
    /// Indexed by `Surface`: a few takes of each, so no two steps in a row
    /// sound the same.
    footsteps: [Vec<Handle<Clip>>; Surface::COUNT],
    landings: [Vec<Handle<Clip>>; Surface::COUNT],
    /// The take of a footstep played last, not to be played again next.
    last_footstep: usize,
    pin: Handle<Clip>,
    throw: Handle<Clip>,
    /// Indexed by `Surface`.
    bounces: [Vec<Handle<Clip>>; Surface::COUNT],
    rolls: [Vec<Handle<Clip>>; Surface::COUNT],
    /// Close by, and far off (see `NEAR_BLAST`).
    explosions: [Vec<Handle<Clip>>; 2],
    flashbangs: [Vec<Handle<Clip>>; 2],
    tick: Handle<Clip>,
    chime: Handle<Clip>,
    shatter: Handle<Clip>,
    charge: Handle<Clip>,
    smoke: Handle<Clip>,
    ring: Handle<Clip>,
    beep: Handle<Clip>,
    blip: Handle<Clip>,
    click: Handle<Clip>,
    draw: Handle<Clip>,
    rustle: Handle<Clip>,
    /// These three indexed by weapon.
    mag_out: Vec<Handle<Clip>>,
    mag_in: Vec<Handle<Clip>>,
    bolt_pull: Vec<Handle<Clip>>,
    bolt: Vec<Handle<Clip>>,
    /// Indexed by `Surface`.
    mag_drops: [Vec<Handle<Clip>>; Surface::COUNT],
    shell_in: Handle<Clip>,
    pump: Handle<Clip>,
    /// Seconds since our ears started ringing: the world sounds muffled
    /// and far away for a while after.
    deafened: f32,
    /// State of the tiny random generator used to vary pitch.
    rng: u32,
}

fn setup(mut commands: Commands, mut clips: ResMut<Assets<Clip>>) {
    let mut add = |samples: Vec<f32>| {
        clips.add(Clip {
            samples: samples.iter().map(|sample| sample.clamp(-1.0, 1.0)).collect(),
            panner: None,
        })
    };
    commands.insert_resource(Sounds {
        gunshots: (0..GUN_COUNT)
            .map(|weapon_id| (0..GUNSHOT_TAKES).map(|take| add(synth_gunshot(weapon_id, take, false))).collect())
            .collect(),
        distant_gunshots: (0..GUN_COUNT)
            .map(|weapon_id| (0..DISTANT_SHOT_TAKES).map(|take| add(synth_gunshot(weapon_id, take, true))).collect())
            .collect(),
        bolt_back: add(synth_bolt_back()),
        bolt_home: add(synth_bolt_home()),
        footsteps: Surface::ALL.map(|surface| {
            (0..FOOTSTEP_TAKES).map(|take| add(synth_footstep(surface, take, false))).collect()
        }),
        landings: Surface::ALL.map(|surface| {
            (0..LANDING_TAKES).map(|take| add(synth_footstep(surface, take, true))).collect()
        }),
        last_footstep: 0,
        pin: add(synth_pin()),
        throw: add(synth_throw()),
        bounces: Surface::ALL.map(|surface| (0..BOUNCE_TAKES).map(|take| add(synth_bounce(surface, take))).collect()),
        rolls: Surface::ALL.map(|surface| (0..ROLL_TAKES).map(|take| add(synth_roll(surface, take))).collect()),
        explosions: [false, true].map(|far| (0..BLAST_TAKES).map(|take| add(synth_explosion(take, far))).collect()),
        flashbangs: [false, true].map(|far| (0..BLAST_TAKES).map(|take| add(synth_flashbang(take, far))).collect()),
        tick: add(synth_tick()),
        chime: add(synth_chime()),
        shatter: add(synth_shatter()),
        charge: add(synth_charge()),
        smoke: add(synth_smoke()),
        ring: add(synth_ring()),
        beep: add(synth_beep()),
        blip: add(synth_blip()),
        click: add(synth_click()),
        draw: add(synth_handling(true)),
        rustle: add(synth_handling(false)),
        mag_out: (0..GUN_COUNT).map(|weapon_id| add(synth_mag_out(weapon_id))).collect(),
        mag_in: (0..GUN_COUNT).map(|weapon_id| add(synth_mag_in(weapon_id))).collect(),
        bolt_pull: (0..GUN_COUNT).map(|weapon_id| add(synth_bolt_pull(weapon_id))).collect(),
        bolt: (0..GUN_COUNT).map(|weapon_id| add(synth_bolt_release(weapon_id))).collect(),
        mag_drops: Surface::ALL.map(|surface| (0..MAG_DROP_TAKES).map(|take| add(synth_mag_drop(surface, take))).collect()),
        shell_in: add(synth_shell_in()),
        pump: add(synth_pump()),
        deafened: f32::INFINITY,
        rng: 0x2545_f491,
    });
}

fn play_cues(
    mut commands: Commands,
    mut cues: MessageReader<SoundCue>,
    mut sounds: ResMut<Sounds>,
    mut clips: ResMut<Assets<Clip>>,
    listener: Query<&GlobalTransform, With<MainCamera>>,
    time: Res<Time>,
) {
    sounds.deafened += time.delta_secs();
    for cue in cues.read() {
        let random = next_random(&mut sounds.rng);
        let own = cue.at.is_none();
        // (sound, volume, playback speed)
        let (source, volume, speed) = match cue.sound {
            Sound::Gunshot(weapon_id) => {
                let takes = sounds.gunshots.get(weapon_id as usize).unwrap_or(&sounds.gunshots[RIFLE as usize]);
                (
                    takes[(random * 997.0) as usize % takes.len()].clone(),
                    if own { 0.5 } else { 1.0 },
                    0.97 + random * 0.06,
                )
            }
            Sound::Hit => (sounds.tick.clone(), 0.55, 0.96 + random * 0.08),
            Sound::Headshot => (sounds.tick.clone(), 0.85, 1.6),
            Sound::Kill => (sounds.chime.clone(), 0.6, 1.0),
            Sound::Footstep(surface) => {
                let takes = &sounds.footsteps[surface as usize];
                let mut take = (random * 997.0) as usize % takes.len();
                if take == sounds.last_footstep {
                    take = (take + 1) % takes.len();
                }
                let source = takes[take].clone();
                sounds.last_footstep = take;
                // The takes differ already: only a little more pitch and level.
                let loudness = 0.85 + 0.15 * next_random(&mut sounds.rng);
                // Ours are much quieter than somebody else's close by: we
                // hear ours all the time and need to hear theirs.
                (source, loudness * if own { 0.11 } else { 0.5 }, 0.95 + random * 0.1)
            }
            Sound::Land(surface) => {
                let takes = &sounds.landings[surface as usize];
                (
                    takes[(random * 997.0) as usize % takes.len()].clone(),
                    if own { 0.16 } else { 0.5 },
                    0.95 + random * 0.1,
                )
            }
            Sound::Draw => (sounds.draw.clone(), 0.5, 0.95 + random * 0.1),
            Sound::Rustle => (sounds.rustle.clone(), 0.35, 0.9 + random * 0.2),
            Sound::Pin => (sounds.pin.clone(), 0.45, 0.98 + random * 0.04),
            Sound::Throw => (sounds.throw.clone(), 0.5, 0.92 + random * 0.16),
            Sound::Bounce(surface, speed) => {
                let takes = &sounds.bounces[surface as usize];
                (
                    takes[(random * 997.0) as usize % takes.len()].clone(),
                    // The harder it hits, the louder.
                    (speed / 12.0).clamp(0.25, 1.0) * 0.3,
                    0.95 + random * 0.1,
                )
            }
            Sound::Explosion => (pick(&sounds.explosions[0], random), 1.0, 0.95 + random * 0.1),
            Sound::Flashbang => (pick(&sounds.flashbangs[0], random), 1.0, 0.97 + random * 0.06),
            Sound::SmokePop => (sounds.smoke.clone(), 1.0, 0.95 + random * 0.1),
            Sound::Roll(surface, speed) => (
                pick(&sounds.rolls[surface as usize], random),
                (speed / 6.0).clamp(0.3, 1.0) * 0.3,
                0.9 + random * 0.2,
            ),
            Sound::Ringing => {
                sounds.deafened = 0.0;
                (sounds.ring.clone(), 0.45, 1.0)
            }
            Sound::ArmorBreak => (sounds.shatter.clone(), 0.75, 1.2 + random * 0.06),
            // Played low and slow it sounds heavier: something of ours gave way.
            Sound::ArmorBroken => (sounds.shatter.clone(), 1.0, 0.68),
            Sound::ArmorPickup => (sounds.charge.clone(), if own { 0.6 } else { 1.0 }, 1.0),
            Sound::Countdown => (sounds.beep.clone(), 0.45, 1.0),
            // An octave up, and louder.
            Sound::Go => (sounds.beep.clone(), 0.7, 2.0),
            Sound::Ping => (sounds.blip.clone(), 0.5, 1.0),
            Sound::Choke => (sounds.click.clone(), 0.3, 1.0),
            Sound::MagOut(weapon_id) => (by_weapon(&sounds.mag_out, weapon_id), 0.55, 0.97 + random * 0.06),
            // Hardly varied: it should seat as the magazine is seen to.
            Sound::MagIn(weapon_id) => (by_weapon(&sounds.mag_in, weapon_id), 0.6, 0.98 + random * 0.04),
            // Not sped up or slowed: it hits the stop as the slide is seen to.
            Sound::BoltPull(weapon_id) => (by_weapon(&sounds.bolt_pull, weapon_id), 0.5, 1.0),
            Sound::Bolt(weapon_id) => (by_weapon(&sounds.bolt, weapon_id), 0.6, 0.97 + random * 0.06),
            Sound::MagDrop(surface, weapon_id) => {
                let takes = &sounds.mag_drops[surface as usize];
                (
                    takes[(random * 997.0) as usize % takes.len()].clone(),
                    0.4,
                    // A smaller magazine sounds higher.
                    handling(weapon_id).pitch.sqrt() * (0.95 + random * 0.1),
                )
            }
            Sound::ShellIn => (sounds.shell_in.clone(), 0.5, 0.92 + random * 0.16),
            Sound::Pump => (sounds.pump.clone(), 0.75, 0.97 + random * 0.06),
            Sound::BoltBack => (sounds.bolt_back.clone(), 0.45, 0.97 + random * 0.06),
            // Not sped up or slowed: the lock must land when the gun is ready.
            Sound::BoltHome => (sounds.bolt_home.clone(), 0.6, 1.0),
        };
        // A shot or blast far off is heard as a sound of its own, duller
        // and rolling, blended in with distance.
        let (distant, near, far) = match cue.sound {
            Sound::Explosion => (Some(pick(&sounds.explosions[1], random)), NEAR_BLAST, FAR_BLAST),
            Sound::Flashbang => (Some(pick(&sounds.flashbangs[1], random)), NEAR_BLAST, FAR_BLAST),
            Sound::Gunshot(weapon_id) => (
                sounds.distant_gunshots.get(weapon_id as usize).map(|takes| pick(takes, random)),
                NEAR_SHOT,
                FAR_SHOT,
            ),
            _ => (None, 0.0, 0.0),
        };
        let far = match (&distant, cue.at, listener.single()) {
            (Some(_), Some(at), Ok(ears)) => smoothstep(near, far, at.distance(ears.translation())),
            _ => 0.0,
        };
        // Just after a flashbang the world is drowned out by the ringing,
        // and comes back over a few seconds. Not the game's own signals.
        let heard = match cue.sound {
            Sound::Ringing | Sound::Hit | Sound::Headshot | Sound::Kill | Sound::Countdown | Sound::Go | Sound::Ping => 1.0,
            _ => 1.0 - DEAFNESS * (1.0 - smoothstep(0.0, DEAF_TIME, sounds.deafened)),
        };
        let volume = volume * heard;
        let range = match cue.sound {
            Sound::Gunshot(_) => GUNSHOT_FULL_VOLUME_RANGE,
            Sound::Explosion | Sound::Flashbang => EXPLOSION_FULL_VOLUME_RANGE,
            Sound::SmokePop => GUNSHOT_FULL_VOLUME_RANGE,
            _ => FOOTSTEP_FULL_VOLUME_RANGE,
        };
        for (source, volume) in [(Some(source), volume * (1.0 - far)), (distant, volume * far * DISTANT_BOOST)] {
            let Some(source) = source.filter(|_| volume > 0.001) else {
                continue;
            };
            let settings = PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(volume))
                .with_speed(speed);
            match cue.at {
                None => {
                    commands.spawn((AudioPlayer(source), settings));
                }
                Some(at) => {
                    let Some(samples) = clips.get(&source).map(|clip| clip.samples.clone()) else {
                        continue;
                    };
                    let heard_as = listener.single().map_or(Placement::CENTRE, |ears| placement(ears, at, range));
                    let panner = Arc::new(Panner::new(heard_as));
                    let clip = clips.add(Clip {
                        samples,
                        panner: Some(panner.clone()),
                    });
                    commands.spawn((AudioPlayer(clip), settings, Emitter { at, range, panner }));
                }
            }
        }
    }
}

/// One of a sound's takes, chosen by `random` (0..1).
fn pick(takes: &[Handle<Clip>], random: f32) -> Handle<Clip> {
    takes[(random * 997.0) as usize % takes.len()].clone()
}

/// 0 below `low`, 1 above `high`, easing in between.
fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The sound for `weapon_id` from a list of one per gun (the rifle's for
/// anything else).
fn by_weapon(sounds: &[Handle<Clip>], weapon_id: WeaponId) -> Handle<Clip> {
    sounds.get(weapon_id as usize).unwrap_or(&sounds[RIFLE as usize]).clone()
}

// ---------------------------------------------------------------- placing

/// How far a sound right to one side is turned towards that ear: 0 plays it
/// the same in both, 1 only in the near one. Hard to one side, the far ear
/// gets about a quarter of the near one (12 dB less).
const PAN_WIDTH: f32 = 0.7;
/// How much of a sound's treble is taken off when it is right behind us
/// (0 none, 1 all of it above `BEHIND_CUTOFF`), as the head shades it.
const BEHIND_MUFFLE: f32 = 0.65;
const BEHIND_CUTOFF: f32 = 2_000.0;
/// Closer than this (metres) a sound is in our face: it has no side.
const SIDELESS_RANGE: f32 = 0.5;
/// Seconds a playing sound takes to follow us turning, so the change is
/// smooth rather than a click.
const PAN_EASE_TIME: f32 = 0.01;
/// Samples between looks at where a playing sound should now be heard.
const PAN_UPDATE_SAMPLES: usize = 64;

/// One of our synthesised sounds: mono samples at `SAMPLE_RATE`. Ours are
/// played as they are; somebody else's are played in stereo, steered by a
/// `Panner`.
#[derive(Asset, TypePath)]
pub struct Clip {
    samples: Arc<[f32]>,
    panner: Option<Arc<Panner>>,
}

/// How loud a sound is in each ear, and how muffled (0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    left: f32,
    right: f32,
    muffle: f32,
}

impl Placement {
    /// Straight ahead and close.
    const CENTRE: Placement = Placement {
        left: std::f32::consts::FRAC_1_SQRT_2,
        right: std::f32::consts::FRAC_1_SQRT_2,
        muffle: 0.0,
    };

    fn towards(self, target: Placement, amount: f32) -> Placement {
        Placement {
            left: self.left + (target.left - self.left) * amount,
            right: self.right + (target.right - self.right) * amount,
            muffle: self.muffle + (target.muffle - self.muffle) * amount,
        }
    }
}

/// Where a playing sound should be heard, set by `place_sounds` each frame
/// and read by the audio thread as it plays.
struct Panner {
    left: AtomicU32,
    right: AtomicU32,
    muffle: AtomicU32,
}

impl Panner {
    fn new(placement: Placement) -> Panner {
        Panner {
            left: AtomicU32::new(placement.left.to_bits()),
            right: AtomicU32::new(placement.right.to_bits()),
            muffle: AtomicU32::new(placement.muffle.to_bits()),
        }
    }

    fn set(&self, placement: Placement) {
        self.left.store(placement.left.to_bits(), Ordering::Relaxed);
        self.right.store(placement.right.to_bits(), Ordering::Relaxed);
        self.muffle.store(placement.muffle.to_bits(), Ordering::Relaxed);
    }

    fn get(&self) -> Placement {
        Placement {
            left: f32::from_bits(self.left.load(Ordering::Relaxed)),
            right: f32::from_bits(self.right.load(Ordering::Relaxed)),
            muffle: f32::from_bits(self.muffle.load(Ordering::Relaxed)),
        }
    }
}

/// A playing sound of somebody else's: where it came from, and the panner
/// to keep pointing at it as we move and turn.
#[derive(Component)]
struct Emitter {
    at: Vec3,
    /// Full volume inside this many metres.
    range: f32,
    panner: Arc<Panner>,
}

/// How a sound at `at` is heard by `ears`: full volume inside `range` metres
/// and falling off with the square of the distance past it; louder in the
/// ear facing it (keeping the same power as it moves round), and duller
/// behind us.
fn placement(ears: &GlobalTransform, at: Vec3, range: f32) -> Placement {
    let offset = at - ears.translation();
    let distance = offset.length();
    let loudness = (range / distance.max(1e-3)).powi(2).min(1.0);
    let sided = (distance / SIDELESS_RANGE).min(1.0);
    let towards = offset.normalize_or_zero();
    let side = towards.dot(*ears.right()) * sided;
    let behind = (-towards.dot(*ears.forward())).max(0.0) * sided;
    let angle = FRAC_PI_4 * (1.0 + PAN_WIDTH * side);
    Placement {
        left: angle.cos() * loudness,
        right: angle.sin() * loudness,
        muffle: BEHIND_MUFFLE * behind,
    }
}

/// Points every playing sound of somebody else's at where it came from, as
/// we are now.
fn place_sounds(ears: Query<&GlobalTransform, With<MainCamera>>, emitters: Query<&Emitter>) {
    let Ok(ears) = ears.single() else {
        return;
    };
    for emitter in &emitters {
        emitter.panner.set(placement(ears, emitter.at, emitter.range));
    }
}

impl Decodable for Clip {
    type Decoder = ClipPlayer;

    fn decoder(&self) -> ClipPlayer {
        let now = self.panner.as_ref().map_or(Placement::CENTRE, |panner| panner.get());
        ClipPlayer {
            samples: self.samples.clone(),
            panner: self.panner.clone(),
            next: 0,
            now,
            target: now,
            low: 0.0,
            right: None,
        }
    }
}

/// Plays a `Clip`: mono as it is, or as stereo (left sample, then right)
/// when it has a panner.
pub struct ClipPlayer {
    samples: Arc<[f32]>,
    panner: Option<Arc<Panner>>,
    /// The next mono sample.
    next: usize,
    /// How it is heard now, easing towards `target`.
    now: Placement,
    target: Placement,
    /// The sound through a low pass, blended in as it goes behind us.
    low: f32,
    /// The right ear's sample, given after the left's.
    right: Option<f32>,
}

impl Iterator for ClipPlayer {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if let Some(right) = self.right.take() {
            return Some(right as Sample);
        }
        let dry = *self.samples.get(self.next)?;
        self.next += 1;
        let Some(panner) = &self.panner else {
            return Some(dry as Sample);
        };
        if self.next % PAN_UPDATE_SAMPLES == 0 {
            self.target = panner.get();
        }
        self.now = self.now.towards(self.target, 1.0 / (PAN_EASE_TIME * SAMPLE_RATE as f32));
        self.low += (1.0 - (-TAU * BEHIND_CUTOFF / SAMPLE_RATE as f32).exp()) * (dry - self.low);
        let sample = dry + self.now.muffle * (self.low - dry);
        self.right = Some(sample * self.now.right);
        Some((sample * self.now.left) as Sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let channels = self.channels().get() as usize;
        let left = (self.samples.len() - self.next) * channels + self.right.is_some() as usize;
        (left, Some(left))
    }
}

impl Source for ClipPlayer {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(if self.panner.is_some() { 2 } else { 1 }).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(self.samples.len() as f32 / SAMPLE_RATE as f32))
    }
}

/// Our own steps: one per stride while running on the ground, plus a thump on landing.
fn local_footsteps(
    player: Res<LocalPlayer>,
    map: Res<GameMap>,
    time: Res<Time>,
    mut cues: MessageWriter<SoundCue>,
    mut stride: Local<f32>,
    mut heard_landings: Local<u32>,
) {
    let landed = player.landings != *heard_landings;
    *heard_landings = player.landings;
    if !player.alive {
        *stride = 0.0;
        return;
    }
    let feet = player.pos - Vec3::Y * crate::map::player_half(player.crouching).y;
    if landed {
        cues.write(SoundCue {
            sound: Sound::Land(surface_under(&map.0, feet)),
            at: None,
        });
    }
    if !player.grounded {
        // Start the next stride half way, so the first step after landing comes quickly.
        *stride = STRIDE * 0.5;
        return;
    }

    let speed = player.horizontal_speed();
    // Crouch-walking and sliding are silent.
    if speed < STEP_MIN_SPEED || player.crouching {
        *stride = STRIDE * 0.5;
        return;
    }
    *stride += speed * time.delta_secs();
    if *stride >= STRIDE {
        *stride -= STRIDE;
        cues.write(SoundCue {
            sound: Sound::Footstep(surface_under(&map.0, feet)),
            at: None,
        });
    }
}

#[derive(Default)]
struct RemoteStride {
    last_pos: Vec3,
    stride: f32,
    airborne_for: f32,
    /// Coming down fast, as of the last frame.
    falling: bool,
}

/// Other players' steps. The server does not tell us whether they are on the
/// ground, so that is inferred from how they move: running without rising or
/// falling. A bunny hop never runs level, so falling one frame and rising the
/// next with the ground right under the feet is a landing too.
fn remote_footsteps(
    map: Res<GameMap>,
    time: Res<Time>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    mut cues: MessageWriter<SoundCue>,
    mut strides: Local<HashMap<PlayerId, RemoteStride>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    strides.retain(|id, _| remotes.iter().any(|(remote, _)| remote.id == *id));

    for (remote, transform) in &remotes {
        let pos = transform.translation;
        let state = strides.entry(remote.id).or_insert_with(|| RemoteStride {
            last_pos: pos,
            ..default()
        });
        let moved = pos - state.last_pos;
        state.last_pos = pos;
        // Dead, or teleported by a respawn.
        if !remote.alive || moved.length() > 3.0 {
            state.stride = 0.0;
            state.airborne_for = 0.0;
            continue;
        }
        let feet = pos - Vec3::Y * crate::map::player_half(remote.crouching).y;
        let vertical_speed = moved.y / dt;
        if vertical_speed.abs() > 1.5 {
            let bounced = vertical_speed > 0.0
                && state.falling
                && map.0.raycast(feet + Vec3::Y * 0.3, Vec3::NEG_Y, 0.8).is_some();
            if bounced {
                if state.airborne_for > LANDING_AIR_TIME {
                    cues.write(SoundCue {
                        sound: Sound::Land(surface_under(&map.0, feet)),
                        at: Some(feet),
                    });
                }
                state.airborne_for = 0.0;
            }
            state.falling = vertical_speed < 0.0;
            state.airborne_for += dt;
            state.stride = STRIDE * 0.5;
            continue;
        }
        state.falling = false;
        if state.airborne_for > LANDING_AIR_TIME {
            cues.write(SoundCue {
                sound: Sound::Land(surface_under(&map.0, feet)),
                at: Some(feet),
            });
        }
        state.airborne_for = 0.0;

        let travelled = moved.with_y(0.0).length();
        if travelled / dt < STEP_MIN_SPEED || remote.crouching {
            state.stride = STRIDE * 0.5;
            continue;
        }
        state.stride += travelled;
        if state.stride >= STRIDE {
            state.stride -= STRIDE;
            cues.write(SoundCue {
                sound: Sound::Footstep(surface_under(&map.0, feet)),
                at: Some(feet),
            });
        }
    }
}

// ---------------------------------------------------------------- synthesis

/// A short, bright blip: easy to hear through gunfire.
fn synth_tick() -> Vec<f32> {
    samples(0.05, |t| {
        let tone = (TAU * 2400.0 * t).sin() + 0.5 * (TAU * 3600.0 * t).sin();
        tone * (-t * 90.0).exp() * (t * 2000.0).min(1.0) * 0.6
    })
}

/// A metal knock: a decaying burst of filtered noise with a ring at
/// `frequency`, starting `at` seconds in. The building block of the reload
/// sounds.
fn knock(t: f32, at: f32, decay: f32, frequency: f32, noise: f32) -> f32 {
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let envelope = (-since * decay).exp() * (since * 4000.0).min(1.0);
    (noise * 1.3 + (TAU * frequency * since).sin() * 0.6) * envelope
}

/// A part ringing at a few unrelated pitches (frequency, decay per
/// second), struck `at` seconds in.
fn ring(t: f32, at: f32, partials: &[(f32, f32)]) -> f32 {
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let tone: f32 = partials
        .iter()
        .map(|(frequency, decay)| (TAU * frequency * since).sin() * (-since * decay).exp())
        .sum();
    tone * (since * 3000.0).min(1.0) / partials.len() as f32
}

/// How loud something sliding over `from..to` seconds is at `t`: it
/// picks up speed and slows again.
fn sliding(t: f32, from: f32, to: f32) -> f32 {
    if (from..to).contains(&t) {
        ((t - from) / (to - from) * std::f32::consts::PI).sin()
    } else {
        0.0
    }
}

/// Noise for things rubbing together: it catches and slips, so its level
/// wanders. `rough` is its state, starting at 0.5.
fn rubbing(noise: f32, rough: &mut f32, rng: &mut u32) -> f32 {
    *rough += (next_random(rng) - *rough) * 0.02;
    noise * *rough * 2.0
}

// ---------------------------------------------------------------- grenades

/// Takes of each blast, and of a grenade hitting each surface.
const BLAST_TAKES: u32 = 2;
const BOUNCE_TAKES: u32 = 3;
const ROLL_TAKES: u32 = 2;
/// Seconds the world stays muffled after our ears start ringing, and how
/// much quieter it is at first.
const DEAF_TIME: f32 = 5.0;
const DEAFNESS: f32 = 0.8;
/// A blast nearer than this (metres) is heard whole; one further than
/// `FAR_BLAST` only as its dull, rolling distant sound; in between, both.
const NEAR_BLAST: f32 = 20.0;
const FAR_BLAST: f32 = 60.0;
/// The same for gunfire, which carries less far.
const NEAR_SHOT: f32 = 25.0;
const FAR_SHOT: f32 = 90.0;
const DISTANT_SHOT_TAKES: u32 = 2;
/// How much louder the distant sound is played: low rumble carries.
const DISTANT_BOOST: f32 = 1.5;

/// Friedlander's blast wave, `t` seconds after it leaves: the pressure
/// jumps up, falls through zero at `duration` (a bigger charge pushes
/// longer, and sounds deeper), and a longer, shallower suction follows.
fn blast_wave(t: f32, duration: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    (1.0 - t / duration) * (-2.0 * t / duration).exp() * (t * 20_000.0).min(1.0)
}

/// A frag's pin pulled: it scrapes out of the fuse and its ring jingles,
/// the spoon flies off with a springy ping, the striker snaps down on the
/// primer, and the fuse starts to fizz.
fn synth_pin() -> Vec<f32> {
    let mut rng = 0x6e_0001;
    let mut rough = 0.5;
    let mut low_passed = 0.0;
    let mut pin = Biquad::new(Pass::Band, 3500.0, 2.0);
    let mut fizz = Biquad::new(Pass::Band, 5000.0, 0.8);
    samples(0.6, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let pulling = pin.run(rubbing(noise, &mut rough, &mut rng) * sliding(t, 0.0, 0.06)) * 0.5;
        let jingle = ring(t, 0.06, &[(4200.0, 30.0), (6100.0, 38.0), (7900.0, 45.0)]) * 0.5;
        let spoon = knock(t, 0.1, 350.0, 2400.0, noise) * 0.4
            + ring(t, 0.1, &[(2700.0, 18.0), (3900.0, 22.0), (5600.0, 28.0)]) * 0.8;
        let striker = knock(t, 0.115, 300.0, 1800.0, noise) * 0.6 + knock(t, 0.115, 120.0, 500.0, low_passed * 2.0) * 0.4;
        let burning = if t > 0.12 { ((t - 0.12) / 0.05).min(1.0) * (-(t - 0.12) * 4.0).exp() } else { 0.0 };
        let fizzing = fizz.run(noise * burning) * 0.25;
        ((pulling + jingle + spoon + striker + fizzing) * 1.4).tanh() * 0.4
    })
}

/// An arm swung hard: air rushing past, rising in pitch as it speeds up
/// and falling as it slows, over a rustle of sleeve.
fn synth_throw() -> Vec<f32> {
    const LENGTH: f32 = 0.28;
    let mut rng = 0x6e_0002;
    let mut air = Biquad::new(Pass::Band, 400.0, 1.4);
    let mut sleeve = Grains::new(2500.0);
    samples(LENGTH, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        // Fastest a little past half way, as the arm whips through.
        let speed = ((t / LENGTH).powf(0.8) * std::f32::consts::PI).sin().powi(2);
        air.retune(Pass::Band, 400.0 + 1600.0 * speed, 1.4);
        let whoosh = air.run(noise) * speed * 1.6;
        if next_random(&mut rng) < 1500.0 * (1.0 - t / LENGTH) / SAMPLE_RATE as f32 {
            let size = next_random(&mut rng).powi(2) * 0.6;
            sleeve.add(1200.0 * 5.0_f32.powf(next_random(&mut rng)), 1.2, size);
        }
        ((whoosh + sleeve.run(noise) * 0.6) * 1.6).tanh() * 0.6
    })
}

/// A grenade hitting something: on hard ground its thick metal body clanks
/// and rings; grass and earth swallow it in a thud; gravel crunches;
/// water plops.
fn synth_bounce(surface: Surface, take: u32) -> Vec<f32> {
    if surface == Surface::Water {
        return synth_plop(take + 10);
    }
    let mut rng = 0x6e_0101 ^ (surface as u32 + 1).wrapping_mul(0x9e37_79b9) ^ (take + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let mut vary = |value: f32, spread: f32| value * (1.0 + spread * (random() * 2.0 - 1.0));
    // (how much it clanks, thud cutoff, grains per second, grain pitches,
    // how sharply they ring, their gain)
    let (clank, thud_cutoff, grain_rate, (low, high), grain_q, grain_gain): (f32, f32, f32, (f32, f32), f32, f32) =
        match surface {
            Surface::Hard => (1.0, 1200.0, 0.0, (3000.0, 6000.0), 8.0, 0.0),
            Surface::Gravel => (0.4, 800.0, 3000.0, (1100.0, 6000.0), 8.0, 2.0),
            Surface::Dirt => (0.15, 550.0, 1500.0, (1500.0, 5000.0), 4.0, 1.2),
            _ => (0.05, 400.0, 4000.0, (2500.0, 6000.0), 0.7, 1.0),
        };
    let body = [
        (vary(1850.0, 0.08), 25.0),
        (vary(2930.0, 0.08), 35.0),
        (vary(4370.0, 0.08), 45.0),
        (vary(5600.0, 0.08), 60.0),
    ];
    let mut thud = Biquad::new(Pass::Low, thud_cutoff, 0.7);
    let mut grains = Grains::new(if grain_q < 1.0 { 3000.0 } else { 900.0 });
    let mut noise_rng = rng ^ 0x5bd1_e995;
    let mut sound = samples(0.35, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let blow = (-t * 70.0).exp() * (t / 0.0008).min(1.0);
        let density = (-t / 0.025).exp();
        if next_random(&mut noise_rng) < grain_rate * density / SAMPLE_RATE as f32 {
            let size = next_random(&mut noise_rng).powi(3) * density;
            grains.add(low * (high / low).powf(next_random(&mut noise_rng)), grain_q, size);
        }
        let out = ring(t, 0.0, &body) * clank * 2.5
            + knock(t, 0.0, 300.0, 2500.0, noise) * clank * 0.5
            + thud.run(noise * blow) * 2.0
            + grains.run(noise) * grain_gain * grain_q.sqrt();
        (out * 1.4).tanh()
    });
    muffle(&mut sound, 6000.0);
    finish(&mut sound, 0.8);
    sound
}

/// A grenade rolling and skidding to a stop: on hard ground its body
/// grinds and the spoon's lever rattles as it turns; gravel crunches under
/// it; earth grits; grass brushes past. All of it slowing down.
fn synth_roll(surface: Surface, take: u32) -> Vec<f32> {
    if surface == Surface::Water {
        return synth_plop(take + 20);
    }
    const LENGTH: f32 = 0.45;
    let mut rng = 0x6e_0501 ^ (surface as u32 + 1).wrapping_mul(0x9e37_79b9) ^ (take + 1).wrapping_mul(7919);
    // (grinding, rattles a second, grains a second, grain pitches, how
    // sharply they ring, their gain, rumble cutoff)
    let (grind, rattle, grain_rate, (low, high), grain_q, grain_gain, rumble_cutoff): (f32, f32, f32, (f32, f32), f32, f32, f32) =
        match surface {
            Surface::Hard => (1.0, 45.0, 0.0, (2000.0, 5000.0), 6.0, 0.0, 500.0),
            Surface::Gravel => (0.2, 12.0, 2200.0, (1000.0, 6000.0), 8.0, 1.6, 400.0),
            Surface::Dirt => (0.1, 4.0, 1200.0, (1200.0, 4500.0), 4.0, 1.1, 300.0),
            _ => (0.0, 2.0, 3500.0, (2500.0, 6500.0), 0.7, 1.0, 250.0),
        };
    let mut scrape = Biquad::new(Pass::Band, 3000.0, 1.5);
    let mut rumble = [Biquad::new(Pass::Low, rumble_cutoff, 0.7); 2];
    let mut grains = Grains::new(if grain_q < 1.0 { 3000.0 } else { 900.0 });
    // Little knocks of the body's ring, as the lever and fuse come round.
    let mut rattles = Grains::new(250.0);
    let mut rough = 0.5;
    let mut sound = samples(LENGTH, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        // Fast at first, then slowing to nothing.
        let speed = (t / 0.01).min(1.0) * (1.0 - t / LENGTH).max(0.0).powi(2);
        scrape.retune(Pass::Band, 1800.0 + 1800.0 * speed, 1.5);
        let grinding = scrape.run(rubbing(noise, &mut rough, &mut rng)) * speed * grind;
        if next_random(&mut rng) < rattle * speed / SAMPLE_RATE as f32 {
            let pitch = [1850.0, 2930.0, 4370.0][(next_random(&mut rng) * 2.99) as usize];
            rattles.add(pitch, 30.0, 0.3 + 0.7 * next_random(&mut rng));
        }
        if next_random(&mut rng) < grain_rate * speed / SAMPLE_RATE as f32 {
            let size = next_random(&mut rng).powi(3) * speed;
            grains.add(low * (high / low).powf(next_random(&mut rng)), grain_q, size);
        }
        let rolling = rumble.iter_mut().fold(noise, |x, filter| filter.run(x)) * speed;
        let out = grinding * 1.2 + rattles.run(noise) * 0.6 + grains.run(noise) * grain_gain * grain_q.sqrt() + rolling * 3.0;
        (out * 1.4).tanh()
    });
    muffle(&mut sound, 6000.0);
    finish(&mut sound, 0.8);
    sound
}

/// A frag going off. Close by: the blast wave's crack, a deep boom, the
/// roar of the fireball darkening as it dies, then earth and fragments
/// pattering down and the blast rolling back off everything around. Far
/// off (`far`), the crack and roar are lost on the way, leaving a dull
/// thump, a long rolling rumble, and echoes off the hills.
fn synth_explosion(take: u32, far: bool) -> Vec<f32> {
    let mut rng = 0x6e_0201 ^ (take * 2 + far as u32 + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let mut vary = |value: f32, spread: f32| value * (1.0 + spread * (random() * 2.0 - 1.0));
    let blast_time = vary(if far { 0.018 } else { 0.006 }, 0.1);
    let (boom_from, boom_to) = if far { (vary(50.0, 0.08), vary(24.0, 0.08)) } else { (vary(75.0, 0.08), vary(30.0, 0.08)) };
    // (seconds after, gain): the blast coming back off hills and buildings.
    let echoes = if far { vec![(vary(0.45, 0.2), 0.5), (vary(0.95, 0.2), 0.3)] } else { Vec::new() };
    let (roll_rate, roll_phase) = (vary(2.5, 0.3), random() * TAU);
    let mut crack = Biquad::new(Pass::Low, 9000.0, 0.7);
    let mut roar = Biquad::new(Pass::Low, 3000.0, 0.7);
    let mut rumble = [Biquad::new(Pass::Low, if far { 350.0 } else { 220.0 }, 0.7); 2];
    // What is left of the roar far off: a dull, short boom.
    let mut dull = Biquad::new(Pass::Low, 700.0, 0.7);
    let mut debris = Grains::new(900.0);
    let mut noise_rng = rng ^ 0x5bd1_e995;
    let mut phase = 0.0_f32;
    let mut sound = samples(if far { 3.5 } else { 3.0 }, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let thump = blast_wave(t, blast_time)
            + echoes.iter().map(|(at, gain)| blast_wave(t - at, blast_time * 1.5) * gain).sum::<f32>();
        phase += TAU * (boom_to + (boom_from - boom_to) * (-t * 12.0).exp()) / SAMPLE_RATE as f32;
        let boom = phase.sin() * (-t * if far { 3.0 } else { 5.0 }).exp() * (t * 1000.0).min(1.0);
        let rolling = rumble.iter_mut().fold(noise, |x, filter| filter.run(x));
        let swell = 1.0 + 0.4 * (TAU * roll_rate * t + roll_phase).sin();
        let tail = rolling * (t / 0.05).min(1.0) * (-t * if far { 1.1 } else { 1.5 }).exp() * swell;
        let near = if far {
            dull.run(noise) * (t / 0.01).min(1.0) * (-t * 7.0).exp() * 2.5
        } else {
            let cracking = crack.run(noise) * (-t * 40.0).exp();
            roar.retune(Pass::Low, 250.0 + 2800.0 * (-t * 6.0).exp(), 0.7);
            let roaring = roar.run(noise) * (-t * 4.5).exp() * (t / 0.004).min(1.0);
            // Debris comes down from a moment after, thickest at about half a second.
            let falling = if t > 0.12 { ((t - 0.12) / 0.35).min(1.0) * (-(t - 0.12) * 2.2).exp() } else { 0.0 };
            if next_random(&mut noise_rng) < 2500.0 * falling / SAMPLE_RATE as f32 {
                let size = next_random(&mut noise_rng).powi(3);
                debris.add(700.0 * (6000.0_f32 / 700.0).powf(next_random(&mut noise_rng)), 6.0, size);
            }
            cracking * 3.0 + roaring * 1.4 + debris.run(noise) * 3.5
        };
        let boom_gain = if far { 1.2 } else { 0.9 };
        ((thump * 1.2 + boom * boom_gain + tail * 4.0 + near) * 2.2).tanh()
    });
    finish(&mut sound, if far { 0.8 } else { 0.68 });
    sound
}

/// A flashbang going off. Close by: a very sharp, very loud bang with
/// little boom (the charge is small), its steel body ringing, and a short
/// tail. Far off (`far`): a duller crack and its echoes.
fn synth_flashbang(take: u32, far: bool) -> Vec<f32> {
    let mut rng = 0x6e_0301 ^ (take * 2 + far as u32 + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let mut vary = |value: f32, spread: f32| value * (1.0 + spread * (random() * 2.0 - 1.0));
    let blast_time = vary(if far { 0.006 } else { 0.0012 }, 0.1);
    let echoes = if far { vec![(vary(0.5, 0.2), 0.4)] } else { Vec::new() };
    let canister = [
        (vary(1450.0, 0.05), 9.0),
        (vary(2380.0, 0.05), 12.0),
        (vary(3720.0, 0.05), 16.0),
        (vary(5100.0, 0.05), 20.0),
    ];
    let (roll_rate, roll_phase) = (vary(3.0, 0.3), random() * TAU);
    let mut crack = Biquad::new(Pass::Low, if far { 1800.0 } else { 11_000.0 }, 0.7);
    let mut roar = Biquad::new(Pass::Low, 2500.0, 0.7);
    let mut rumble = [Biquad::new(Pass::Low, if far { 500.0 } else { 1200.0 }, 0.7); 2];
    let mut noise_rng = rng ^ 0x5bd1_e995;
    let mut phase = 0.0_f32;
    let mut sound = samples(if far { 2.5 } else { 2.2 }, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let bang = blast_wave(t, blast_time)
            + echoes.iter().map(|(at, gain)| blast_wave(t - at, blast_time * 1.5) * gain).sum::<f32>();
        let cracking = crack.run(noise) * (-t * if far { 40.0 } else { 90.0 }).exp();
        phase += TAU * (60.0 + 80.0 * (-t * 30.0).exp()) / SAMPLE_RATE as f32;
        let boom = phase.sin() * (-t * 22.0).exp() * (t * 1000.0).min(1.0);
        let rolling = rumble.iter_mut().fold(noise, |x, filter| filter.run(x));
        let swell = 1.0 + 0.4 * (TAU * roll_rate * t + roll_phase).sin();
        let tail = rolling * (t / 0.03).min(1.0) * (-t * if far { 1.8 } else { 2.5 }).exp() * swell;
        let near = if far {
            0.0
        } else {
            let roaring = roar.run(noise) * (-t * 12.0).exp();
            roaring + ring(t, 0.0, &canister) * 1.4
        };
        ((bang * 1.4 + cracking * 1.6 + boom * 0.5 + tail * 3.0 + near) * 3.0).tanh()
    });
    finish(&mut sound, if far { 0.75 } else { 0.66 });
    sound
}

/// A smoke grenade going off: the cap pops, the filler crackles alight,
/// then gas jets out with a sputtering hiss for several seconds and dies
/// down.
fn synth_smoke() -> Vec<f32> {
    let mut rng = 0x6e_0401;
    let mut low_passed = 0.0;
    let mut sputter = 0.5;
    let mut jet = Biquad::new(Pass::Band, 3200.0, 0.5);
    let mut flow = Biquad::new(Pass::Low, 400.0, 0.7);
    let mut crackle = Grains::new(1500.0);
    samples(6.0, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let pop = knock(t, 0.0, 90.0, 380.0, low_passed * 2.0) * 0.9 + knock(t, 0.0, 250.0, 1600.0, noise) * 0.4;
        let catching = (-t * 5.0).exp();
        if next_random(&mut rng) < (2500.0 * catching + 6.0) / SAMPLE_RATE as f32 {
            let size = next_random(&mut rng).powi(2) * (0.3 + catching);
            crackle.add(800.0 * 6.0_f32.powf(next_random(&mut rng)), 3.0, size);
        }
        let blowing = (t / 0.5).min(1.0) * if t < 3.5 { 1.0 } else { (-(t - 3.5) * 1.3).exp() };
        // The jet catches and chokes as it burns.
        sputter += (next_random(&mut rng) - sputter) * 0.004;
        let jetting = jet.run(noise) * blowing * (0.5 + sputter) * 0.9 + flow.run(noise) * blowing * 0.6;
        ((pop + crackle.run(noise) * 1.5 + jetting) * 1.4).tanh() * 0.8
    })
}

// ---------------------------------------------------------------- reloads

/// Seconds into `Sound::MagIn` that the magazine seats. It is played at
/// `MAGAZINE_IN`, a moment before the magazine is seen to be all the way in.
const MAGAZINE_SEAT: f32 = 0.03;
/// Takes of a magazine hitting each surface.
const MAG_DROP_TAKES: u32 = 2;

/// What a gun's magazine and action are like in the hands, for its reload
/// sounds.
struct Handling {
    /// Higher for a smaller magazine and lighter parts.
    pitch: f32,
    /// How much its parts ring: polymer hardly, steel a lot (0..1).
    metal: f32,
    /// Heavier parts hit with more body.
    weight: f32,
    /// The magazine is slapped home with a palm, not pushed with a thumb.
    slap: bool,
}

fn handling(weapon_id: WeaponId) -> Handling {
    match weapon_id {
        // Polymer, light.
        SMG => Handling {
            pitch: 1.2,
            metal: 0.3,
            weight: 0.75,
            slap: true,
        },
        // A bigger steel magazine than the rifle's, for a bigger cartridge.
        DMR => Handling {
            pitch: 0.92,
            metal: 0.75,
            weight: 1.15,
            slap: true,
        },
        SNIPER => Handling {
            pitch: 0.85,
            metal: 0.6,
            weight: 1.3,
            slap: true,
        },
        // A fat steel magazine and a very heavy slide.
        PISTOL => Handling {
            pitch: 1.1,
            metal: 0.9,
            weight: 0.95,
            slap: false,
        },
        // Aluminium.
        _ => Handling {
            pitch: 1.0,
            metal: 0.7,
            weight: 1.0,
            slap: true,
        },
    }
}

/// A gun's magazine released: the button and the catch click, the
/// magazine slides out of the well (a light one drops, a heavy one is
/// pulled) and ticks as it comes free.
fn synth_mag_out(weapon_id: WeaponId) -> Vec<f32> {
    let Handling { pitch, metal, weight, .. } = handling(weapon_id);
    let mut rng = 0x3a6_0101 ^ (weapon_id as u32 + 1).wrapping_mul(7919);
    let mut rough = 0.5;
    let mut well = Biquad::new(Pass::Band, 1500.0 * pitch, 1.0);
    let ringing = [(1900.0 * pitch, 60.0), (3100.0 * pitch, 80.0), (4700.0 * pitch, 110.0)];
    let out = 0.07 * weight;
    samples(0.15 + out, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        let button = knock(t, 0.0, 400.0, 3200.0 * pitch, noise) * 0.3;
        let catch = knock(t, 0.012, 260.0, 2300.0 * pitch, noise) * 0.6 + ring(t, 0.012, &ringing) * metal * 0.9;
        let slide = well.run(rubbing(noise, &mut rough, &mut rng) * sliding(t, 0.02, 0.02 + out)) * 0.8;
        let free = knock(t, 0.02 + out, 300.0, 2600.0 * pitch, noise) * 0.25;
        ((button + catch + slide + free) * 1.5).tanh() * 0.8
    })
}

/// A gun's magazine pushed home: it slides into the well and seats
/// `MAGAZINE_SEAT` seconds in with a thud and the latch clicking, a
/// palm slapping it on a rifle, and the rounds inside settling.
fn synth_mag_in(weapon_id: WeaponId) -> Vec<f32> {
    let Handling { pitch, metal, weight, slap } = handling(weapon_id);
    let mut rng = 0x3a6_0201 ^ (weapon_id as u32 + 1).wrapping_mul(7919);
    let mut rough = 0.5;
    let mut low_passed = 0.0;
    let mut well = Biquad::new(Pass::Band, 1700.0 * pitch, 1.0);
    let mut palm = Biquad::new(Pass::Low, 500.0, 0.7);
    let ringing = [(1700.0 * pitch, 50.0), (2900.0 * pitch, 70.0), (4300.0 * pitch, 95.0)];
    let mut rounds = Grains::new(1500.0);
    samples(0.22, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let entering = well.run(rubbing(noise, &mut rough, &mut rng) * sliding(t, 0.0, MAGAZINE_SEAT + 0.004)) * 0.6;
        let seat = knock(t, MAGAZINE_SEAT, 70.0 / weight, 260.0 * pitch, low_passed * 2.0) * weight.min(1.2)
            + knock(t, MAGAZINE_SEAT + 0.004, 260.0, 2600.0 * pitch, noise) * 0.8
            + ring(t, MAGAZINE_SEAT, &ringing) * metal * 0.9;
        let slapped = if slap && t >= MAGAZINE_SEAT { (-(t - MAGAZINE_SEAT) * 55.0).exp() } else { 0.0 };
        let slapping = palm.run(noise * slapped) * 1.6;
        let since = t - MAGAZINE_SEAT;
        if (0.008..0.07).contains(&since) && next_random(&mut rng) < 900.0 / SAMPLE_RATE as f32 {
            let size = next_random(&mut rng).powi(2) * 0.3;
            rounds.add((3000.0 + 3000.0 * next_random(&mut rng)) * pitch, 8.0, size);
        }
        let settling = rounds.run(noise) * 1.5;
        ((entering + seat + slapping + settling) * 1.5).tanh() * 0.62
    })
}

/// A gun's bolt or slide drawn back to cock it: fingers take hold of it, it
/// slides back against its spring over the time the view model takes, and
/// knocks into its stop.
fn synth_bolt_pull(weapon_id: WeaponId) -> Vec<f32> {
    let Handling { pitch, metal, weight, .. } = handling(weapon_id);
    let mut rng = 0x3a6_0302 ^ (weapon_id as u32 + 1).wrapping_mul(7919);
    let mut rough = 0.5;
    let mut rails = Biquad::new(Pass::Band, 2100.0 * pitch, 1.1);
    let ringing = [(1700.0 * pitch, 50.0), (2900.0 * pitch, 70.0)];
    let pull = (BOLT_PULLED - BOLT_PULL) * weapon(weapon_id).reload;
    samples(pull + 0.15, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        let grab = knock(t, 0.0, 500.0, 1800.0 * pitch, noise) * 0.12;
        // The spring gets stiffer as it goes back, so the rub gets louder.
        let drawn = (t / pull).min(1.0);
        let rub = rails.run(rubbing(noise, &mut rough, &mut rng) * sliding(t, 0.005, pull)) * (0.3 + 0.5 * drawn);
        let stop = knock(t, pull, 260.0, 1500.0 * pitch, noise) * 0.45 * weight.min(1.0)
            + ring(t, pull, &ringing) * metal * 0.5;
        ((grab + rub + stop) * 1.5).tanh() * 0.5
    })
}

/// A gun's bolt or slide let go at the end of a reload: the catch clicks,
/// it slams home and turns into its lugs, and the spring behind it rings.
fn synth_bolt_release(weapon_id: WeaponId) -> Vec<f32> {
    let Handling { pitch, metal, weight, .. } = handling(weapon_id);
    let mut rng = 0x3a6_0301 ^ (weapon_id as u32 + 1).wrapping_mul(7919);
    let mut low_passed = 0.0;
    let spring = [(880.0 * pitch, 14.0), (1340.0 * pitch, 18.0), (2210.0 * pitch, 26.0)];
    samples(0.4, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let catch = knock(t, 0.0, 380.0, 3000.0 * pitch, noise) * 0.3;
        let slam = knock(t, 0.015, 110.0, 1200.0 * pitch, noise)
            + knock(t, 0.015, 60.0 / weight, 300.0 * pitch, low_passed * 2.0) * weight;
        let lugs = knock(t, 0.032, 220.0, 2000.0 * pitch, noise) * 0.4;
        // A rifle's long buffer spring rings out; a pistol's short one hardly.
        let ringing = ring(t, 0.015, &spring) * metal * weight.min(1.0) * 0.8;
        ((catch + slam + lugs + ringing) * 1.6).tanh() * 0.6
    })
}

/// Clothes and gear moving: many tiny crinkles under a soft swish. With
/// `grip`, a hand then takes hold of the gun and its sling swivel jingles.
fn synth_handling(grip: bool) -> Vec<f32> {
    let mut rng = if grip { 0x3a6_0401 } else { 0x3a6_0402 };
    let mut low_passed = 0.0;
    let mut swish = Biquad::new(Pass::Band, 2200.0, 0.6);
    let mut cloth = Grains::new(2500.0);
    samples(0.26, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let moving = sliding(t, 0.0, 0.18);
        if next_random(&mut rng) < 1800.0 * moving / SAMPLE_RATE as f32 {
            let size = next_random(&mut rng).powi(2);
            cloth.add(1200.0 * 5.0_f32.powf(next_random(&mut rng)), 1.2, size);
        }
        let rustle = cloth.run(noise) * 0.9 + swish.run(noise * moving) * 0.25;
        let held = if grip {
            knock(t, 0.14, 200.0, 1500.0, noise) * 0.5
                + knock(t, 0.143, 110.0, 450.0, low_passed * 1.5) * 0.5
                + ring(t, 0.15, &[(4800.0, 60.0), (6900.0, 80.0)]) * 0.3
        } else {
            0.0
        };
        ((rustle + held) * 1.4).tanh() * 0.7
    })
}

/// A shell thumbed into the tube: its hull slides over the lifter, passes
/// the latch with a click, and knocks against the follower.
fn synth_shell_in() -> Vec<f32> {
    let mut rng = 0x3a6_0004;
    let mut rough = 0.5;
    let mut low_passed = 0.0;
    let mut hull = Biquad::new(Pass::Band, 1300.0, 0.9);
    samples(0.15, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let pushing = hull.run(rubbing(noise, &mut rough, &mut rng) * sliding(t, 0.0, 0.05)) * 0.6;
        let latch = knock(t, 0.045, 300.0, 2900.0, noise) * 0.55;
        let seat = knock(t, 0.05, 160.0, 700.0, low_passed * 1.5) * 0.7 + ring(t, 0.05, &[(1800.0, 60.0), (2700.0, 75.0)]) * 0.3;
        let slip = knock(t, 0.075, 350.0, 3400.0, noise) * 0.15;
        ((pushing + latch + seat + slip) * 1.5).tanh() * 0.75
    })
}

/// A pump racked: the forend slides back along its rails to a heavy clack
/// `PUMP_CLACK` seconds in, flicking the empty hull out, then forward again
/// and locks.
fn synth_pump() -> Vec<f32> {
    let mut rng = 0x3a6_0005;
    let mut rough = 0.5;
    let mut low_passed = 0.0;
    let mut rails = Biquad::new(Pass::Band, 1100.0, 0.8);
    samples(0.34, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let moving = sliding(t, 0.0, PUMP_CLACK + 0.005) + sliding(t, PUMP_CLACK + 0.04, 0.225);
        let slide = rails.run(rubbing(noise, &mut rough, &mut rng) * moving) * 0.5;
        let back = knock(t, PUMP_CLACK, 110.0, 1300.0, noise) + knock(t, PUMP_CLACK, 70.0, 350.0, low_passed * 2.0) * 0.8;
        let hull = knock(t, PUMP_CLACK + 0.03, 220.0, 1700.0, low_passed) * 0.15;
        let lock = knock(t, 0.22, 90.0, 900.0, noise) * 1.1
            + knock(t, 0.226, 220.0, 2400.0, noise) * 0.6
            + knock(t, 0.22, 60.0, 280.0, low_passed * 2.0) * 0.8;
        ((slide + back + hull + lock) * 1.5).tanh() * 0.8
    })
}

/// A dropped magazine hitting the ground: on hard ground its hollow body
/// clatters and bounces and the rounds rattle; grass takes it with a dull
/// thud, earth and gravel with grit or a crunch; water with a plop.
fn synth_mag_drop(surface: Surface, take: u32) -> Vec<f32> {
    if surface == Surface::Water {
        return synth_plop(take);
    }
    let mut rng = 0x3a6_0501 ^ (surface as u32 + 1).wrapping_mul(0x9e37_79b9) ^ (take + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let mut vary = |value: f32, spread: f32| value * (1.0 + spread * (random() * 2.0 - 1.0));
    // (when, how hard) it hits: it bounces only off hard things.
    let bounce = vary(0.1, 0.25);
    let hits = match surface {
        Surface::Hard => vec![(0.0, 1.0), (bounce, 0.35), (bounce + vary(0.06, 0.2), 0.12)],
        Surface::Gravel => vec![(0.0, 1.0), (bounce * 0.7, 0.2)],
        _ => vec![(0.0, 1.0)],
    };
    // (how much its body rings, thud cutoff, grains per second, grain
    // pitches, how sharply they ring, their gain)
    let (clatter, thud_cutoff, grain_rate, (low, high), grain_q, grain_gain): (f32, f32, f32, (f32, f32), f32, f32) = match surface {
        Surface::Hard => (1.0, 900.0, 1200.0, (3000.0, 6500.0), 10.0, 1.0),
        Surface::Gravel => (0.5, 700.0, 2500.0, (1100.0, 6000.0), 8.0, 2.0),
        Surface::Dirt => (0.15, 500.0, 1200.0, (1500.0, 5000.0), 4.0, 1.4),
        _ => (0.05, 380.0, 4000.0, (2500.0, 6000.0), 0.7, 1.2),
    };
    // The hollow body of the magazine.
    let body = [
        (vary(620.0, 0.08), 35.0),
        (vary(1130.0, 0.08), 45.0),
        (vary(1870.0, 0.08), 55.0),
        (vary(2950.0, 0.08), 70.0),
    ];
    let mut thud = Biquad::new(Pass::Low, thud_cutoff, 0.7);
    let mut grains = Grains::new(if grain_q < 1.0 { 3000.0 } else { 900.0 });
    let mut noise_rng = rng ^ 0x5bd1_e995;
    let mut sound = samples(0.45, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let (mut out, mut blow, mut density) = (0.0, 0.0, 0.0);
        for (at, hard) in &hits {
            if t < *at {
                continue;
            }
            let since = t - at;
            out += ring(t, *at, &body) * hard * clatter * 2.5;
            blow += hard * (-since * 60.0).exp() * (since / 0.001).min(1.0);
            density += hard * (-since / 0.03).exp();
        }
        out += thud.run(noise * blow) * 2.0;
        if next_random(&mut noise_rng) < grain_rate * density / SAMPLE_RATE as f32 {
            let size = next_random(&mut noise_rng).powi(3) * density.min(1.0);
            grains.add(low * (high / low).powf(next_random(&mut noise_rng)), grain_q, size);
        }
        out += grains.run(noise) * grain_gain * grain_q.sqrt();
        (out * 1.4).tanh()
    });
    muffle(&mut sound, 4500.0);
    finish(&mut sound, 0.8);
    sound
}

/// Something small dropping into shallow water: a little slosh and a few
/// bubbles.
fn synth_plop(take: u32) -> Vec<f32> {
    let mut rng = 0x00a9_7e99 ^ (take + 1).wrapping_mul(7919);
    let bubbles: Vec<(f32, f32, f32)> = (0..6)
        .map(|_| {
            let pitch = 700.0 * (2000.0_f32 / 700.0).powf(next_random(&mut rng));
            (0.005 + next_random(&mut rng).powi(2) * 0.08, pitch, 0.4 + 0.6 * next_random(&mut rng))
        })
        .collect();
    let mut slosh = Biquad::new(Pass::Low, 500.0, 0.9);
    let mut phases = vec![0.0_f32; bubbles.len()];
    let mut sound = samples(0.3, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        let mut out = slosh.run(noise * (t / 0.006).min(1.0) * (-t * 25.0).exp()) * 2.5;
        for ((at, pitch, gain), phase) in bubbles.iter().zip(&mut phases) {
            if t < *at {
                continue;
            }
            let since = t - at;
            let damping = 0.13 * pitch + 0.0072 * pitch.powf(1.5);
            *phase += TAU * pitch * (1.0 + 0.1 * damping * since) / SAMPLE_RATE as f32;
            out += phase.sin() * (-damping * since).exp() * (since * 4000.0).min(1.0) * gain;
        }
        out
    });
    muffle(&mut sound, FOOTSTEP_CUTOFF);
    finish(&mut sound, 0.7);
    sound
}

/// A soft, dry click, like a dial turning one notch.
fn synth_click() -> Vec<f32> {
    let mut rng = 0x00c1_1c4e;
    let mut low_passed = 0.0;
    samples(0.045, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.3;
        let knock = (low_passed * 1.4 + (TAU * 1350.0 * t).sin() * 0.5) * (-t * 170.0).exp();
        (knock * 1.8).tanh() * (t * 4000.0).min(1.0) * 0.7
    })
}

/// A short, plain electronic beep.
fn synth_beep() -> Vec<f32> {
    samples(0.18, |t| {
        let tone = (TAU * 740.0 * t).sin() + 0.25 * (TAU * 1480.0 * t).sin();
        tone * (t * 600.0).min(1.0) * ((0.18 - t) * 40.0).min(1.0) * 0.45
    })
}

/// Two quick high notes, falling: "over here".
fn synth_blip() -> Vec<f32> {
    samples(0.22, |t| {
        let note = |start: f32, frequency: f32| {
            if t < start {
                return 0.0;
            }
            let since = t - start;
            (TAU * frequency * since).sin() * (-since * 30.0).exp() * (since * 1500.0).min(1.0)
        };
        (note(0.0, 1975.5) + note(0.06, 1318.5)) * 0.45
    })
}

/// Two rising notes.
fn synth_chime() -> Vec<f32> {
    samples(0.4, |t| {
        let note = |start: f32, frequency: f32| {
            if t < start {
                return 0.0;
            }
            let since = t - start;
            let tone = (TAU * frequency * since).sin() + 0.3 * (TAU * frequency * 2.0 * since).sin();
            tone * (-since * 11.0).exp() * (since * 800.0).min(1.0)
        };
        (note(0.0, 880.0) + note(0.09, 1318.5)) * 0.5
    })
}

/// Armor shattering: a sharp crack of bright noise, glassy shards ringing
/// out one after another, and a dull thump as the plate gives way.
fn synth_shatter() -> Vec<f32> {
    let mut rng = 0x51ab_0001;
    let mut low_passed = 0.0;
    // (frequency, start, decay): inharmonic, so it rings like a shard, not a note.
    const SHARDS: [(f32, f32, f32); 5] = [
        (2350.0, 0.0, 22.0),
        (3170.0, 0.012, 26.0),
        (1720.0, 0.03, 18.0),
        (4410.0, 0.045, 30.0),
        (2890.0, 0.07, 24.0),
    ];
    samples(0.45, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.5;
        // What the low pass took out: only the hiss.
        let crack = (noise - low_passed) * (-t * 28.0).exp() * 1.6;
        let ring: f32 = SHARDS
            .iter()
            .filter(|(_, start, _)| t >= *start)
            .map(|(frequency, start, decay)| {
                let since = t - start;
                (TAU * frequency * since).sin() * (-since * decay).exp() * (since * 3000.0).min(1.0)
            })
            .sum();
        let thump = (TAU * 140.0 * t).sin() * (-t * 25.0).exp();
        (crack + ring * 0.4 + thump * 0.7).tanh() * (t * 2000.0).min(1.0) * 0.85
    })
}

/// A rising, shimmering sweep: armor charging up.
fn synth_charge() -> Vec<f32> {
    const LENGTH: f32 = 0.32;
    let mut phase = 0.0_f32;
    samples(LENGTH, |t| {
        let progress = t / LENGTH;
        let frequency = 520.0 + 900.0 * progress;
        phase += TAU * frequency / SAMPLE_RATE as f32;
        let tone = phase.sin() + 0.4 * (phase * 1.5).sin() + 0.25 * (phase * 2.0).sin();
        let shimmer = 0.75 + 0.25 * (TAU * 28.0 * t).sin();
        let envelope = (t * 200.0).min(1.0) * (1.0 - progress).powf(1.5);
        (tone * shimmer * envelope * 0.9).tanh() * 0.8
    })
}

/// Ears ringing after a bang: a high, pure whine, two tones a hair apart
/// beating slowly against each other, over a thin hiss, as loud as can be
/// at first and fading for as long as the world is muffled (`DEAF_TIME`).
fn synth_ring() -> Vec<f32> {
    let mut rng = 0x6e_0601;
    let mut hiss = [Biquad::new(Pass::Band, 3800.0, 6.0); 2];
    let mut phases = [0.0_f32; 3];
    let mut sound = samples(DEAF_TIME + 0.5, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        // Starts a little sharp and settles, wavering.
        let settle = 1.0 + 0.03 * (-t * 3.0).exp() + 0.0015 * (TAU * 0.7 * t).sin();
        let mut tone = 0.0;
        for (phase, (frequency, gain)) in phases.iter_mut().zip([(3800.0, 1.0), (3801.3, 0.8), (5650.0, 0.12)]) {
            *phase = (*phase + TAU * frequency * settle / SAMPLE_RATE as f32) % TAU;
            tone += phase.sin() * gain;
        }
        let hissing = hiss.iter_mut().fold(noise, |x, filter| filter.run(x));
        // Hits at once, then dies away slowly: faster at first.
        let fade = (t / 0.02).min(1.0) * (0.55 * (-t * 1.4).exp() + 0.45 * (-t * 0.45).exp());
        (tone * 0.5 + hissing * 3.0 * (-t * 0.8).exp()) * fade
    });
    finish(&mut sound, 0.75);
    sound
}

/// How one gun's shot is put together (see `synth_gunshot`). Decays are
/// per second, frequencies in Hz.
struct Report {
    length: f32,
    /// The blast wave out of the muzzle: seconds of overpressure (a bigger
    /// charge pushes longer, and sounds deeper), and its gain.
    blast: (f32, f32),
    /// The crack: bright noise (cutoff, decay, gain).
    crack: (f32, f32, f32),
    /// The boom: a sine falling from one pitch to another (from, to, decay, gain).
    boom: (f32, f32, f32, f32),
    /// The body: darker noise (cutoff, decay, gain).
    body: (f32, f32, f32),
    /// The action slamming back and forward: a metal knock (seconds after
    /// the shot, pitch, gain). Not on guns worked by hand.
    action: Option<(f32, f32, f32)>,
    /// The shot rolling back off the surroundings: dark noise (cutoff, decay, gain).
    tail: (f32, f32, f32),
    /// How hard it is driven into soft clipping: louder-seeming and rougher.
    drive: f32,
    level: f32,
}

fn report(weapon_id: WeaponId) -> Report {
    match weapon_id {
        SMG => Report {
            length: 0.6,
            blast: (0.0008, 0.9),
            crack: (8000.0, 240.0, 2.2),
            boom: (230.0, 95.0, 45.0, 0.5),
            body: (2800.0, 70.0, 1.2),
            action: Some((0.022, 2400.0, 0.3)),
            tail: (1000.0, 9.0, 1.2),
            drive: 1.8,
            level: 0.8,
        },
        SHOTGUN => Report {
            length: 1.4,
            blast: (0.0025, 1.0),
            crack: (5500.0, 110.0, 3.0),
            boom: (120.0, 48.0, 15.0, 0.75),
            body: (1500.0, 22.0, 1.6),
            action: None,
            tail: (700.0, 4.0, 1.8),
            drive: 2.5,
            level: 0.8,
        },
        // A bigger cartridge than the rifle's through a muzzle brake: louder
        // and deeper, with a heavier action, short of the sniper's boom.
        DMR => Report {
            length: 1.3,
            blast: (0.0015, 1.05),
            crack: (8200.0, 140.0, 3.0),
            boom: (160.0, 60.0, 20.0, 0.75),
            body: (2000.0, 34.0, 1.35),
            action: Some((0.032, 1700.0, 0.3)),
            tail: (850.0, 4.5, 1.7),
            drive: 2.3,
            level: 0.86,
        },
        SNIPER => Report {
            length: 1.8,
            blast: (0.0018, 1.1),
            crack: (7500.0, 120.0, 3.4),
            boom: (140.0, 50.0, 13.0, 0.7),
            body: (1800.0, 26.0, 1.4),
            action: None,
            tail: (800.0, 3.2, 2.0),
            drive: 2.6,
            level: 0.83,
        },
        PISTOL => Report {
            length: 0.95,
            blast: (0.0012, 1.0),
            crack: (6500.0, 170.0, 2.8),
            boom: (170.0, 70.0, 28.0, 0.8),
            body: (1500.0, 40.0, 1.4),
            action: Some((0.03, 2200.0, 0.3)),
            tail: (700.0, 5.0, 1.4),
            drive: 2.3,
            level: 0.85,
        },
        _ => Report {
            length: 0.9,
            blast: (0.0012, 1.0),
            crack: (9000.0, 160.0, 2.6),
            boom: (180.0, 70.0, 30.0, 0.8),
            body: (2200.0, 45.0, 1.3),
            action: Some((0.035, 1900.0, 0.25)),
            tail: (900.0, 6.0, 1.4),
            drive: 2.0,
            level: 0.9,
        },
    }
}

/// One take of a gun's shot: the blast wave and the crack of the muzzle,
/// a falling boom, the action working, and the tail rolling back. Far off
/// (`far`), the highs are lost on the way and the action is not heard: a
/// dull pop, then a longer rumble and an echo off the hills.
fn synth_gunshot(weapon_id: WeaponId, take: u32, far: bool) -> Vec<f32> {
    let mut report = report(weapon_id);
    if far {
        report.length *= 1.6;
        report.blast.0 *= 3.0;
        report.crack = (1400.0, report.crack.1 * 0.4, report.crack.2 * 0.6);
        report.boom.0 *= 0.75;
        report.boom.1 *= 0.75;
        report.boom.2 *= 0.6;
        report.body.0 *= 0.35;
        report.action = None;
        report.tail = (report.tail.0 * 0.6, report.tail.1 * 0.55, report.tail.2 * 1.4);
    }
    let mut rng = 0x9e37_79b9
        ^ (weapon_id as u32 + 1).wrapping_mul(0x85eb_ca6b)
        ^ (take * 2 + far as u32 + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let mut vary = |value: f32, spread: f32| value * (1.0 + spread * (random() * 2.0 - 1.0));
    let (blast_time, blast_gain) = report.blast;
    let blast_time = vary(blast_time, 0.1);
    let echo = if far { Some((vary(0.4, 0.25), 0.45)) } else { None };
    let (crack_cutoff, crack_decay, crack_gain) = report.crack;
    let mut crack = Biquad::new(Pass::Low, vary(crack_cutoff, 0.1), 0.7);
    let crack_decay = vary(crack_decay, 0.1);
    let (boom_from, boom_to, boom_decay, boom_gain) = report.boom;
    let (boom_from, boom_to) = (vary(boom_from, 0.06), vary(boom_to, 0.06));
    let (body_cutoff, body_decay, body_gain) = report.body;
    let mut body = Biquad::new(Pass::Low, vary(body_cutoff, 0.1), 0.7);
    let action = report.action.map(|(at, pitch, gain)| (vary(at, 0.15), vary(pitch, 0.08), gain));
    let (tail_cutoff, tail_decay, tail_gain) = report.tail;
    let mut tail = [Biquad::new(Pass::Low, vary(tail_cutoff, 0.1), 0.7); 2];
    // The tail swells and fades a little as echoes come back.
    let (roll_rate, roll_phase) = (vary(if far { 3.0 } else { 5.0 }, 0.3), random() * TAU);
    let mut noise_rng = rng ^ 0x5bd1_e995;
    let mut phase = 0.0_f32;
    let mut sound = samples(report.length, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let blast = blast_wave(t, blast_time)
            + echo.map_or(0.0, |(at, gain)| blast_wave(t - at, blast_time * 1.5) * gain);
        let cracking = crack.run(noise) * (-t * crack_decay).exp();
        phase += TAU * (boom_to + (boom_from - boom_to) * (-t * 40.0).exp()) / SAMPLE_RATE as f32;
        let boom = phase.sin() * (-t * boom_decay).exp() * (t * 2000.0).min(1.0);
        let bodies = body.run(noise) * (-t * body_decay).exp();
        let working = action.map_or(0.0, |(at, pitch, gain)| knock(t, at, 140.0, pitch, noise) * gain);
        let rolling = tail.iter_mut().fold(noise, |x, filter| filter.run(x));
        let swell = 1.0 + 0.35 * (TAU * roll_rate * t + roll_phase).sin();
        let tails = rolling * (t / 0.03).min(1.0) * (-t * tail_decay).exp() * swell;
        let shot = blast * blast_gain
            + cracking * crack_gain
            + boom * boom_gain
            + bodies * body_gain
            + working
            + tails * tail_gain;
        (shot * report.drive).tanh()
    });
    finish(&mut sound, if far { 0.8 } else { report.level });
    sound
}

/// A bolt lifted and drawn back: the handle knocks up, the bolt slides back
/// to a hard stop, and the empty case rings out as it is thrown and lands.
fn synth_bolt_back() -> Vec<f32> {
    let mut rng = 0x3a6_0006;
    let mut low_passed = 0.0;
    let mut scrape = Biquad::new(Pass::Band, 1900.0, 1.2);
    // A brass case rings at a few unrelated pitches (frequency, decay).
    const CASE: [(f32, f32); 3] = [(3650.0, 30.0), (5480.0, 38.0), (7320.0, 45.0)];
    // (when, how hard): thrown out, then hitting the ground and bouncing.
    const RINGS: [(f32, f32); 4] = [(0.17, 0.2), (0.46, 0.13), (0.53, 0.06), (0.57, 0.03)];
    samples(0.7, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let lift = knock(t, 0.0, 300.0, 2600.0, noise) * 0.6 + knock(t, 0.012, 120.0, 700.0, low_passed * 1.5) * 0.5;
        let back = BOLT_LIFT_TIME + BOLT_DRAW_TIME;
        let drawing = if (BOLT_LIFT_TIME..back).contains(&t) {
            let along = (t - BOLT_LIFT_TIME) / BOLT_DRAW_TIME;
            scrape.run(noise) * (along * std::f32::consts::PI).sin() * 0.7
        } else {
            scrape.run(0.0)
        };
        let stop = knock(t, back, 150.0, 1500.0, noise);
        let ring: f32 = RINGS
            .iter()
            .filter(|(at, _)| t >= *at)
            .map(|(at, hard)| {
                let since = t - at;
                let tone: f32 = CASE
                    .iter()
                    .map(|(frequency, decay)| (TAU * frequency * since).sin() * (-since * decay).exp())
                    .sum();
                tone * hard * (since * 3000.0).min(1.0)
            })
            .sum();
        ((lift + drawing + stop + ring) * 1.5).tanh() * 0.85
    })
}

/// A bolt pushed home: it slides forward, chambers the round with a dull
/// knock, and locks down with a sharp click `BOLT_LOCK` seconds in.
fn synth_bolt_home() -> Vec<f32> {
    let mut rng = 0x3a6_0007;
    let mut low_passed = 0.0;
    let mut scrape = Biquad::new(Pass::Band, 1600.0, 1.2);
    samples(0.24, |t| {
        let noise = next_random(&mut rng) * 2.0 - 1.0;
        low_passed += (noise - low_passed) * 0.2;
        let pushing = if t < BOLT_PUSH_TIME {
            scrape.run(noise) * (t / BOLT_PUSH_TIME * std::f32::consts::PI).sin() * 0.6
        } else {
            scrape.run(0.0)
        };
        let chamber = knock(t, BOLT_PUSH_TIME, 110.0, 800.0, low_passed * 1.5) * 0.8;
        let lock = knock(t, BOLT_LOCK, 200.0, 2200.0, noise) + knock(t, BOLT_LOCK + 0.004, 160.0, 3300.0, 0.0) * 0.5;
        ((pushing + chamber + lock) * 1.6).tanh() * 0.9
    })
}

// ---------------------------------------------------------------- footsteps

/// Splashes keep little above this, and steps on dry ground less still (see
/// `Tread::cutoff`): boots, clothes and the way to the ears soak up the top end.
const FOOTSTEP_CUTOFF: f32 = 3200.0;
/// And little below this: the rumble of a step goes through the ground, not
/// the air.
const FOOTSTEP_RUMBLE: f32 = 110.0;
/// Different takes of each footstep, and of each landing.
const FOOTSTEP_TAKES: u32 = 8;
const LANDING_TAKES: u32 = 3;

/// What a foot comes down on, which decides how the step sounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// Concrete, asphalt, floors, roofs: anything built.
    Hard,
    /// Grass, fresh or dry.
    Grass,
    /// Bare, packed earth.
    Dirt,
    /// Loose stones and rock.
    Gravel,
    /// Wading through the creek.
    Water,
}

impl Surface {
    const COUNT: usize = 5;
    const ALL: [Surface; Surface::COUNT] =
        [Surface::Hard, Surface::Grass, Surface::Dirt, Surface::Gravel, Surface::Water];
}

/// What the feet at `feet` are standing on. The map does not say, so it is
/// worked out: wading if they are below the creek's surface, hard if they
/// are on a block or a paved road, and on the bare ground its colour tells
/// grass (green) from earth (brown) and stones (light grey).
pub fn surface_under(map: &Map, feet: Vec3) -> Surface {
    let at = Vec2::new(feet.x, feet.z);
    let wading = map.water.iter().any(|(points, width)| {
        points.windows(2).any(|pair| {
            let (a, b) = (pair[0], pair[1]);
            let (distance, along) = nearest_on_segment(at, Vec2::new(a.x, a.z), Vec2::new(b.x, b.z));
            distance < width / 2.0 && feet.y < a.y + (b.y - a.y) * along + 0.05
        })
    });
    if wading {
        return Surface::Water;
    }
    let Some(terrain) = &map.terrain else {
        return Surface::Hard;
    };
    if feet.y - terrain.height(at.x, at.y) > 0.15 {
        return Surface::Hard;
    }
    let paved = map.roads.iter().any(|road| {
        road.paved
            && road
                .points
                .windows(2)
                .any(|pair| nearest_on_segment(at, pair[0], pair[1]).0 < road.width / 2.0)
    });
    if paved {
        return Surface::Hard;
    }
    let Some([r, g, b]) = terrain.color(at.x, at.y) else {
        return Surface::Grass;
    };
    if g - (r + b) / 2.0 > 0.07 {
        Surface::Grass
    } else if r - b < 0.12 && g > 0.35 {
        // Grey and light: stones. Dark mud is grey too.
        Surface::Gravel
    } else {
        Surface::Dirt
    }
}

/// Distance from `at` to the segment from `a` to `b`, and how far along it
/// (0..1) the nearest point is.
fn nearest_on_segment(at: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let along = ((at - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    (at.distance(a + ab * along), along)
}

/// How a step on one surface is put together. Each part of the foot that
/// meets the ground (heel, then the ball of the foot) makes a tap, a thud
/// and a thump, and crushes or brushes grains of the surface.
struct Tread {
    length: f32,
    /// Seconds the sole takes to meet the ground: soft ground gives.
    attack: f32,
    /// The sharp tap of the sole: band-passed noise (frequency, q, decay per second, gain).
    tap: (f32, f32, f32, f32),
    /// The weight coming down: low-passed noise (cutoff, decay, gain).
    thud: (f32, f32, f32),
    /// The ground and the leg taking the blow: a low, damped sine (frequency, decay, gain).
    thump: (f32, f32, f32),
    /// Grains per second at the peak, and seconds they keep coming.
    grain_rate: f32,
    grain_time: f32,
    /// Lowest and highest pitch of a grain, how sharply it rings, how fast it dies, and its gain.
    grain_pitch: (f32, f32),
    grain_q: f32,
    grain_decay: f32,
    grain_gain: f32,
    /// The sole sliding as it rolls off: band-passed noise (frequency, gain).
    scuff: (f32, f32),
    /// Everything above this is taken out (see `muffle`).
    cutoff: f32,
    /// Peak level, so that the surfaces sit together: hard ground is loud,
    /// grass quiet.
    level: f32,
}

fn tread(surface: Surface) -> Tread {
    // A boot's rubber sole, heard from head height: mostly a dull knock in
    // the low middle, short and dry, with only a little grit on top. What
    // would make a step sound wet is left out: no hiss trailing after the
    // blow, and no grains that ring at a pitch like drops.
    match surface {
        Surface::Hard => Tread {
            length: 0.16,
            attack: 0.002,
            tap: (1050.0, 0.8, 330.0, 1.0),
            thud: (600.0, 115.0, 2.0),
            thump: (135.0, 85.0, 0.14),
            grain_rate: 500.0,
            grain_time: 0.012,
            grain_pitch: (1400.0, 3000.0),
            grain_q: 1.5,
            grain_decay: 2500.0,
            grain_gain: 0.5,
            scuff: (1700.0, 0.04),
            cutoff: 2400.0,
            level: 0.8,
        },
        // Dry blades brushed and snapped: few enough short ticks of noise
        // to be told apart (more of them run into a hiss), and not ringing
        // ones (which sound like bubbles).
        Surface::Grass => Tread {
            length: 0.18,
            attack: 0.006,
            tap: (600.0, 0.8, 200.0, 0.2),
            thud: (450.0, 75.0, 2.4),
            thump: (95.0, 60.0, 0.14),
            grain_rate: 1800.0,
            grain_time: 0.032,
            grain_pitch: (1300.0, 3600.0),
            grain_q: 1.0,
            grain_decay: 2600.0,
            grain_gain: 1.0,
            scuff: (2000.0, 0.07),
            cutoff: 3400.0,
            level: 0.47,
        },
        Surface::Dirt => Tread {
            length: 0.18,
            attack: 0.004,
            tap: (800.0, 0.8, 240.0, 0.4),
            thud: (520.0, 85.0, 2.6),
            thump: (105.0, 65.0, 0.14),
            grain_rate: 900.0,
            grain_time: 0.02,
            grain_pitch: (1000.0, 2800.0),
            grain_q: 1.5,
            grain_decay: 2200.0,
            grain_gain: 0.9,
            scuff: (1600.0, 0.06),
            cutoff: 2600.0,
            level: 0.55,
        },
        // Stones knocking together: the only grains with a little pitch.
        Surface::Gravel => Tread {
            length: 0.2,
            attack: 0.003,
            tap: (1000.0, 1.0, 260.0, 0.3),
            thud: (560.0, 85.0, 2.0),
            thump: (105.0, 65.0, 0.12),
            grain_rate: 2000.0,
            grain_time: 0.034,
            grain_pitch: (600.0, 2200.0),
            grain_q: 2.5,
            grain_decay: 1500.0,
            grain_gain: 1.3,
            scuff: (1600.0, 0.05),
            cutoff: 2800.0,
            level: 0.5,
        },
        Surface::Water => unreachable!("water splashes instead"),
    }
}

/// One step on `surface`, or with `landing` both feet hitting the ground
/// at once. Each take is shaped a little differently.
fn synth_footstep(surface: Surface, take: u32, landing: bool) -> Vec<f32> {
    if surface == Surface::Water {
        return synth_splash(take, landing);
    }
    let mut rng = 0x1234_5678 ^ (surface as u32 + 1).wrapping_mul(0x9e37_79b9) ^ (take * 2 + landing as u32 + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let tread = tread(surface);
    // Scales a number by up to `spread` either way.
    let mut vary = |value: f32, spread: f32| value * (1.0 + spread * (random() * 2.0 - 1.0));

    // (start, gain, how much brighter): heel, then the ball of the foot.
    // Running lands on the front of the foot, the heel only just ahead: far
    // enough apart to give the step a shape, close enough to be one blow.
    let roll = vary(0.016, 0.3);
    let mut contacts = vec![(0.002, 1.0, 1.0), (0.002 + roll, vary(0.4, 0.25), vary(1.2, 0.1))];
    if landing {
        // The other foot, a moment later, and it lands flatter.
        let other = vary(0.012, 0.4);
        contacts.push((0.002 + other, vary(0.85, 0.1), 0.9));
        contacts.push((0.002 + other + roll * 0.6, vary(0.4, 0.2), 1.1));
    }
    let weight: f32 = if landing { 1.4 } else { 1.0 };
    let (tap_frequency, tap_q, tap_decay, tap_gain) = tread.tap;
    let (thud_cutoff, thud_decay, thud_gain) = tread.thud;
    let (thump_frequency, thump_decay, thump_gain) = tread.thump;
    let thump_frequency = vary(thump_frequency, 0.12) / weight.sqrt();
    let mut tap = Biquad::new(Pass::Band, vary(tap_frequency, 0.15), tap_q);
    let mut thud = Biquad::new(Pass::Low, vary(thud_cutoff, 0.15), 0.7);
    let mut scuff = Biquad::new(Pass::Band, vary(tread.scuff.0, 0.2), 0.8);
    let scuff_gain = vary(tread.scuff.1, 0.4);
    let grain_rate = vary(tread.grain_rate, 0.25) * weight;
    let grain_time = vary(tread.grain_time, 0.25) * weight.sqrt();
    let (low, high) = tread.grain_pitch;
    let mut grains = Grains::new(tread.grain_decay);
    // Webbing and kit rattling as we land.
    let mut rattle = Grains::new(250.0);
    // (sample it starts on, pitch)
    let rattles: Vec<(usize, f32)> = (0..if landing { 5 } else { 0 })
        .map(|_| (((0.01 + random() * 0.07) * SAMPLE_RATE as f32) as usize, 2600.0 + random() * 3800.0))
        .collect();
    let mut thump_phases = vec![0.0_f32; contacts.len()];
    let mut noise_rng = rng ^ 0x5bd1_e995;

    let mut sound = samples(tread.length, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let chance = next_random(&mut noise_rng);
        let (mut taps, mut thuds, mut thumps, mut density) = (0.0, 0.0, 0.0, 0.0);
        for ((at, gain, brightness), phase) in contacts.iter().zip(&mut thump_phases) {
            if t < *at {
                continue;
            }
            let since = t - at;
            let onset = (since / tread.attack).min(1.0);
            taps += gain * onset * (-since * tap_decay * brightness).exp();
            thuds += gain * onset * (-since * thud_decay / weight).exp();
            // The thump drops in pitch as the blow is taken up.
            *phase += TAU * thump_frequency * (1.0 + 0.25 * (-since * 70.0).exp()) / SAMPLE_RATE as f32;
            thumps += gain * onset * phase.sin() * (-since * thump_decay / weight).exp();
            density += gain * (since / 0.004).min(1.0) * (-since / grain_time).exp();
        }
        if chance < grain_rate * density / SAMPLE_RATE as f32 {
            // Mostly faint ticks, now and then a loud one; the loud ones
            // right at the blow; late ones on their own sound like echoes.
            let size = next_random(&mut noise_rng).powi(3) * density.min(1.0);
            let pitch = low * (high / low).powf(next_random(&mut noise_rng));
            grains.add(pitch, tread.grain_q, size);
        }
        let index = (t * SAMPLE_RATE as f32).round() as usize;
        for (at, pitch) in &rattles {
            if *at == index {
                rattle.add(*pitch, 30.0, 0.3);
            }
        }
        let scuffs = contacts.get(1).map_or(0.0, |(at, ..)| {
            let since = t - at;
            // Brief: a scuff that lingers after the blow is a squelch.
            if since < 0.0 { 0.0 } else { (since / 0.006).min(1.0) * (-since * 120.0).exp() }
        });
        tap.run(noise * taps) * tap_gain
            + thud.run(noise * thuds) * thud_gain
            + thumps * thump_gain
            + grains.run(noise) * tread.grain_gain * tread.grain_q.sqrt()
            + scuff.run(noise * scuffs) * scuff_gain
            + rattle.run(noise) * 3.0
    });
    muffle(&mut sound, tread.cutoff);
    thin(&mut sound, FOOTSTEP_RUMBLE);
    finish(&mut sound, tread.level);
    sound
}

/// A foot coming down in shallow water: the slosh of the water pushed
/// aside, a spray, and the bubbles it traps ringing out as they rise, then
/// a few drops falling back.
fn synth_splash(take: u32, landing: bool) -> Vec<f32> {
    let mut rng = 0x00a9_7e12 ^ (take * 2 + landing as u32 + 1).wrapping_mul(7919);
    let mut random = || next_random(&mut rng);
    let size = if landing { 1.5 } else { 1.0 };
    // (start, pitch, gain). A bubble's pitch is set by its size: big ones
    // are deep and ring longer.
    let mut bubbles: Vec<(f32, f32, f32)> = (0..if landing { 26 } else { 14 })
        .map(|_| {
            let pitch = 350.0 * (2200.0_f32 / 350.0).powf(random());
            (0.008 + random().powi(2) * 0.2 * size, pitch, (0.3 + 0.7 * random()) * (600.0 / pitch).sqrt())
        })
        .collect();
    bubbles.extend((0..if landing { 7 } else { 4 }).map(|_| {
        (0.16 + random() * 0.26 * size, 1400.0 + random() * 2400.0, 0.25 + 0.3 * random())
    }));
    let mut slosh = Biquad::new(Pass::Low, 420.0 + random() * 160.0, 0.9);
    let mut spray = Biquad::new(Pass::Band, 2400.0 + random() * 1200.0, 0.6);
    let mut thud = Biquad::new(Pass::Low, 160.0, 0.7);
    let mut phases = vec![0.0_f32; bubbles.len()];
    let mut noise_rng = rng ^ 0x5bd1_e995;
    let mut sound = samples(0.5 * size, |t| {
        let noise = next_random(&mut noise_rng) * 2.0 - 1.0;
        let push = (t / 0.012).min(1.0) * (-t * 13.0 / size).exp();
        let spraying = (t / 0.004).min(1.0) * (-t * 22.0 / size).exp();
        let blow = (t / 0.002).min(1.0) * (-t * 35.0).exp();
        let mut out = slosh.run(noise * push) * 3.0 + spray.run(noise * spraying) * 0.9 + thud.run(noise * blow) * 4.0 * size;
        for ((at, pitch, gain), phase) in bubbles.iter().zip(&mut phases) {
            if t < *at {
                continue;
            }
            let since = t - at;
            // Minnaert resonance, damped and rising as the bubble shrinks
            // (van den Doel's model).
            let damping = 0.13 * pitch + 0.0072 * pitch.powf(1.5);
            *phase += TAU * pitch * (1.0 + 0.1 * damping * since) / SAMPLE_RATE as f32;
            out += phase.sin() * (-damping * since).exp() * (since * 4000.0).min(1.0) * gain;
        }
        out
    });
    muffle(&mut sound, FOOTSTEP_CUTOFF);
    finish(&mut sound, if landing { 0.64 } else { 0.57 });
    sound
}

#[derive(Clone, Copy)]
enum Pass {
    Low,
    Band,
}

/// A two-pole filter (from the Audio EQ Cookbook). The band pass peaks at 1.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
}

impl Biquad {
    fn new(pass: Pass, frequency: f32, q: f32) -> Biquad {
        let w = TAU * frequency.min(SAMPLE_RATE as f32 * 0.45) / SAMPLE_RATE as f32;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let b = match pass {
            Pass::Low => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0],
            Pass::Band => [alpha, 0.0, -alpha],
        };
        let a0 = 1.0 + alpha;
        Biquad {
            b: b.map(|b| b / a0),
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    /// Moves the filter to `frequency`, keeping what it is ringing with.
    fn retune(&mut self, pass: Pass, frequency: f32, q: f32) {
        let fresh = Biquad::new(pass, frequency, q);
        (self.b, self.a) = (fresh.b, fresh.a);
    }

    fn run(&mut self, input: f32) -> f32 {
        let output = self.b[0] * input + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [input, self.x[0]];
        self.y = [output, self.y[0]];
        output
    }
}

/// Many tiny impacts, each a burst of noise ringing a filter of its own:
/// stones knocking, grit crushed, blades of grass flicked (Perry Cook's
/// PhISEM, more or less).
struct Grains {
    live: Vec<(Biquad, f32)>,
    /// How much a grain's burst keeps each sample.
    keep: f32,
}

impl Grains {
    /// Grains whose bursts die away at `decay` per second.
    fn new(decay: f32) -> Grains {
        Grains {
            live: Vec::new(),
            keep: (-decay / SAMPLE_RATE as f32).exp(),
        }
    }

    fn add(&mut self, pitch: f32, q: f32, size: f32) {
        self.live.push((Biquad::new(Pass::Band, pitch, q), size));
    }

    fn run(&mut self, noise: f32) -> f32 {
        let mut out = 0.0;
        for (filter, burst) in &mut self.live {
            out += filter.run(noise * *burst);
            *burst *= self.keep;
        }
        self.live.retain(|(_, burst)| *burst > 1e-4);
        out
    }
}

/// Takes off the top end: a steep low pass at `cutoff`.
fn muffle(sound: &mut [f32], cutoff: f32) {
    let mut filters = [Biquad::new(Pass::Low, cutoff, 0.707); 2];
    for sample in sound.iter_mut() {
        for filter in &mut filters {
            *sample = filter.run(*sample);
        }
    }
}

/// Takes off the bottom end: a gentle high pass at `cutoff`.
fn thin(sound: &mut [f32], cutoff: f32) {
    let keep = 1.0 / (1.0 + TAU * cutoff / SAMPLE_RATE as f32);
    let (mut last_in, mut last_out) = (0.0, 0.0);
    for sample in sound.iter_mut() {
        last_out = keep * (last_out + *sample - last_in);
        last_in = *sample;
        *sample = last_out;
    }
}

/// Fades the tail to exactly zero and scales the loudest sample to `peak`.
fn finish(sound: &mut [f32], peak: f32) {
    let count = sound.len();
    for (i, sample) in sound.iter_mut().enumerate() {
        *sample *= ((count - 1 - i) as f32 / 400.0).min(1.0);
    }
    let loudest = sound.iter().fold(0.0_f32, |loudest, s| loudest.max(s.abs()));
    if loudest > 0.0 {
        for sample in sound.iter_mut() {
            *sample *= peak / loudest;
        }
    }
}

fn samples(seconds: f32, mut wave: impl FnMut(f32) -> f32) -> Vec<f32> {
    let count = (seconds * SAMPLE_RATE as f32) as usize;
    (0..count)
        .map(|i| {
            // Fade the tail to exactly zero.
            let fade_out = ((count - 1 - i) as f32 / 400.0).min(1.0);
            wave(i as f32 / SAMPLE_RATE as f32) * fade_out
        })
        .collect()
}

/// Wraps mono samples in a 16-bit PCM WAV file, for listening to them
/// outside the game.
#[cfg(test)]
fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    wav_bytes_in(samples, 1)
}

/// The same for samples of `channels` interleaved channels.
#[cfg(test)]
fn wav_bytes_in(samples: &[f32], channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes()); // format chunk size
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2 * channels as u32).to_le_bytes()); // bytes per second
    bytes.extend_from_slice(&(2 * channels).to_le_bytes()); // bytes per frame
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// xorshift32, returned as a float in 0..1.
fn next_random(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state >> 8) as f32 / (1u32 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ears at the origin, turned `yaw` radians left from looking down -Z.
    fn ears(yaw: f32) -> GlobalTransform {
        GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(yaw)))
    }

    #[test]
    fn sounds_are_louder_in_the_ear_facing_them() {
        let right = placement(&ears(0.0), Vec3::X * 5.0, 10.0);
        let left = placement(&ears(0.0), Vec3::NEG_X * 5.0, 10.0);
        assert!(right.right > 3.5 * right.left, "{right:?}");
        assert!(left.left > 3.5 * left.right, "{left:?}");
        // Turned to face it, it is ahead.
        let faced = placement(&ears(-std::f32::consts::FRAC_PI_2), Vec3::X * 5.0, 10.0);
        assert!((faced.left - faced.right).abs() < 1e-4, "{faced:?}");
        // The same power wherever it is round us.
        for heard in [right, left, faced] {
            assert!((heard.left.powi(2) + heard.right.powi(2) - 1.0).abs() < 1e-4, "{heard:?}");
        }
    }

    #[test]
    fn sounds_behind_are_duller_and_ahead_are_not() {
        let ahead = placement(&ears(0.0), Vec3::NEG_Z * 5.0, 10.0);
        let behind = placement(&ears(0.0), Vec3::Z * 5.0, 10.0);
        let side = placement(&ears(0.0), Vec3::X * 5.0, 10.0);
        assert_eq!(ahead.muffle, 0.0);
        assert_eq!(side.muffle, 0.0);
        assert!((behind.muffle - BEHIND_MUFFLE).abs() < 1e-4);
        assert!((ahead.left - behind.left).abs() < 1e-4 && (ahead.right - behind.right).abs() < 1e-4);
        // Treble is what goes.
        let hiss = |at: Vec3| {
            let panner = Arc::new(Panner::new(placement(&ears(0.0), at, 10.0)));
            let clip = Clip {
                samples: samples(0.2, |t| (TAU * 6_000.0 * t).sin()).into(),
                panner: Some(panner),
            };
            clip.decoder().skip(2_000).fold(0.0_f32, |loudest, s| loudest.max(s.abs() as f32))
        };
        assert!(hiss(Vec3::Z * 5.0) < 0.6 * hiss(Vec3::NEG_Z * 5.0));
    }

    #[test]
    fn sounds_fade_past_their_range() {
        let near = placement(&ears(0.0), Vec3::NEG_Z * 10.0, 10.0);
        let far = placement(&ears(0.0), Vec3::NEG_Z * 20.0, 10.0);
        assert!((far.left / near.left - 0.25).abs() < 1e-4);
        // Right on top of us: no side, no muffle, full volume.
        let close = placement(&ears(0.0), Vec3::new(0.0, -0.01, 0.01), 4.0);
        assert!((close.left - close.right).abs() < 0.05 && close.muffle < 0.05, "{close:?}");
    }

    #[test]
    fn clips_play_left_then_right_and_ours_as_they_are() {
        let sound: Arc<[f32]> = vec![0.5; 100].into();
        let ours = Clip {
            samples: sound.clone(),
            panner: None,
        }
        .decoder();
        assert_eq!(ours.channels().get(), 1);
        assert_eq!(ours.collect::<Vec<_>>(), vec![0.5 as Sample; 100]);

        let on_the_right = Arc::new(Panner::new(placement(&ears(0.0), Vec3::X * 5.0, 10.0)));
        let theirs = Clip {
            samples: sound,
            panner: Some(on_the_right.clone()),
        }
        .decoder();
        assert_eq!(theirs.channels().get(), 2);
        assert_eq!(theirs.size_hint(), (200, Some(200)));
        let played: Vec<_> = theirs.collect();
        assert_eq!(played.len(), 200);
        let (left, right) = (played[0] as f32, played[1] as f32);
        assert!(right > 3.5 * left && left > 0.0, "left {left} right {right}");
    }

    /// For headphones: `cargo test write_directions -- --ignored`, then play
    /// `target/sounds/directions.wav`. A rifle shot 8 m away comes from the
    /// right, the left, ahead and behind, one second apart, then footsteps
    /// walk once round us.
    #[test]
    #[ignore]
    fn write_directions() {
        let dir = std::env::var("SOUND_DIR").unwrap_or_else(|_| "target/sounds".into());
        std::fs::create_dir_all(&dir).unwrap();
        let rate = SAMPLE_RATE as usize;
        let heard = |sound: Vec<f32>, at: Vec3, range: f32| -> Vec<f32> {
            let clip = Clip {
                samples: sound.into(),
                panner: Some(Arc::new(Panner::new(placement(&ears(0.0), at, range)))),
            };
            clip.decoder().map(|sample| sample as f32).collect()
        };
        let mut out = vec![0.0_f32; 2 * rate * 10];
        let mut mix = |stereo: &[f32], at_seconds: f32| {
            let start = 2 * (at_seconds * rate as f32) as usize;
            for (i, sample) in stereo.iter().enumerate() {
                if let Some(slot) = out.get_mut(start + i) {
                    *slot += sample;
                }
            }
        };
        for (second, at) in [Vec3::X, Vec3::NEG_X, Vec3::NEG_Z, Vec3::Z].into_iter().enumerate() {
            mix(&heard(synth_gunshot(RIFLE, 0, false), at * 8.0, GUNSHOT_FULL_VOLUME_RANGE), second as f32);
        }
        // Round us clockwise from ahead, a step every 0.3 s.
        for step in 0..16 {
            let angle = step as f32 / 16.0 * TAU;
            let at = Vec3::new(angle.sin(), -1.6, -angle.cos()) * 3.0;
            let sound = synth_footstep(Surface::ALL[0], step % FOOTSTEP_TAKES, false);
            mix(&heard(sound, at, FOOTSTEP_FULL_VOLUME_RANGE), 4.5 + step as f32 * 0.3);
        }
        std::fs::write(format!("{dir}/directions.wav"), wav_bytes_in(&out, 2)).unwrap();
    }

    #[test]
    fn a_playing_sound_follows_us_turning() {
        let panner = Arc::new(Panner::new(placement(&ears(0.0), Vec3::X * 5.0, 10.0)));
        let mut player = Clip {
            samples: vec![0.5; SAMPLE_RATE as usize].into(),
            panner: Some(panner.clone()),
        }
        .decoder();
        let frame = |player: &mut ClipPlayer| (player.next().unwrap() as f32, player.next().unwrap() as f32);
        let (left, right) = frame(&mut player);
        assert!(right > left);
        // We turn round: it is on our left now.
        panner.set(placement(&ears(std::f32::consts::PI), Vec3::X * 5.0, 10.0));
        // Not at once, which would click...
        let (left, right) = frame(&mut player);
        assert!(right > left);
        // ...but within a few hundredths of a second.
        for _ in 0..(0.05 * SAMPLE_RATE as f32) as usize {
            frame(&mut player);
        }
        let (left, right) = frame(&mut player);
        assert!(left > 3.5 * right, "left {left} right {right}");
    }

    #[test]
    fn sounds_are_audible_and_do_not_clip() {
        let mut all = vec![
            synth_bolt_back(),
            synth_bolt_home(),
            synth_pin(),
            synth_throw(),
            synth_smoke(),
            synth_tick(),
            synth_chime(),
            synth_shatter(),
            synth_charge(),
            synth_ring(),
            synth_click(),
            synth_handling(true),
            synth_handling(false),
            synth_shell_in(),
            synth_pump(),
        ];
        for weapon_id in 0..GUN_COUNT {
            all.extend((0..GUNSHOT_TAKES).map(|take| synth_gunshot(weapon_id, take, false)));
            all.extend((0..DISTANT_SHOT_TAKES).map(|take| synth_gunshot(weapon_id, take, true)));
            all.extend([synth_mag_out(weapon_id), synth_mag_in(weapon_id), synth_bolt_release(weapon_id)]);
        }
        for surface in Surface::ALL {
            all.extend((0..MAG_DROP_TAKES).map(|take| synth_mag_drop(surface, take)));
            all.extend((0..BOUNCE_TAKES).map(|take| synth_bounce(surface, take)));
            all.extend((0..ROLL_TAKES).map(|take| synth_roll(surface, take)));
        }
        for far in [false, true] {
            all.extend((0..BLAST_TAKES).map(|take| synth_explosion(take, far)));
            all.extend((0..BLAST_TAKES).map(|take| synth_flashbang(take, far)));
        }
        for surface in Surface::ALL {
            all.extend((0..FOOTSTEP_TAKES).map(|take| synth_footstep(surface, take, false)));
            all.extend((0..LANDING_TAKES).map(|take| synth_footstep(surface, take, true)));
        }
        for sound in all {
            let peak = sound.iter().fold(0.0_f32, |peak, s| peak.max(s.abs()));
            assert!(peak > 0.3 && peak <= 1.0, "peak {peak}");
            assert!(sound.iter().all(|s| s.is_finite()));
            assert_eq!(*sound.last().unwrap(), 0.0);
            assert_eq!(wav_bytes(&sound).len(), 44 + sound.len() * 2);
        }
    }

    /// Each map is walked on what it looks like: built maps are hard all
    /// over, the valley is mostly grass with its creek and roads, the
    /// hamlet earth and stones.
    #[test]
    fn maps_have_their_surfaces() {
        for (id, expected) in [
            (0, vec![Surface::Hard]),
            (1, vec![Surface::Hard]),
            (2, vec![Surface::Grass, Surface::Dirt, Surface::Gravel, Surface::Water, Surface::Hard]),
            (3, vec![Surface::Dirt, Surface::Gravel]),
        ] {
            let map = Map::by_id(id);
            let [low, high] = map.bounds;
            let mut found = Vec::new();
            for i in 0..60 {
                for j in 0..60 {
                    let at = low + (high - low) * Vec2::new(i as f32, j as f32) / 59.0;
                    let feet = Vec3::new(at.x, map.ground(at.x, at.y).unwrap_or(0.0), at.y);
                    let surface = surface_under(&map, feet);
                    if !found.contains(&surface) {
                        found.push(surface);
                    }
                }
            }
            found.sort_by_key(|surface| *surface as u8);
            let mut expected = expected;
            expected.sort_by_key(|surface| *surface as u8);
            assert_eq!(found, expected, "map {id}");
        }
    }

    /// Writes the guns out to listen to: each take of each shot, the bolt
    /// and pump, and each gun fired at its own pace with what is heard in
    /// between, at the levels they are played at. `cargo test write_guns --
    /// -- --ignored`, into `target/sounds` (or `$SOUND_DIR`).
    #[test]
    #[ignore]
    fn write_guns() {
        use crate::{player::{BOLT_BACK, PUMP_AFTER, PUMP_TIME}, weapons::weapon};
        let dir = std::env::var("SOUND_DIR").unwrap_or_else(|_| "target/sounds".into());
        std::fs::create_dir_all(&dir).unwrap();
        let rate = SAMPLE_RATE as f32;
        for (name, sound) in [("bolt_back", synth_bolt_back()), ("bolt_home", synth_bolt_home()), ("pump", synth_pump())] {
            std::fs::write(format!("{dir}/{name}.wav"), wav_bytes(&sound)).unwrap();
        }
        for weapon_id in 0..GUN_COUNT {
            let stats = weapon(weapon_id);
            let name = stats.name.to_lowercase();
            let takes: Vec<_> = (0..GUNSHOT_TAKES).map(|take| synth_gunshot(weapon_id, take, false)).collect();
            let distant = synth_gunshot(weapon_id, 0, true);
            std::fs::write(format!("{dir}/shot_{name}_far.wav"), wav_bytes(&distant)).unwrap();
            // A shot heard from 0, 50 and 100 metres, blended as in the game
            // without the fall off in loudness.
            for metres in [0.0, 50.0, 100.0] {
                let far = smoothstep(NEAR_SHOT, FAR_SHOT, metres);
                let mut heard = vec![0.0; distant.len()];
                for (i, s) in takes[0].iter().enumerate() {
                    heard[i] += s * (1.0 - far);
                }
                for (i, s) in distant.iter().enumerate() {
                    heard[i] += s * far * DISTANT_BOOST;
                }
                std::fs::write(format!("{dir}/shot_{name}_at_{metres}m.wav"), wav_bytes(&heard)).unwrap();
            }
            for (take, sound) in takes.iter().enumerate() {
                std::fs::write(format!("{dir}/shot_{name}_{take}.wav"), wav_bytes(sound)).unwrap();
            }
            let shots = if stats.auto { 12 } else { 4 };
            let mut fired = vec![0.0; ((shots as f32 * stats.interval + 2.0) * rate) as usize];
            let mut put = |at: f32, sound: &[f32], volume: f32| {
                for (i, s) in sound.iter().enumerate() {
                    fired[(at * rate) as usize + i] += s * volume;
                }
            };
            for shot in 0..shots {
                let at = shot as f32 * stats.interval;
                put(at, &takes[shot % takes.len()], 0.5);
                if shot + 1 < shots {
                    match stats.cycle {
                        Some(crate::weapons::Cycle::Pump) => {
                            put(at + PUMP_AFTER + PUMP_TIME / 2.0 - PUMP_CLACK, &synth_pump(), 0.75)
                        }
                        Some(crate::weapons::Cycle::Bolt) => {
                            put(at + BOLT_BACK, &synth_bolt_back(), 0.45);
                            put(at + stats.interval - BOLT_LOCK, &synth_bolt_home(), 0.6);
                        }
                        None => {}
                    }
                }
            }
            std::fs::write(format!("{dir}/firing_{name}.wav"), wav_bytes(&fired)).unwrap();
        }
    }

    /// Writes the reloads out to listen to: each gun's whole reload from
    /// empty at its own pace (the shotgun three rounds and the rack), the
    /// draw, and a magazine landing on each surface. `cargo test
    /// write_reloads -- --ignored`, into `target/sounds` (or `$SOUND_DIR`).
    #[test]
    #[ignore]
    fn write_reloads() {
        use crate::{
            player::{BOLT_HOME, BOLT_PULL, MAGAZINE_DROPPED, MAGAZINE_IN, MAGAZINE_OUT, RACK_TIME},
            weapons::weapon,
        };
        let dir = std::env::var("SOUND_DIR").unwrap_or_else(|_| "target/sounds".into());
        std::fs::create_dir_all(&dir).unwrap();
        let rate = SAMPLE_RATE as f32;
        std::fs::write(format!("{dir}/draw.wav"), wav_bytes(&synth_handling(true))).unwrap();
        for surface in Surface::ALL {
            let name = format!("{surface:?}").to_lowercase();
            for take in 0..MAG_DROP_TAKES {
                std::fs::write(format!("{dir}/mag_drop_{name}_{take}.wav"), wav_bytes(&synth_mag_drop(surface, take)))
                    .unwrap();
            }
        }
        for weapon_id in 0..GUN_COUNT {
            let stats = weapon(weapon_id);
            let mut heard = vec![0.0; ((stats.reload * 4.0 + 1.5) * rate) as usize];
            let mut put = |at: f32, sound: &[f32], volume: f32| {
                for (i, s) in sound.iter().enumerate() {
                    heard[(at * rate) as usize + i] += s * volume;
                }
            };
            put(0.0, &synth_handling(false), 0.35);
            if stats.reload_per_round {
                for round in 0..3 {
                    put((round as f32 + 0.55) * stats.reload, &synth_shell_in(), 0.5);
                }
                put(3.0 * stats.reload + 0.1 * RACK_TIME, &synth_pump(), 0.75);
            } else {
                let reload = stats.reload;
                put(MAGAZINE_OUT * reload, &synth_mag_out(weapon_id), 0.55);
                // It falls about a metre and a half.
                put(MAGAZINE_DROPPED * reload + 0.5, &synth_mag_drop(Surface::Hard, 0), 0.4);
                put(MAGAZINE_IN * reload, &synth_mag_in(weapon_id), 0.6);
                put(BOLT_PULL * reload, &synth_bolt_pull(weapon_id), 0.5);
                put(BOLT_HOME * reload, &synth_bolt_release(weapon_id), 0.6);
            }
            let name = stats.name.to_lowercase();
            std::fs::write(format!("{dir}/reload_{name}.wav"), wav_bytes(&heard)).unwrap();
            if !stats.reload_per_round {
                std::fs::write(format!("{dir}/mag_out_{name}.wav"), wav_bytes(&synth_mag_out(weapon_id))).unwrap();
                std::fs::write(format!("{dir}/mag_in_{name}.wav"), wav_bytes(&synth_mag_in(weapon_id))).unwrap();
                std::fs::write(format!("{dir}/bolt_pull_{name}.wav"), wav_bytes(&synth_bolt_pull(weapon_id))).unwrap();
                std::fs::write(format!("{dir}/bolt_{name}.wav"), wav_bytes(&synth_bolt_release(weapon_id))).unwrap();
            }
        }
        std::fs::write(format!("{dir}/shell_in.wav"), wav_bytes(&synth_shell_in())).unwrap();
        std::fs::write(format!("{dir}/pump.wav"), wav_bytes(&synth_pump())).unwrap();
    }

    /// Writes the grenades out to listen to: each sound, a frag cooked,
    /// thrown, bouncing on concrete and going off, and an explosion heard
    /// from 0, 40 and 80 metres (blended as in the game, without the fall
    /// off in loudness). `cargo test write_grenades -- --ignored`, into
    /// `target/sounds` (or `$SOUND_DIR`).
    #[test]
    #[ignore]
    fn write_grenades() {
        let dir = std::env::var("SOUND_DIR").unwrap_or_else(|_| "target/sounds".into());
        std::fs::create_dir_all(&dir).unwrap();
        let rate = SAMPLE_RATE as f32;
        let write = |name: &str, sound: &[f32]| std::fs::write(format!("{dir}/{name}.wav"), wav_bytes(sound)).unwrap();
        write("pin", &synth_pin());
        write("throw", &synth_throw());
        write("smoke", &synth_smoke());
        for far in [false, true] {
            let distance = if far { "far" } else { "near" };
            write(&format!("explosion_{distance}"), &synth_explosion(0, far));
            write(&format!("flashbang_{distance}"), &synth_flashbang(0, far));
        }
        for surface in Surface::ALL {
            let name = format!("{surface:?}").to_lowercase();
            write(&format!("bounce_{name}"), &synth_bounce(surface, 0));
            write(&format!("roll_{name}"), &synth_roll(surface, 0));
        }
        write("ringing", &synth_ring());
        let mut frag = vec![0.0; (6.0 * rate) as usize];
        let put = |into: &mut Vec<f32>, at: f32, sound: &[f32], volume: f32| {
            for (i, s) in sound.iter().enumerate() {
                into[(at * rate) as usize + i] += s * volume;
            }
        };
        put(&mut frag, 0.0, &synth_pin(), 0.45);
        put(&mut frag, 0.9, &synth_throw(), 0.5);
        for (n, (at, speed)) in [(1.6, 11.0), (1.95, 6.0), (2.15, 3.5)].into_iter().enumerate() {
            put(&mut frag, at, &synth_bounce(Surface::Hard, n as u32), (speed / 12.0_f32).clamp(0.25, 1.0) * 0.3);
        }
        put(&mut frag, 2.25, &synth_roll(Surface::Hard, 0), (4.0 / 6.0_f32).clamp(0.3, 1.0) * 0.3);
        put(&mut frag, 2.9, &synth_explosion(0, false), 1.0);
        write("frag", &frag);
        // Flashed while somebody keeps shooting nearby: the ringing drowns
        // them out, and they come back.
        let mut flashed = vec![0.0; (7.5 * rate) as usize];
        put(&mut flashed, 0.0, &synth_flashbang(0, false), 1.0);
        put(&mut flashed, 0.0, &synth_ring(), 0.45);
        let shot = synth_gunshot(RIFLE, 0, false);
        for n in 0..10 {
            let at = 0.4 + n as f32 * 0.65;
            put(&mut flashed, at, &shot, 0.5 * (1.0 - DEAFNESS * (1.0 - smoothstep(0.0, DEAF_TIME, at))));
        }
        write("flashed", &flashed);
        for metres in [0.0, 40.0, 80.0] {
            let far = smoothstep(NEAR_BLAST, FAR_BLAST, metres);
            let mut heard = vec![0.0; (3.6 * rate) as usize];
            put(&mut heard, 0.0, &synth_explosion(1, false), 1.0 - far);
            put(&mut heard, 0.0, &synth_explosion(1, true), far * DISTANT_BOOST);
            write(&format!("explosion_at_{metres}m"), &heard);
        }
    }

    /// Writes the footsteps out to listen to: each take, and a run of
    /// steps on each surface at running pace. `cargo test write_footsteps
    /// -- --ignored`, into `target/sounds` (or `$SOUND_DIR`).
    #[test]
    #[ignore]
    fn write_footsteps() {
        let dir = std::env::var("SOUND_DIR").unwrap_or_else(|_| "target/sounds".into());
        std::fs::create_dir_all(&dir).unwrap();
        // Seconds between steps when running.
        let pace = STRIDE / 9.5;
        let mut rng = 1;
        for surface in Surface::ALL {
            let name = format!("{surface:?}").to_lowercase();
            let takes: Vec<_> = (0..FOOTSTEP_TAKES).map(|take| synth_footstep(surface, take, false)).collect();
            for (take, sound) in takes.iter().enumerate() {
                std::fs::write(format!("{dir}/{name}_{take}.wav"), wav_bytes(sound)).unwrap();
            }
            for take in 0..LANDING_TAKES {
                let sound = synth_footstep(surface, take, true);
                std::fs::write(format!("{dir}/{name}_land_{take}.wav"), wav_bytes(&sound)).unwrap();
            }
            // Twelve steps, then a landing.
            let step = (pace * SAMPLE_RATE as f32) as usize;
            let mut run = vec![0.0; step * 14 + SAMPLE_RATE as usize];
            let mut last = 0;
            for n in 0..12 {
                let mut take = (next_random(&mut rng) * 997.0) as usize % takes.len();
                if take == last {
                    take = (take + 1) % takes.len();
                }
                last = take;
                let loudness = 0.85 + 0.15 * next_random(&mut rng);
                for (i, s) in takes[take].iter().enumerate() {
                    run[n * step + i] += s * loudness;
                }
            }
            for (i, s) in synth_footstep(surface, 0, true).iter().enumerate() {
                run[step * 13 + i] += s;
            }
            std::fs::write(format!("{dir}/run_{name}.wav"), wav_bytes(&run)).unwrap();
        }
    }
}
