//! The bots: what they see and hear, where they go, which gun they take and how
//! they fight. The server flies their bullets too, since they have no game to.

use super::*;

pub(crate) const BOT_NAMES: [&str; 8] = [
    "Clanker", "Tinhead", "Bolt", "Rusty", "Sprocket", "Gizmo", "Widget", "Scrap",
];

// How well bots fight. They see enemies in front of them, or close by, or
// whoever just hurt them; take a moment to react; turn at a human-ish rate;
// lead their shots, though not perfectly; and their aim wobbles, more so
// right after they find a target and while it darts across their view.
/// Bots notice enemies within this angle either side of where they look
/// (cosine of the angle)...
pub(crate) const BOT_VIEW_COS: f32 = 0.26;
/// ...or anywhere this close.
pub(crate) const BOT_HEARING: f32 = 6.0;
/// They see as far as there is a clear line, but fights are reckoned to go
/// no further than this when bots pick their guns.
pub(crate) const LONG_RANGE: f32 = 70.0;
/// Bots hear gunshots (not the knife), grenades going off and footsteps
/// (see `NoiseKind::reach`), through walls and all, and go and see what it
/// was, guessing where it came from: off by up to the first of these many
/// radians either side, and the second of the distance nearer or further.
/// Loud bangs are easy to place; footsteps are not.
pub(crate) const SHOT_GUESS: (f32, f32) = (0.09, 0.15);
pub(crate) const STEP_GUESS: (f32, f32) = (0.52, 0.4);
/// Seconds a noise can be heard for after it is made.
pub(crate) const NOISE_LIFETIME: f32 = 0.5;
/// Footsteps the way the clients play them (see `audio.rs`): one every
/// `STRIDE` metres running on the ground, faster than `STEP_MIN_SPEED`, not
/// crouched. Worked out from how far everyone moved every `STEP_SAMPLE`
/// seconds.
pub(crate) const STRIDE: f32 = 2.6;
pub(crate) const STEP_MIN_SPEED: f32 = 2.0;
pub(crate) const STEP_SAMPLE: f32 = 0.25;
/// Seconds a bot spends going to see what it heard, the first of them
/// turned towards it.
pub(crate) const BOT_INVESTIGATE_TIME: f32 = 10.0;
pub(crate) const BOT_GLANCE_TIME: f32 = 1.5;
/// Conquest: bots leave their flag only for noises this close.
pub(crate) const BOT_INVESTIGATE_NEAR: f32 = 30.0;
/// Radians per second, turning to look where it is going.
pub(crate) const BOT_TURN_RATE: f32 = 6.0;
/// Beyond this many metres, a far body is small and hard to make out (see
/// `BotSkill::spot_time` and `BotSkill::far_doubling`).
pub(crate) const BOT_FAR: f32 = 20.0;
/// Seconds over which a bot works out how fast its enemy is going.
pub(crate) const BOT_VEL_SMOOTHING: f32 = 0.15;

/// How well bots fight at one of the `BOT_DIFFICULTY_NAMES`.
pub(crate) struct BotSkill {
    /// Seconds an enemy must be in sight before the first shot: at least
    /// `reaction`, and up to `reaction_jitter` more...
    pub(crate) reaction: f32,
    pub(crate) reaction_jitter: f32,
    /// ...and `spot_time` more for every metre beyond `BOT_FAR`.
    pub(crate) spot_time: f32,
    /// Radians per second, turning towards an enemy.
    pub(crate) turn_rate: f32,
    /// Radius of the aim wobble, in radians, once settled on a target,
    /// growing beyond `BOT_FAR` to twice as wide `far_doubling` metres on...
    pub(crate) aim_error: f32,
    pub(crate) far_doubling: f32,
    /// ...plus this many metres at any range...
    pub(crate) aim_slop: f32,
    /// ...plus `tracking` radians for every radian per second the enemy
    /// sweeps across its view beyond `easy_track` (one strafing up close is
    /// hard to follow; far away, easy).
    pub(crate) easy_track: f32,
    pub(crate) tracking: f32,
    /// Seconds it takes the wobble to settle down from its widest.
    pub(crate) settle: f32,
    /// How much of the enemy's movement during the bullet's flight a bot
    /// aims ahead by: somewhere in this range, rolled with each wobble.
    pub(crate) lead: (f32, f32),
}

/// Easy, normal and hard.
const _: () = assert!(BOT_EASY == 0 && BOT_NORMAL == 1 && BOT_HARD == 2);
pub(crate) const BOT_SKILLS: [BotSkill; BOT_DIFFICULTY_NAMES.len()] = [
    BotSkill {
        reaction: 0.3,
        reaction_jitter: 0.2,
        spot_time: 0.008,
        turn_rate: 6.0,
        aim_error: 0.015,
        far_doubling: 30.0,
        aim_slop: 0.15,
        easy_track: 0.3,
        tracking: 0.12,
        settle: 1.5,
        lead: (0.5, 1.2),
    },
    BotSkill {
        reaction: 0.22,
        reaction_jitter: 0.15,
        spot_time: 0.005,
        turn_rate: 7.5,
        aim_error: 0.011,
        far_doubling: 45.0,
        aim_slop: 0.1,
        easy_track: 0.45,
        tracking: 0.08,
        settle: 1.1,
        lead: (0.7, 1.1),
    },
    BotSkill {
        reaction: 0.15,
        reaction_jitter: 0.1,
        spot_time: 0.003,
        turn_rate: 9.0,
        aim_error: 0.008,
        far_doubling: 70.0,
        aim_slop: 0.06,
        easy_track: 0.6,
        tracking: 0.05,
        settle: 0.7,
        lead: (0.85, 1.05),
    },
];

pub(crate) fn bot_skill(difficulty: u8) -> &'static BotSkill {
    BOT_SKILLS.get(difficulty as usize).unwrap_or(&BOT_SKILLS[BOT_NORMAL as usize])
}
/// Seconds a bot stays turned towards whoever hurt it.
pub(crate) const BOT_GRUDGE: f32 = 3.0;
/// A bot that kills somebody within this many metres, and has nothing
/// better to do, goes over to the body (giving up after `BOT_GLOAT_TIME`
/// seconds) and crouches up and down on it `BOT_BOBS` times, `BOT_BOB`
/// seconds down and as long up.
pub(crate) const BOT_GLOAT_RANGE: f32 = 25.0;
pub(crate) const BOT_GLOAT_TIME: f32 = 6.0;
pub(crate) const BOT_BOBS: u32 = 5;
pub(crate) const BOT_BOB: f32 = 0.25;
/// Close enough to the body to start (metres along the ground).
pub(crate) const BOT_GLOAT_REACH: f32 = 0.8;
/// Bullets vanish after this many seconds, as they do on the clients.
pub(crate) const BULLET_LIFETIME: f32 = 1.5;

pub(crate) struct Bot {
    pub(crate) vel: Vec3,
    /// It was standing on something last tick.
    pub(crate) grounded: bool,
    /// The way to `goal`, waypoint by waypoint (empty: run straight at it),
    /// and the goal it was found for.
    pub(crate) path: Vec<Vec3>,
    pub(crate) path_to: Option<Vec3>,
    /// Seconds spent wanting to move but hardly moving.
    pub(crate) stuck: f32,
    /// Conquest: the flag it is going for, and seconds until it thinks again.
    pub(crate) objective: Option<usize>,
    pub(crate) rethink_in: f32,
    /// Where it is running to.
    pub(crate) goal: Vec3,
    pub(crate) retarget_in: f32,
    /// The gun in its hands, with its magazine; the other one it carries
    /// this life, with its own; and which of the two it picked to lead with.
    pub(crate) gun: WeaponId,
    pub(crate) ammo: u32,
    pub(crate) spare: WeaponId,
    pub(crate) spare_ammo: u32,
    pub(crate) primary: WeaponId,
    /// Seconds until it may change guns again.
    pub(crate) switch_in: f32,
    /// Seconds of reloading left; 0 when not reloading.
    pub(crate) reload: f32,
    /// Seconds until it may fire again.
    pub(crate) cooldown: f32,
    /// Shots left in the current burst (automatic guns).
    pub(crate) burst: u32,
    /// The enemy it is fighting, and for how long it has had them in sight.
    pub(crate) enemy: Option<PlayerId>,
    pub(crate) sight: f32,
    /// Seconds of `sight` it needs before shooting this enemy.
    pub(crate) reaction: f32,
    /// Where the enemy was last tick, and how fast it reckons they are going.
    pub(crate) enemy_at: Vec3,
    pub(crate) enemy_vel: Vec3,
    /// How far off its aim is (yaw, pitch), and how much it leads by,
    /// re-rolled every `wobble_in` seconds.
    pub(crate) aim_error: Vec2,
    pub(crate) lead: f32,
    pub(crate) wobble_in: f32,
    /// Whoever hurt it last, and for how much longer it cares.
    pub(crate) provoked_by: Option<PlayerId>,
    pub(crate) provoked_for: f32,
    /// Seconds of being blinded by a flashbang left.
    pub(crate) blind: f32,
    /// Where it reckons a noise it heard came from, and for how much
    /// longer it will go and look; and the last `Noise::id` it listened to.
    pub(crate) investigate: Option<Vec3>,
    pub(crate) investigate_for: f32,
    pub(crate) last_noise: u64,
    /// Where lies the body of somebody it just killed close by, for how much
    /// longer it will go there, and for how long it has been bobbing on it.
    pub(crate) gloat: Option<Vec3>,
    pub(crate) gloat_for: f32,
    pub(crate) bobbing: f32,
    /// On the short box (see `bot_crouch`).
    pub(crate) crouched: bool,
}

impl Bot {
    pub(crate) fn new() -> Bot {
        Bot {
            vel: Vec3::ZERO,
            grounded: false,
            path: Vec::new(),
            path_to: None,
            stuck: 0.0,
            objective: None,
            rethink_in: 0.0,
            goal: Vec3::ZERO,
            retarget_in: 0.0,
            gun: 0,
            ammo: 0,
            spare: weapons::PISTOL,
            spare_ammo: 0,
            primary: 0,
            switch_in: 0.0,
            reload: 0.0,
            cooldown: 0.0,
            burst: 0,
            enemy: None,
            sight: 0.0,
            reaction: 0.0,
            enemy_at: Vec3::ZERO,
            enemy_vel: Vec3::ZERO,
            aim_error: Vec2::ZERO,
            lead: 1.0,
            wobble_in: 0.0,
            provoked_by: None,
            provoked_for: 0.0,
            blind: 0.0,
            investigate: None,
            investigate_for: 0.0,
            last_noise: 0,
            gloat: None,
            gloat_for: 0.0,
            bobbing: 0.0,
            crouched: false,
        }
    }
}

/// Something for bots to hear.
pub(crate) struct Noise {
    /// Counts up, so a bot can tell which it has listened to already.
    pub(crate) id: u64,
    /// Who made it: the shooter, the runner, or whoever threw the grenade.
    pub(crate) source: PlayerId,
    pub(crate) team: u8,
    pub(crate) pos: Vec3,
    pub(crate) kind: NoiseKind,
    pub(crate) age: f32,
}

/// From quietest to loudest: a bot hearing several goes for the loudest.
#[derive(Clone, Copy, PartialEq, PartialOrd, Debug)]
pub(crate) enum NoiseKind {
    Step,
    /// A smoke grenade popping.
    Pop,
    Shot,
    /// A frag or a flashbang going off.
    Bang,
}

impl NoiseKind {
    /// How far away it can be heard (metres).
    pub(crate) fn reach(self) -> f32 {
        match self {
            NoiseKind::Step => 16.0,
            NoiseKind::Pop => 40.0,
            NoiseKind::Shot => 100.0,
            NoiseKind::Bang => 150.0,
        }
    }

    /// How far off a guess of where it came from may be (see `SHOT_GUESS`).
    pub(crate) fn guess(self) -> (f32, f32) {
        match self {
            NoiseKind::Step => STEP_GUESS,
            _ => SHOT_GUESS,
        }
    }
}

/// A bullet fired by a bot. People's bullets are flown by their own clients;
/// bots have none, so the server flies theirs.
pub(crate) struct BotBullet {
    pub(crate) shooter: PlayerId,
    pub(crate) team: u8,
    pub(crate) weapon: WeaponId,
    pub(crate) pos: Vec3,
    pub(crate) vel: Vec3,
    pub(crate) age: f32,
}

/// What a bot needs to know about everyone else this tick.
pub(crate) struct Body {
    pub(crate) id: PlayerId,
    pub(crate) team: u8,
    pub(crate) center: Vec3,
}

/// A bot's shot: shooter, team, weapon, origin and one direction per bullet.
pub(crate) type BotShot = (PlayerId, u8, WeaponId, Vec3, Vec<Vec3>);

/// True if a player coming in with their box centred at `spawn` would be
/// inside, or right up against, one whose box is centred at `body`.
/// Where bots can walk: nowhere on a course, where there are none.
pub(crate) fn nav_for(map: &Map, mode: u8) -> NavGrid {
    if modes::info(mode).bots { NavGrid::new(map) } else { NavGrid::empty() }
}

/// Where a bot's eyes are.
pub(crate) fn bot_eye(player: &Player) -> Vec3 {
    let crouched = player.flags & FLAG_CROUCH != 0;
    let eye = if crouched { map::CROUCH_EYE } else { map::STAND_EYE };
    player.pos + Vec3::Y * (eye - map::player_half(crouched).y)
}

/// Crouches the bot, or stands it back up if there is room, the way people
/// do: the box shrinks towards the feet, and clients are told.
pub(crate) fn bot_crouch(map: &Map, player: &mut Player, bot: &mut Bot, crouch: bool) {
    let drop = map::PLAYER_HALF.y - map::CROUCH_HALF.y;
    if crouch && !bot.crouched {
        player.pos.y -= drop;
    } else if !crouch && bot.crouched {
        let standing = player.pos + Vec3::Y * drop;
        if !map.is_free(standing, map::PLAYER_HALF) {
            return;
        }
        player.pos = standing;
    } else {
        return;
    }
    bot.crouched = crouch;
    player.flags = (player.flags & !FLAG_CROUCH) | if crouch { FLAG_CROUCH } else { 0 };
}

/// The point a bot aims at on a body: the upper chest.
pub(crate) fn aim_point(center: Vec3) -> Vec3 {
    center + Vec3::Y * 0.25
}

/// The distances (metres) at which a bot reckons a weapon does best.
pub(crate) fn bot_gun_band(gun: WeaponId) -> (f32, f32) {
    match gun {
        weapons::RIFLE => (5.0, 40.0),
        weapons::SMG => (0.0, 15.0),
        weapons::SHOTGUN => (0.0, 7.0),
        weapons::DMR => (15.0, 60.0),
        weapons::SNIPER => (25.0, LONG_RANGE),
        weapons::PISTOL => (0.0, 20.0),
        _ => (0.0, weapons::KNIFE_RANGE),
    }
}

/// How well a weapon suits a fight `distance` away: 1 within its band,
/// less the further outside it, never quite nothing.
pub(crate) fn bot_gun_fit(gun: WeaponId, distance: f32) -> f32 {
    let (near, far) = bot_gun_band(gun);
    let off = (near - distance).max(0.0) + (distance - far).max(0.0);
    (1.0 - off / 20.0).max(0.05)
}

/// The two guns a bot carries for a life, when fights tend to be `range`
/// away: mostly one that suits that range, less often one `teammates`
/// already lead with; and a second for where the first is weak.
pub(crate) fn bot_loadout(range: f32, teammates: &[WeaponId], rng: &mut u64) -> (WeaponId, WeaponId) {
    let pick = |rng: &mut u64, weight: &dyn Fn(WeaponId) -> f32| {
        let weights: Vec<f32> = (0..GUN_COUNT).map(weight).collect();
        let mut roll = random_unit(rng) * weights.iter().sum::<f32>();
        for (gun, weight) in weights.iter().enumerate() {
            roll -= weight;
            if roll <= 0.0 {
                return gun as WeaponId;
            }
        }
        GUN_COUNT - 1
    };
    let primary = pick(rng, &|gun| {
        let taken = teammates.iter().filter(|other| **other == gun).count() as f32;
        bot_gun_fit(gun, range).powi(2) / (1.0 + taken)
    });
    // A long gun wants something for up close, and a short one something
    // that reaches.
    let long = |gun: WeaponId| bot_gun_band(gun).0 >= 10.0;
    let secondary = if long(primary) {
        pick(rng, &|gun| if long(gun) { 0.0 } else { bot_gun_fit(gun, 6.0).powi(2) })
    } else {
        pick(rng, &|gun| if gun == primary { 0.0 } else { bot_gun_fit(gun, range.max(30.0)).powi(2) })
    };
    (primary, secondary)
}

/// Whether a bot would rather have its other gun in hand: in a fight
/// `distance` away, if it suits the distance clearly better, counting an
/// empty magazine against a gun (drawing the other beats reloading); with
/// nobody about, to reload the other, or to go back to its first.
pub(crate) fn bot_wants_spare(bot: &Bot, distance: Option<f32>) -> bool {
    match distance {
        Some(distance) => {
            let score = |gun: WeaponId, ammo: u32| bot_gun_fit(gun, distance) * if ammo == 0 { 0.35 } else { 1.0 };
            score(bot.spare, bot.spare_ammo) > score(bot.gun, bot.ammo) + 0.2
        }
        None => {
            let loaded = bot.reload <= 0.0 && bot.ammo >= weapon(bot.gun).magazine;
            loaded && (bot.spare_ammo < weapon(bot.spare).magazine || bot.gun != bot.primary)
        }
    }
}

/// How far apart fights on `map` tend to be in `mode`: how far one can
/// see from where people come in (around the hill or the flags, in those
/// modes), the longer half of the sight lines counting.
pub(crate) fn typical_range(map: &Map, mode: u8) -> f32 {
    let lift = Vec3::Y * (map::STAND_EYE - map::PLAYER_HALF.y);
    let centers: Vec<Vec3> = match mode {
        MODE_CONQUEST if !map.flags.is_empty() => map.flags.iter().flat_map(|flag| flag.spawns.iter().map(|s| s.pos)).collect(),
        MODE_HILL => (0..8)
            .map(|k| {
                let around = k as f32 * std::f32::consts::TAU / 8.0;
                map.hill + Vec3::new(around.cos() * 4.0, map::PLAYER_HALF.y, around.sin() * 4.0)
            })
            .filter(|center| map.is_free(*center, map::PLAYER_HALF))
            .collect(),
        _ => map.spawns.iter().map(|s| s.pos).collect(),
    };
    let mut sights: Vec<f32> = centers
        .iter()
        .flat_map(|center| {
            (0..16).map(move |k| {
                let dir = map::look_dir(k as f32 * std::f32::consts::TAU / 16.0, 0.0);
                map.raycast(*center + lift, dir, LONG_RANGE).unwrap_or(LONG_RANGE)
            })
        })
        .collect();
    if sights.is_empty() {
        return 25.0;
    }
    sights.sort_by(f32::total_cmp);
    let longer = &sights[sights.len() / 2..];
    longer.iter().sum::<f32>() / longer.len() as f32
}

/// Decides whom the bot fights, if anybody: the enemy it is already fighting
/// while it can still see them, otherwise the nearest one it notices.
/// Returns where that enemy is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bot_pick_enemy(
    map: &Map,
    smokes: &[Smoke],
    id: PlayerId,
    player: &Player,
    bot: &mut Bot,
    bodies: &[Body],
    team_mode: bool,
    skill: &BotSkill,
    rng: &mut u64,
    dt: f32,
) -> Option<Vec3> {
    bot.provoked_for -= dt;
    if bot.provoked_for <= 0.0 {
        bot.provoked_by = None;
    }
    bot.blind = (bot.blind - dt).max(0.0);
    if bot.blind > 0.0 {
        bot.enemy = None;
        bot.sight = 0.0;
        return None;
    }
    let eye = bot_eye(player);
    let facing = map::look_dir(player.yaw, 0.0);
    let mut current = None;
    let mut nearest: Option<(f32, &Body)> = None;
    for body in bodies {
        if body.id == id || (team_mode && body.team == player.team) {
            continue;
        }
        let offset = (body.center - player.pos).with_y(0.0);
        let distance = offset.length();
        let noticed = distance < BOT_HEARING
            || bot.provoked_by == Some(body.id)
            || offset.dot(facing) > BOT_VIEW_COS * distance;
        let fighting = Some(body.id) == bot.enemy;
        // No looking through walls (the dearer test) for one it would not notice anyway.
        if !noticed && !fighting {
            continue;
        }
        // As far as the eye can see.
        let to = aim_point(body.center);
        let in_sight = (map.line_of_sight(eye, to) || map.line_of_sight(eye, to + Vec3::Y * 0.5))
            && !smokes.iter().any(|smoke| map::smoke_blocks(smoke.center, eye, to));
        if !in_sight {
            continue;
        }
        if fighting {
            current = Some(body);
        }
        if noticed && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, body));
        }
    }
    let Some(target) = current.or(nearest.map(|(_, body)| body)) else {
        bot.enemy = None;
        bot.sight = 0.0;
        return None;
    };
    if bot.enemy == Some(target.id) {
        bot.sight += dt;
        // People's positions come in with the network's hiccups, so the
        // speed is an average over a moment, not this tick's jump.
        if dt > 0.0 {
            let measured = ((target.center - bot.enemy_at) / dt).clamp_length_max(60.0);
            bot.enemy_vel += (measured - bot.enemy_vel) * (dt / BOT_VEL_SMOOTHING).min(1.0);
        }
    } else {
        // A new enemy: take a moment to react, and dodge sideways right away.
        bot.enemy = Some(target.id);
        bot.sight = 0.0;
        let far = (target.center.distance(player.pos) - BOT_FAR).max(0.0);
        bot.reaction = skill.reaction + skill.reaction_jitter * random_unit(rng) + skill.spot_time * far;
        bot.enemy_vel = Vec3::ZERO;
        bot.wobble_in = 0.0;
        bot.retarget_in = 0.0;
    }
    bot.enemy_at = target.center;
    Some(target.center)
}

/// Hears the enemies' gunshots, grenades and footsteps made since it last
/// listened, and while not fighting anybody, guesses where the one that
/// matters most (the loudest kind, then the nearest) came from, to go and
/// see. In conquest only noises close by draw it off its flag.
pub(crate) fn bot_listen(id: PlayerId, player: &Player, bot: &mut Bot, noises: &[Noise], team_mode: bool, conquest: bool, rng: &mut u64) {
    let mut loudest: Option<(&Noise, f32)> = None;
    let heard_up_to = bot.last_noise;
    for noise in noises.iter().filter(|noise| noise.id > heard_up_to) {
        bot.last_noise = noise.id;
        if noise.source == id || (team_mode && noise.team == player.team) {
            continue;
        }
        let distance = noise.pos.distance(player.pos);
        if distance > noise.kind.reach() || (conquest && distance > BOT_INVESTIGATE_NEAR) {
            continue;
        }
        let louder = loudest.is_none_or(|(best, best_distance)| (noise.kind, -distance) > (best.kind, -best_distance));
        if louder {
            loudest = Some((noise, distance));
        }
    }
    let Some((noise, _)) = loudest else {
        return;
    };
    if bot.enemy.is_some() {
        return;
    }
    let (angle, depth) = noise.kind.guess();
    let turn = Quat::from_rotation_y(angle * (random_unit(rng) * 2.0 - 1.0));
    let scale = 1.0 + depth * (random_unit(rng) * 2.0 - 1.0);
    let guess = player.pos + turn * (noise.pos - player.pos).with_y(0.0) * scale;
    bot.investigate = Some(guess.with_y(noise.pos.y));
    bot.investigate_for = BOT_INVESTIGATE_TIME;
}

/// Conquest: the flag the bot is going for. It thinks again every so often,
/// and as soon as the flag it was after is safely its team's: then it goes
/// for one still to take, mostly the nearest, or else guards one it holds.
pub(crate) fn bot_objective(map: &Map, flags: &[FlagState], player: &Player, bot: &mut Bot, rng: &mut u64, dt: f32) -> Option<usize> {
    if flags.is_empty() {
        return None;
    }
    let team = player.team & 1;
    bot.rethink_in -= dt;
    let done = bot.objective.is_some_and(|i| flags[i].secure(team));
    if bot.objective.is_some() && bot.rethink_in > 0.0 && !done {
        return bot.objective;
    }
    bot.rethink_in = 20.0 + random_unit(rng) * 15.0;
    let mut to_take: Vec<usize> = (0..flags.len()).filter(|&i| !flags[i].secure(team)).collect();
    if to_take.is_empty() {
        bot.objective = Some(next_random(rng) as usize % flags.len());
        return bot.objective;
    }
    to_take.sort_by(|a, b| {
        let distance = |i: &usize| map.flags[*i].pos.distance(player.pos);
        distance(a).total_cmp(&distance(b))
    });
    let roll = random_unit(rng);
    let pick = if roll < 0.6 || to_take.len() == 1 {
        0
    } else if roll < 0.85 {
        1
    } else {
        next_random(rng) as usize % to_take.len()
    };
    bot.objective = Some(to_take[pick]);
    bot.objective
}

/// Runs towards the bot's goal, finding the way round buildings and the
/// like, hopping over whatever is still in the way. While fighting, the
/// goals are short dashes from side to side. `target` is somewhere to be
/// (centre, radius, and whether to stay there or only drop by now and then).
#[allow(clippy::too_many_arguments)]
pub(crate) fn bot_move(
    map: &Map,
    nav: &NavGrid,
    player: &mut Player,
    bot: &mut Bot,
    target: Option<(Vec3, f32, bool)>,
    enemy: Option<Vec3>,
    rng: &mut u64,
    dt: f32,
) {
    // Going to see what it heard: until it gets there, it has been at it
    // long enough, or it finds somebody to fight.
    bot.investigate_for -= dt;
    let looked = |spot: Vec3| bot.investigate_for <= 0.0 || (spot - player.pos).with_y(0.0).length() < 2.5;
    if (enemy.is_some() || bot.investigate.is_some_and(looked)) && bot.investigate.take().is_some() {
        bot.retarget_in = 0.0;
    }
    // Gloating over a body, while there is nothing better to do: nobody to
    // fight, nothing heard, nobody to get back at. Done, it stands up.
    bot.gloat_for -= dt;
    let distracted = enemy.is_some() || bot.investigate.is_some() || bot.provoked_by.is_some();
    let bobbed = bot.bobbing >= BOT_BOBS as f32 * 2.0 * BOT_BOB;
    let gave_up = bot.gloat_for <= 0.0 && bot.bobbing == 0.0;
    if (distracted || bobbed || gave_up) && bot.gloat.take().is_some() {
        bot.retarget_in = 0.0;
    }
    let on_body = bot.gloat.is_some_and(|body| (body - player.pos).with_y(0.0).length() < BOT_GLOAT_REACH);
    if on_body {
        bot.bobbing += dt;
    }
    let down = on_body && ((bot.bobbing / BOT_BOB) as u32).is_multiple_of(2);
    bot_crouch(map, player, bot, down);
    let half = map::player_half(bot.crouched);
    // Careful when fighting or creeping up on a noise; still on a body.
    let speed = if on_body {
        0.0
    } else if enemy.is_some() || bot.investigate.is_some() {
        5.5
    } else {
        7.0
    };
    bot.retarget_in -= dt;
    let to_goal = (bot.goal - player.pos).with_y(0.0);
    // A new target (a conquest flag) takes over at once.
    let off_target = target.is_some_and(|(center, radius, stay)| stay && (bot.goal - center).with_y(0.0).length() > radius + 0.5);
    if let Some(spot) = bot.investigate.or(bot.gloat) {
        bot.goal = spot;
    } else if bot.retarget_in <= 0.0 || to_goal.length() < 1.5 || (enemy.is_none() && off_target) {
        if let Some(enemy) = enemy {
            let towards = (enemy - player.pos).with_y(0.0).normalize_or_zero();
            let side = Vec3::new(-towards.z, 0.0, towards.x);
            let sign = if next_random(rng).is_multiple_of(2) { 1.0 } else { -1.0 };
            // Closing in, or backing off, to where its gun does best.
            let (near, far) = bot_gun_band(bot.gun);
            let distance = (enemy - player.pos).with_y(0.0).length();
            let closer = if distance > far {
                1.0
            } else if distance < near {
                -1.0
            } else {
                0.0
            };
            bot.goal = player.pos + side * sign * (3.0 + random_unit(rng) * 4.0) + towards * closer * (2.0 + random_unit(rng) * 3.0);
            bot.retarget_in = 0.6 + random_unit(rng);
        } else {
            let [min, max] = map.bounds;
            let x = min.x + random_unit(rng) * (max.x - min.x);
            let z = min.y + random_unit(rng) * (max.y - min.y);
            bot.goal = match target {
                // On the hill half the time, so there is something to fight
                // over; on a conquest flag, all the time.
                Some((center, radius, stay)) if stay || next_random(rng).is_multiple_of(2) => {
                    let (dx, dz) = (random_unit(rng) * 2.0 - 1.0, random_unit(rng) * 2.0 - 1.0);
                    center + Vec3::new(dx, 0.0, dz) * radius
                }
                _ => Vec3::new(x, 0.0, z),
            };
            // Far goals take a while to reach.
            let far = (bot.goal - player.pos).with_y(0.0).length() > 20.0;
            bot.retarget_in = if far { 45.0 } else { 2.0 + random_unit(rng) * 3.0 };
        }
    }
    // Follow the way to the goal while not fighting.
    let mut steer = bot.goal;
    if enemy.is_none() {
        let feet = player.pos - Vec3::Y * half.y;
        let goal = bot.goal.with_y(map.ground(bot.goal.x, bot.goal.z).unwrap_or(0.0));
        if bot.path_to.is_none_or(|to| to.distance(goal) > 0.5) {
            bot.path = nav.path(map, feet, goal).unwrap_or_default();
            bot.path_to = Some(goal);
        }
        crate::nav::pass_waypoints(map, &mut bot.path, player.pos);
        if let Some(next) = bot.path.first() {
            steer = *next;
        }
    } else {
        bot.path_to = None;
    }
    let to_goal = (steer - player.pos).with_y(0.0);
    let wish = to_goal.normalize_or_zero() * speed;
    let blend = (8.0 * dt).min(1.0);
    bot.vel.x += (wish.x - bot.vel.x) * blend;
    bot.vel.z += (wish.z - bot.vel.z) * blend;
    bot.vel.y -= 22.0 * dt;
    let wanted = Vec3::new(bot.vel.x, 0.0, bot.vel.z);
    let mut grounded = map.move_and_slide(&mut player.pos, &mut bot.vel, half, dt);
    if bot.grounded && !grounded && bot.vel.y <= 0.0 {
        let drop = map::STEP_UP.max(speed * dt);
        if map.snap_to_ground(&mut player.pos, half, drop) {
            grounded = true;
            bot.vel.y = 0.0;
        }
    }
    bot.grounded = grounded;
    if let Some(pad) = map.pad_at(player.pos - Vec3::Y * half.y) {
        // Bots aim with arrow pads from a standstill so they land where intended.
        bot.vel = match pad.direction() {
            Some(_) => pad.launch,
            None => pad.launch_velocity(bot.vel),
        };
    } else if grounded {
        // Hop when blocked by something, and every now and then for fun.
        let blocked = Vec3::new(bot.vel.x, 0.0, bot.vel.z).length() < wanted.length() * 0.5;
        if !on_body && (blocked || next_random(rng).is_multiple_of(90)) {
            bot.vel.y = 8.0;
        }
    }
    // Getting nowhere for a while: find the way again, or give up on the goal.
    let moving = Vec3::new(bot.vel.x, 0.0, bot.vel.z).length() > 1.0;
    bot.stuck = if moving || wanted.length() < 0.1 { 0.0 } else { bot.stuck + dt };
    if bot.stuck > 1.5 {
        bot.stuck = 0.0;
        if bot.path_to.is_some() {
            bot.path_to = None;
        } else {
            // A noise, or a body, somewhere it cannot get to is not worth the trouble.
            bot.investigate = None;
            bot.gloat = None;
            bot.retarget_in = 0.0;
        }
    }
    // Not fighting: look where it is going, or, having just heard
    // something, the way it came from.
    if enemy.is_none() {
        let glance = bot.investigate.filter(|_| bot.investigate_for > BOT_INVESTIGATE_TIME - BOT_GLANCE_TIME);
        if let Some(spot) = glance {
            let to = spot - player.pos;
            player.yaw = turn_towards(player.yaw, (-to.x).atan2(-to.z), BOT_TURN_RATE * 2.0 * dt);
        } else if wish.length_squared() > 0.01 {
            player.yaw = turn_towards(player.yaw, (-wish.x).atan2(-wish.z), BOT_TURN_RATE * 2.0 * dt);
        }
        // Looking down at the body it is on.
        let pitch = if on_body { -0.6 } else { 0.0 };
        player.pitch = turn_towards(player.pitch, pitch, BOT_TURN_RATE * dt);
    }
}

/// Keeps a bot's gun loaded and aims it at `enemy` as well as its `skill`
/// allows, and shoots when it is lined up, unless it may not pull the
/// `trigger`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bot_fight(
    id: PlayerId,
    player: &mut Player,
    bot: &mut Bot,
    enemy: Option<Vec3>,
    trigger: bool,
    skill: &BotSkill,
    rng: &mut u64,
    dt: f32,
) -> Option<BotShot> {
    bot.cooldown = (bot.cooldown - dt).max(0.0);
    bot.switch_in -= dt;
    if bot.switch_in <= 0.0 && bot_wants_spare(bot, enemy.map(|enemy| enemy.distance(player.pos))) {
        std::mem::swap(&mut bot.gun, &mut bot.spare);
        std::mem::swap(&mut bot.ammo, &mut bot.spare_ammo);
        bot.reload = 0.0;
        bot.burst = 0;
        bot.cooldown = DRAW_TIME;
        bot.switch_in = if enemy.is_some() { 1.0 } else { 0.5 };
    }
    let stats = weapon(bot.gun);
    if bot.reload > 0.0 {
        bot.reload -= dt;
        if bot.reload <= 0.0 {
            bot.reload = 0.0;
            if stats.reload_per_round {
                // Round by round until it is full, unless there is someone to shoot.
                bot.ammo += 1;
                if bot.ammo < stats.magazine && enemy.is_none() {
                    bot.reload = stats.reload;
                }
            } else {
                bot.ammo = stats.magazine;
            }
        }
    } else if bot.ammo == 0 || (enemy.is_none() && bot.ammo < stats.magazine) {
        // Empty, or a quiet moment to top up.
        bot.reload = stats.reload;
    }
    player.weapon = bot.gun;
    player.flags = (bot.crouched as u8 * FLAG_CROUCH) | if bot.reload > 0.0 { FLAG_RELOAD } else { 0 };
    let enemy = enemy?;
    // Aims down sights at whoever it is after, between reloads.
    if bot.reload <= 0.0 && player.weapon != weapons::KNIFE {
        player.flags |= FLAG_AIM;
    }

    let eye = bot_eye(player);
    // The aim wobbles around the target: widely at first, then less; and
    // more while the enemy (or the bot itself) moves fast across its view.
    bot.wobble_in -= dt;
    if bot.wobble_in <= 0.0 {
        bot.wobble_in = 0.25 + random_unit(rng) * 0.25;
        let offset = aim_point(enemy) - eye;
        let distance = offset.length().max(1.0);
        let along = offset / distance;
        let relative = bot.enemy_vel - bot.vel;
        let sweep = (relative - along * relative.dot(along)).length() / distance;
        let far = 1.0 + (distance - BOT_FAR).max(0.0) / skill.far_doubling;
        let tracking = skill.tracking * (sweep - skill.easy_track).max(0.0);
        let error = skill.aim_error * far + skill.aim_slop / distance + tracking;
        let unsettled = 1.0 + 1.5 * (1.0 - bot.sight / skill.settle).max(0.0);
        let radius = error * unsettled * random_unit(rng).sqrt();
        let around = random_unit(rng) * std::f32::consts::TAU;
        bot.aim_error = Vec2::new(around.cos(), around.sin()) * radius;
        bot.lead = skill.lead.0 + (skill.lead.1 - skill.lead.0) * random_unit(rng);
    }
    // Aim where the enemy will be when the bullet gets there, and above it
    // by as much as the bullet will drop on the way.
    let mut target = aim_point(enemy);
    if stats.speed > 0.0 {
        let flight = eye.distance(target) / stats.speed;
        let flight = eye.distance(target + bot.enemy_vel * flight) / stats.speed;
        target += bot.enemy_vel * flight * bot.lead;
        target.y += 0.5 * stats.gravity * flight * flight;
    }
    let to = target - eye;
    let yaw = (-to.x).atan2(-to.z) + bot.aim_error.x;
    let pitch = to.y.atan2(to.with_y(0.0).length()) + bot.aim_error.y;
    player.yaw = turn_towards(player.yaw, yaw, skill.turn_rate * dt);
    player.pitch = turn_towards(player.pitch, pitch, skill.turn_rate * dt);
    let lined_up = angle_between(player.yaw, yaw).abs() < 0.1 && (player.pitch - pitch).abs() < 0.1;
    if !trigger || !lined_up || bot.sight < bot.reaction || bot.reload > 0.0 || bot.cooldown > 0.0 || bot.ammo == 0 {
        return None;
    }

    bot.ammo -= 1;
    bot.cooldown = stats.interval;
    if stats.auto {
        // Automatic guns go in bursts.
        if bot.burst == 0 {
            bot.burst = 3 + (next_random(rng) % 5) as u32;
        }
        bot.burst -= 1;
        if bot.burst == 0 {
            bot.cooldown += 0.3 + random_unit(rng) * 0.4;
        }
    } else {
        // Nobody clicks at exactly the fire rate.
        bot.cooldown += 0.1 + random_unit(rng) * 0.25;
    }
    // Every bullet goes to a random point inside the spread cone, like
    // people's: the aimed one, as bots shoot aimed (their own aim is off
    // enough without the hip's spread on top).
    let rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
    let cone = stats.aim_spread;
    let dirs = (0..stats.pellets)
        .map(|_| {
            let off_axis = cone * random_unit(rng).sqrt();
            let around = random_unit(rng) * std::f32::consts::TAU;
            (rotation * Vec3::new(off_axis * around.cos(), off_axis * around.sin(), -1.0)).normalize()
        })
        .collect();
    Some((id, player.team, bot.gun, eye, dirs))
}

/// `angle - from`, wrapped to -π..π.
pub(crate) fn angle_between(from: f32, angle: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (angle - from + PI).rem_euclid(TAU) - PI
}

/// Turns `current` towards `wanted` by at most `max` radians, the short way round.
pub(crate) fn turn_towards(current: f32, wanted: f32, max: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let turned = current + angle_between(current, wanted).clamp(-max, max);
    (turned + PI).rem_euclid(TAU) - PI
}

impl Server {
    /// Moves bots across until the teams differ by at most one player. People
    /// stay on the team they picked.
    pub(crate) fn balance_bots(&mut self) {
        loop {
            let ones = self.players.values().filter(|p| p.team == 1).count();
            let zeros = self.players.len() - ones;
            let (big, small) = if zeros > ones { (0, 1) } else { (1, 0) };
            if zeros.abs_diff(ones) < 2 {
                break;
            }
            let Some(bot) = self.players.values_mut().find(|p| p.bot.is_some() && p.team == big) else {
                break;
            };
            bot.team = small;
        }
    }

    pub(crate) fn add_bot(&mut self) {
        if self.players.len() >= MAX_PLAYERS || !modes::info(self.settings.mode).bots {
            return;
        }
        let id = self.next_bot_id;
        self.next_bot_id += 1;
        let name = format!("[bot] {}", BOT_NAMES[(id as usize - 1) % BOT_NAMES.len()]);
        self.join_counter += 1;
        let player = self.new_player(name, Some(Bot::new()));
        self.players.insert(id, player);
        self.roster_dirty = true;
        if self.phase == Phase::Playing {
            let roster = self.roster_msg();
            self.broadcast(&roster);
            self.roster_dirty = false;
            self.spawn_player(id);
        }
    }

    /// Bots run between random points and hop around, using the same
    /// collision code as real players, and shoot at the enemies they see.
    pub(crate) fn update_bots(&mut self, dt: f32) {
        let team_mode = is_team_mode(self.settings.mode);
        let conquest = self.settings.mode == MODE_CONQUEST;
        let hill = (self.settings.mode == MODE_HILL).then_some(self.map.hill);
        let bodies: Vec<Body> = self
            .players
            .iter()
            .filter(|(_, p)| p.alive)
            .map(|(id, p)| Body {
                id: *id,
                team: p.team,
                center: p.pos,
            })
            .collect();
        let skill = bot_skill(self.settings.bot_difficulty);
        let mut rng = self.rng;
        let mut shots = Vec::new();
        for (id, player) in self.players.iter_mut() {
            if !player.alive {
                continue;
            }
            let Some(mut bot) = player.bot.take() else {
                continue;
            };
            let enemy = bot_pick_enemy(&self.map, &self.smokes, *id, player, &mut bot, &bodies, team_mode, skill, &mut rng, dt);
            bot_listen(*id, player, &mut bot, &self.noises, team_mode, conquest, &mut rng);
            let target = if conquest {
                bot_objective(&self.map, &self.flags, player, &mut bot, &mut rng, dt)
                    .map(|flag| (self.map.flags[flag].pos, self.map.flags[flag].radius * 0.7, true))
            } else {
                hill.map(|hill| (hill, 5.5, false))
            };
            bot_move(&self.map, &self.nav, player, &mut bot, target, enemy, &mut rng, dt);
            let trigger = !self.settings.bots_hold_fire;
            if let Some(shot) = bot_fight(*id, player, &mut bot, enemy, trigger, skill, &mut rng, dt) {
                shots.push(shot);
            }
            player.bot = Some(bot);
        }
        self.rng = rng;

        // Everyone sees and hears a bot's shot like anybody else's.
        for (shooter, team, weapon_id, origin, dirs) in shots {
            self.make_noise(shooter, origin, NoiseKind::Shot);
            let speed = weapon(weapon_id).speed;
            self.bullets.extend(dirs.iter().map(|dir| BotBullet {
                shooter,
                team,
                weapon: weapon_id,
                pos: origin,
                vel: *dir * speed,
                age: 0.0,
            }));
            self.broadcast(&ServerMsg::Fire {
                shooter,
                weapon: weapon_id,
                origin: origin.to_array(),
                dirs: dirs.iter().map(|dir| dir.to_array()).collect(),
            });
        }
    }

    /// Something bots may hear, made by `source` at `pos`.
    pub(crate) fn make_noise(&mut self, source: PlayerId, pos: Vec3, kind: NoiseKind) {
        let Some(team) = self.players.get(&source).map(|player| player.team) else {
            return;
        };
        self.noises.push(Noise {
            id: self.next_noise_id,
            source,
            team,
            pos,
            kind,
            age: 0.0,
        });
        self.next_noise_id += 1;
    }

    /// Everyone's footsteps, worked out from how they move as the clients
    /// work out each other's: running level, not crouched, a step a stride.
    pub(crate) fn update_footsteps(&mut self, dt: f32) {
        let mut steps = Vec::new();
        for (id, player) in self.players.iter_mut() {
            if !player.alive {
                (player.step_from, player.step_clock, player.stride) = (player.pos, 0.0, 0.0);
                continue;
            }
            player.step_clock += dt;
            if player.step_clock < STEP_SAMPLE {
                continue;
            }
            let moved = player.pos - player.step_from;
            let (run, climb) = (moved.with_y(0.0).length() / player.step_clock, moved.y.abs() / player.step_clock);
            (player.step_from, player.step_clock) = (player.pos, 0.0);
            // Too fast is a respawn or a jump pad, not running.
            let running = (STEP_MIN_SPEED..15.0).contains(&run) && climb < 1.5 && player.flags & FLAG_CROUCH == 0;
            if !running {
                player.stride = STRIDE * 0.5;
                continue;
            }
            player.stride += moved.with_y(0.0).length();
            if player.stride >= STRIDE {
                player.stride %= STRIDE;
                steps.push((*id, player.pos - Vec3::Y * map::PLAYER_HALF.y));
            }
        }
        for (id, feet) in steps {
            self.make_noise(id, feet, NoiseKind::Step);
        }
    }

    /// Flies the bots' bullets the way the clients fly theirs: each stops at
    /// the first wall or enemy in its way, and hurts the enemy.
    pub(crate) fn update_bullets(&mut self, dt: f32) {
        let team_mode = is_team_mode(self.settings.mode);
        let (map, players) = (&self.map, &self.players);
        let mut hits = Vec::new();
        self.bullets.retain_mut(|bullet| {
            let step = bullet.vel * dt;
            let distance = step.length();
            if distance > 0.0 {
                let dir = step / distance;
                let wall = map.raycast(bullet.pos, dir, distance);
                let mut reach = wall.unwrap_or(distance);
                let mut victim = None;
                // Bullets pass through the shooter's own team.
                for (id, player) in players {
                    if !player.alive || *id == bullet.shooter || (team_mode && player.team == bullet.team) {
                        continue;
                    }
                    let crouching = player.flags & FLAG_CROUCH != 0;
                    let half = map::hitbox_half(crouching);
                    if let Some(t) = map::ray_box(bullet.pos, dir, player.pos - half, player.pos + half)
                        && t < reach
                    {
                        reach = t;
                        let (head_min, head_max) = map::head_box(player.pos, crouching);
                        victim = Some((*id, map::ray_box(bullet.pos, dir, head_min, head_max).is_some()));
                    }
                }
                if let Some((victim, headshot)) = victim {
                    hits.push((victim, bullet.shooter, bullet.weapon, headshot));
                    return false;
                }
                if wall.is_some() {
                    return false;
                }
            }
            bullet.pos += step;
            bullet.vel.y -= weapon(bullet.weapon).gravity * dt;
            bullet.age += dt;
            bullet.age <= BULLET_LIFETIME
        });
        for (victim, shooter, weapon_id, headshot) in hits {
            // A kill can end the match, and then nobody else gets hurt.
            if self.phase == Phase::Playing {
                self.damage(victim, shooter, hit_damage(weapon_id, headshot), weapon_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_a_bot_takes_the_last_one_in() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        server.join_counter += 1;
        let player = server.new_player("P".into(), None);
        // Clear of the bots' ids, which count up from 1.
        let me = 1000;
        server.players.insert(me, player);
        server.handle_message(me, ClientMsg::AddBot);
        server.handle_message(me, ClientMsg::AddBot);
        let bots = |server: &Server| {
            let mut ids: Vec<_> = server.players.iter().filter(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).collect();
            ids.sort();
            ids
        };
        let before = bots(&server);
        assert_eq!(before.len(), 2);
        server.handle_message(me, ClientMsg::RemoveBot);
        assert_eq!(bots(&server), [before[0]], "the first bot stays");
        assert!(server.players.contains_key(&me), "people are never taken out");
    }

    #[test]
    fn bots_load_a_shotgun_shell_by_shell() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Loader".into(), None);
        let mut bot = Bot::new();
        bot.gun = weapons::SHOTGUN;
        let stats = weapon(weapons::SHOTGUN);
        let mut rng = 1;
        let dt = 0.01;
        let mut loading_for = 0.0;
        // Empty: it starts loading, one shell per `reload` seconds, until full.
        loop {
            bot_fight(1, &mut player, &mut bot, None, true, bot_skill(BOT_NORMAL), &mut rng, dt);
            if bot.reload <= 0.0 {
                break;
            }
            loading_for += dt;
            let loaded = (loading_for / stats.reload) as u32;
            assert!(bot.ammo.abs_diff(loaded) <= 1, "{} shells after {loading_for:.2} s", bot.ammo);
            assert!(loading_for < 10.0, "never done");
        }
        assert_eq!(bot.ammo, stats.magazine);
        assert!((loading_for - stats.reload * stats.magazine as f32).abs() < 0.1, "{loading_for}");
    }

    #[test]
    fn bots_fight_each_other() {
        let settings = ServerSettings {
            port: 0,
            bots: 4,
            ..ServerSettings::default()
        };
        let mut server = Server::bind(settings).unwrap();
        server.start_match();
        server.countdown = 0.0;
        // A minute of play.
        for _ in 0..60 * 60 {
            server.update_match(1.0 / 60.0);
            if server.phase != Phase::Playing {
                break;
            }
        }
        let kills: u32 = server.players.values().map(|p| p.kills).sum();
        assert!(kills > 0, "bots never killed anybody");
    }

    #[test]
    fn bots_take_flags_on_the_valley() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            mode: MODE_CONQUEST,
            map: 2,
            bots: 4,
            ..ServerSettings::default()
        })
        .unwrap();
        server.start_match();
        server.countdown = 0.0;
        // Two minutes of play.
        for _ in 0..60 * 120 {
            server.update_match(1.0 / 60.0);
            if server.phase != Phase::Playing {
                break;
            }
        }
        let taken: Vec<Option<u8>> = server.flags.iter().map(|f| f.owner).collect();
        println!("flags after two minutes: {taken:?}, tickets {:?}", server.team_scores);
        assert!(taken.iter().filter(|owner| owner.is_some()).count() >= 2, "{taken:?}");
    }

    #[test]
    fn bots_shoot_a_person_standing_in_front_of_them() {
        let (fired, hurt) = bot_in_front_of_a_person(false);
        assert!(fired && hurt, "the bot never hit the person in front of it");
    }

    /// Bots carry short guns where fights are close and long ones where they
    /// are far, with a second gun for the other end.
    #[test]
    fn bots_pick_guns_for_the_map() {
        for id in 0..map::MAP_NAMES.len() as u8 {
            for mode in 0..modes::count() {
                if map::plays(id, mode) {
                    let range = typical_range(&Map::for_game(id, mode), mode);
                    println!("{} / {}: {range:.1} m", map::MAP_NAMES[id as usize], modes::name(mode));
                }
            }
        }
        let share = |range: f32, guns: &[WeaponId]| {
            let mut rng = 99;
            let picks: Vec<(WeaponId, WeaponId)> = (0..2000).map(|_| bot_loadout(range, &[], &mut rng)).collect();
            assert!(picks.iter().all(|(a, b)| a != b && *a < GUN_COUNT && *b < GUN_COUNT));
            picks.iter().filter(|(primary, _)| guns.contains(primary)).count() as f32 / picks.len() as f32
        };
        let long = [weapons::DMR, weapons::SNIPER];
        let short = [weapons::SMG, weapons::SHOTGUN, weapons::PISTOL];
        assert!(share(10.0, &short) > 0.55, "close quarters: {}", share(10.0, &short));
        assert!(share(10.0, &[weapons::SNIPER]) < 0.03);
        assert!(share(55.0, &long) > 0.55, "open ground: {}", share(55.0, &long));
        assert!(share(55.0, &[weapons::SHOTGUN]) < 0.02);
        // A sniper's second gun is for up close.
        let mut rng = 5;
        for _ in 0..200 {
            let (primary, secondary) = bot_loadout(65.0, &[], &mut rng);
            if primary == weapons::SNIPER {
                assert!(bot_gun_band(secondary).0 < 10.0, "sniper with {}", weapon(secondary).name);
            }
        }
    }

    /// A bot draws its other gun when it suits the fight better, or when
    /// the one in hand is empty with an enemy close; and with nobody about,
    /// loads both and goes back to the first.
    #[test]
    fn bots_change_guns_for_the_fight() {
        let mut bot = Bot::new();
        bot.gun = weapons::SNIPER;
        bot.ammo = weapon(bot.gun).magazine;
        bot.spare = weapons::PISTOL;
        bot.spare_ammo = weapon(bot.spare).magazine;
        bot.primary = weapons::SNIPER;
        assert!(!bot_wants_spare(&bot, Some(50.0)), "pistol for a far fight");
        assert!(bot_wants_spare(&bot, Some(4.0)), "sniper up close");
        assert!(!bot_wants_spare(&bot, None), "both loaded, first in hand");
        bot.gun = weapons::RIFLE;
        bot.primary = weapons::RIFLE;
        bot.spare = weapons::SMG;
        assert!(!bot_wants_spare(&bot, Some(10.0)), "swapping between two that both do");
        bot.ammo = 0;
        assert!(bot_wants_spare(&bot, Some(10.0)), "reloading with an enemy close");
        bot.ammo = weapon(bot.gun).magazine;
        bot.spare_ammo = 3;
        assert!(bot_wants_spare(&bot, None), "the other left half empty");
        std::mem::swap(&mut bot.gun, &mut bot.spare);
        std::mem::swap(&mut bot.ammo, &mut bot.spare_ammo);
        bot.ammo = weapon(bot.gun).magazine;
        assert!(bot_wants_spare(&bot, None), "back to the first");
    }

    /// Nothing in the way, a bot sees an enemy in front of it however far.
    #[test]
    fn bots_see_as_far_as_there_is_a_clear_line() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        // High above the map, where nothing is in the way.
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::new(0.0, 300.0, 0.0);
        player.yaw = 0.0;
        player.team = 0;
        let mut bot = Bot::new();
        let far = Body {
            id: 2,
            team: 1,
            center: Vec3::new(0.0, 300.0, -250.0),
        };
        let mut rng = 1;
        let seen = bot_pick_enemy(&server.map, &[], 1, &player, &mut bot, &[far], false, bot_skill(BOT_NORMAL), &mut rng, 0.1);
        assert_eq!(seen, Some(Vec3::new(0.0, 300.0, -250.0)));
    }

    /// Running makes footsteps, a step a stride; crouching or standing, none.
    #[test]
    fn running_makes_footsteps() {
        let steps_made = |crouched: bool, speed: f32| {
            let mut server = Server::bind(ServerSettings {
                port: 0,
                ..ServerSettings::default()
            })
            .unwrap();
            let mut person = server.new_player("Runner".into(), None);
            person.alive = true;
            person.pos = Vec3::new(0.0, 50.0, 0.0);
            person.step_from = person.pos;
            person.flags = if crouched { FLAG_CROUCH } else { 0 };
            server.players.insert(1000, person);
            let dt = 1.0 / 60.0;
            let mut steps = 0;
            // Five seconds.
            for _ in 0..300 {
                server.players.get_mut(&1000).unwrap().pos.x += speed * dt;
                server.update_footsteps(dt);
                steps += server.noises.drain(..).filter(|noise| noise.kind == NoiseKind::Step).count();
            }
            steps
        };
        let running = steps_made(false, 7.0);
        // 35 m at a step every 2.6.
        assert!((12..=14).contains(&running), "{running} steps");
        assert_eq!(steps_made(true, 4.0), 0, "crouched");
        assert_eq!(steps_made(false, 0.0), 0, "standing");
    }

    /// Having killed somebody close by, with nobody else about, a bot goes
    /// over to the body and crouches up and down on it, then gets up and
    /// goes about its business. Killed from far off, or with a noise to go
    /// and see, it does not bother.
    #[test]
    fn bots_teabag_who_they_killed_close_by() {
        // (crouched on the body at some point, ended up standing)
        let gloat = |apart: f32, noise: bool| {
            let mut server = Server::bind(ServerSettings {
                port: 0,
                bots: 1,
                ..ServerSettings::default()
            })
            .unwrap();
            server.join_counter += 1;
            let human = server.new_player("Victim".into(), None);
            server.players.insert(1000, human);
            server.start_match();
            server.countdown = 0.0;
            let bot = server.players.iter().find(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).unwrap();
            // A spawn point with open ground `apart` metres in some direction.
            let (bot_pos, victim_pos) = server
                .map
                .spawns
                .iter()
                .flat_map(|spawn| (0..16).map(move |step| (spawn.pos, step as f32 * std::f32::consts::TAU / 16.0)))
                .map(|(from, yaw)| (from, from + map::look_dir(yaw, 0.0) * apart))
                .find(|(from, to)| {
                    server.map.is_free(*to, map::PLAYER_HALF)
                        && server.map.line_of_sight(*from, *to)
                        && server.map.line_of_sight(*from - Vec3::Y * 0.8, *to - Vec3::Y * 0.8)
                        // Standing on something, not in the air.
                        && !server.map.is_free(*to - Vec3::Y * 0.2, map::PLAYER_HALF)
                })
                .expect("no open ground on the map");
            server.players.get_mut(&bot).unwrap().pos = bot_pos;
            let victim = server.players.get_mut(&1000).unwrap();
            victim.pos = victim_pos;
            victim.armor = 0;
            server.damage(1000, bot, 1000, weapons::RIFLE);
            assert!(!server.players[&1000].alive);
            if noise {
                server.make_noise(1000, victim_pos + Vec3::X * 20.0, NoiseKind::Shot);
                // Somebody else's, really: the dead make no noise.
                server.noises.last_mut().unwrap().source = 2000;
            }
            let mut bobbed = false;
            for _ in 0..60 * 8 {
                server.update_bots(1.0 / 60.0);
                let player = &server.players[&bot];
                let crouched = player.flags & FLAG_CROUCH != 0;
                let on_body = (player.pos - victim_pos).with_y(0.0).length() < 1.0;
                bobbed |= crouched && on_body;
            }
            (bobbed, server.players[&bot].flags & FLAG_CROUCH == 0)
        };
        assert_eq!(gloat(6.0, false), (true, true), "killed close by");
        assert_eq!(gloat(30.0, false), (false, true), "killed far off");
        assert_eq!(gloat(6.0, true), (false, true), "heard a shot");
    }

    /// A bot hears an enemy's step close by, and goes to see roughly where
    /// it was; a shot from much further, and knows better where it was.
    /// Teammates' noises, far steps, and noises while it fights, it ignores.
    #[test]
    fn bots_hear_steps_and_shots_and_go_to_see() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::ZERO;
        player.team = 0;
        let noise = |id: u64, team: u8, pos: Vec3, kind: NoiseKind| Noise {
            id,
            source: 2,
            team,
            pos,
            kind,
            age: 0.0,
        };
        let guessed = |noises: &[Noise], team_mode: bool, rng: &mut u64| {
            let mut bot = Bot::new();
            bot_listen(1, &player, &mut bot, noises, team_mode, false, rng);
            bot.investigate
        };
        let mut rng = 7;
        let step = Vec3::new(0.0, 0.0, -10.0);
        for _ in 0..200 {
            let guess = guessed(&[noise(1, 1, step, NoiseKind::Step)], true, &mut rng).expect("not heard");
            let off = guess.with_y(0.0).angle_between(step);
            let distance = guess.with_y(0.0).length();
            assert!(off <= STEP_GUESS.0 + 1e-3, "{off} rad off");
            assert!((6.0 - 1e-3..=14.0 + 1e-3).contains(&distance), "{distance} m");
        }
        let shot = Vec3::new(80.0, 0.0, 0.0);
        let guess = guessed(&[noise(1, 1, shot, NoiseKind::Shot)], true, &mut rng).expect("shot not heard");
        assert!(guess.with_y(0.0).angle_between(shot) <= SHOT_GUESS.0 + 1e-3);
        assert_eq!(guessed(&[noise(1, 0, step, NoiseKind::Step)], true, &mut rng), None, "a teammate");
        assert!(guessed(&[noise(1, 0, step, NoiseKind::Step)], false, &mut rng).is_some(), "everyone's an enemy");
        assert_eq!(guessed(&[noise(1, 1, step * 2.0, NoiseKind::Step)], true, &mut rng), None, "steps 20 m off");
        // The shot wins over a step, wherever they are.
        let both = [noise(1, 1, step, NoiseKind::Step), noise(2, 1, shot, NoiseKind::Shot)];
        let guess = guessed(&both, true, &mut rng).unwrap();
        assert!(guess.with_y(0.0).angle_between(shot) < 0.2);
        // Busy fighting.
        let mut bot = Bot::new();
        bot.enemy = Some(3);
        bot_listen(1, &player, &mut bot, &both, true, false, &mut rng);
        assert_eq!(bot.investigate, None);
        assert_eq!(bot.last_noise, 2, "heard them anyway");
    }

    /// A grenade going off is a noise where it went off, made by whoever
    /// threw it: a frag or a flash a bang heard from far off, a smoke a
    /// quieter pop.
    #[test]
    fn grenades_going_off_are_heard() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        server.start_match();
        let mut thrower = server.new_player("Thrower".into(), None);
        thrower.team = 1;
        server.players.insert(1000, thrower);
        let at = Vec3::new(0.0, 300.0, 0.0);
        for (k, kind) in [FRAG, FLASH, SMOKE].into_iter().enumerate() {
            server.explode(Grenade {
                id: k as u32,
                kind,
                owner: 1000,
                pos: at,
                vel: Vec3::ZERO,
                fuse: 0.0,
            });
        }
        let heard: Vec<(NoiseKind, PlayerId, u8, Vec3)> = server.noises.iter().map(|n| (n.kind, n.source, n.team, n.pos)).collect();
        assert_eq!(
            heard,
            [
                (NoiseKind::Bang, 1000, 1, at),
                (NoiseKind::Bang, 1000, 1, at),
                (NoiseKind::Pop, 1000, 1, at)
            ]
        );
        // A bot 120 m off hears the bangs but not the pop.
        let mut listener = server.new_player("Bot".into(), None);
        listener.team = 0;
        let mut rng = 4;
        let mut bot = Bot::new();
        listener.pos = at + Vec3::X * 120.0;
        bot_listen(1, &listener, &mut bot, &server.noises[2..], true, false, &mut rng);
        assert_eq!(bot.investigate, None, "a smoke from 120 m");
        let mut bot = Bot::new();
        bot_listen(1, &listener, &mut bot, &server.noises[..1], true, false, &mut rng);
        assert!(bot.investigate.is_some(), "a frag from 120 m");
    }

    /// Having heard something behind it, a bot turns that way.
    #[test]
    fn a_bot_turns_to_what_it_heard() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::new(0.0, 300.0, 0.0);
        player.yaw = 0.0;
        let mut bot = Bot::new();
        bot.investigate = Some(Vec3::new(0.0, 300.0, 12.0));
        bot.investigate_for = BOT_INVESTIGATE_TIME;
        let mut rng = 3;
        for _ in 0..30 {
            bot_move(&server.map, &server.nav, &mut player, &mut bot, None, None, &mut rng, 1.0 / 60.0);
        }
        assert!(angle_between(player.yaw, std::f32::consts::PI).abs() < 0.2, "facing {}", player.yaw);
        assert_eq!(bot.goal.with_y(0.0), Vec3::new(0.0, 0.0, 12.0));
    }

    /// Someone running across the bot's view 40 m away: it aims ahead of
    /// them, in the way they are running.
    #[test]
    fn bots_lead_a_running_target() {
        let server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        let mut player = server.new_player("Bot".into(), None);
        player.pos = Vec3::ZERO;
        player.yaw = 0.0;
        player.pitch = 0.0;
        let mut bot = Bot::new();
        bot.gun = weapons::SMG;
        bot.ammo = weapon(bot.gun).magazine;
        // Long settled on them, running to the right (+x) at 7 m/s.
        bot.sight = 10.0;
        bot.enemy_vel = Vec3::X * 7.0;
        let enemy = Vec3::new(0.0, 0.0, -40.0);
        let to = aim_point(enemy) - bot_eye(&player);
        let straight = (-to.x).atan2(-to.z);
        // Its aim wobbles, so on average over ten seconds (after turning).
        let mut rng = 12345;
        let mut off = 0.0;
        for tick in 0..60 * 11 {
            bot_fight(1, &mut player, &mut bot, Some(enemy), false, bot_skill(BOT_EASY), &mut rng, 1.0 / 60.0);
            if tick >= 60 {
                off += angle_between(straight, player.yaw) / (60.0 * 10.0);
            }
        }
        // Yaw grows to the left, so aiming right of them is a smaller yaw.
        // Running 7 m/s, a bullet's 0.27 s flight ahead is 0.047 rad.
        assert!(off < -0.02, "not leading: {off}");
    }

    #[test]
    fn bots_holding_fire_never_shoot() {
        let (fired, hurt) = bot_in_front_of_a_person(true);
        assert!(!fired && !hurt, "the bot fired anyway");
    }

    #[test]
    fn only_the_leader_tells_the_bots_to_hold_fire() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        for id in [1, 2] {
            server.join_counter += 1;
            let player = server.new_player(format!("P{id}"), None);
            server.players.insert(id, player);
        }
        let leader = server.leader().unwrap();
        let other = if leader == 1 { 2 } else { 1 };
        server.handle_message(other, ClientMsg::SetBotsHoldFire(true));
        assert!(!server.settings.bots_hold_fire, "only the leader may");
        server.handle_message(leader, ClientMsg::SetBotsHoldFire(true));
        let ServerMsg::Roster { bots_hold_fire, .. } = server.roster_msg() else {
            panic!("not a roster");
        };
        assert!(bots_hold_fire);
        server.start_match();
        server.handle_message(leader, ClientMsg::SetBotsHoldFire(false));
        assert!(server.settings.bots_hold_fire, "not during a match");
    }

    #[test]
    fn only_the_leader_picks_how_well_bots_fight() {
        let mut server = Server::bind(ServerSettings {
            port: 0,
            ..ServerSettings::default()
        })
        .unwrap();
        for id in [1, 2] {
            server.join_counter += 1;
            let player = server.new_player(format!("P{id}"), None);
            server.players.insert(id, player);
        }
        let leader = server.leader().unwrap();
        let other = if leader == 1 { 2 } else { 1 };
        assert_eq!(server.settings.bot_difficulty, BOT_NORMAL);
        server.handle_message(other, ClientMsg::SetBotDifficulty(BOT_HARD));
        assert_eq!(server.settings.bot_difficulty, BOT_NORMAL, "only the leader may");
        server.handle_message(leader, ClientMsg::SetBotDifficulty(BOT_DIFFICULTY_NAMES.len() as u8));
        assert_eq!(server.settings.bot_difficulty, BOT_NORMAL, "no such level");
        server.handle_message(leader, ClientMsg::SetBotDifficulty(BOT_EASY));
        let ServerMsg::Roster { bot_difficulty, .. } = server.roster_msg() else {
            panic!("not a roster");
        };
        assert_eq!(bot_difficulty, BOT_EASY);
        server.start_match();
        server.handle_message(leader, ClientMsg::SetBotDifficulty(BOT_HARD));
        assert_eq!(server.settings.bot_difficulty, BOT_EASY, "not during a match");
    }

    /// Each level is at least as good as the one below it, in every way.
    #[test]
    fn harder_bots_are_better_at_everything() {
        for pair in BOT_SKILLS.windows(2) {
            let (worse, better) = (&pair[0], &pair[1]);
            assert!(better.reaction <= worse.reaction && better.reaction_jitter <= worse.reaction_jitter);
            assert!(better.spot_time <= worse.spot_time && better.turn_rate >= worse.turn_rate);
            assert!(better.aim_error <= worse.aim_error && better.far_doubling >= worse.far_doubling);
            assert!(better.aim_slop <= worse.aim_slop && better.settle <= worse.settle);
            assert!(better.easy_track >= worse.easy_track && better.tracking <= worse.tracking);
            assert!(better.lead.1 - better.lead.0 <= worse.lead.1 - worse.lead.0);
        }
    }

    /// Five seconds of a bot facing a person 12 m away across open ground,
    /// both held in place: whether it fired, and whether it hurt them.
    fn bot_in_front_of_a_person(hold_fire: bool) -> (bool, bool) {
        let settings = ServerSettings {
            port: 0,
            bots: 1,
            bots_hold_fire: hold_fire,
            ..ServerSettings::default()
        };
        let mut server = Server::bind(settings).unwrap();
        server.join_counter += 1;
        let human = server.new_player("Target".into(), None);
        server.players.insert(1000, human);
        server.start_match();
        server.countdown = 0.0;
        let bot = server.players.iter().find(|(_, p)| p.bot.is_some()).map(|(id, _)| *id).unwrap();
        // A spawn point with open ground 12 m in some direction.
        let (bot_pos, target_pos) = server
            .map
            .spawns
            .iter()
            .flat_map(|spawn| {
                (0..16).map(|step| {
                    let yaw = step as f32 * std::f32::consts::TAU / 16.0;
                    (spawn.pos, spawn.pos + map::look_dir(yaw, 0.0) * 12.0)
                })
            })
            .find(|(from, to)| {
                let eye = from.with_y(from.y + 0.7);
                server.map.is_free(*to, map::PLAYER_HALF)
                    && server.map.line_of_sight(eye, aim_point(*to))
                    && server.map.line_of_sight(eye, *to - Vec3::Y * 0.5)
            })
            .expect("no open line on the map");
        let to = target_pos - bot_pos;
        server.players.get_mut(&bot).unwrap().yaw = (-to.x).atan2(-to.z);
        let (mut fired, mut hurt) = (false, false);
        for _ in 0..60 * 5 {
            // Both stay put: the bot may turn and shoot but not walk away.
            server.players.get_mut(&bot).unwrap().pos = bot_pos;
            server.players.get_mut(&1000).unwrap().pos = target_pos;
            server.update_match(1.0 / 60.0);
            fired |= !server.bullets.is_empty();
            let target = &server.players[&1000];
            hurt |= target.armor < START_ARMOR || target.deaths > 0;
        }
        (fired, hurt)
    }
}
