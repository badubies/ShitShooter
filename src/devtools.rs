//! Developer helpers: F12 screenshots, and a scripted self-test (`--selftest <dir>`)
//! that plays a short match against bots by itself and saves screenshots.

use std::{collections::VecDeque, path::PathBuf, time::SystemTime};

use bevy::{
    camera::{ImageRenderTarget, NormalizedRenderTarget, RenderTarget, ShadowLodOrigin, visibility::RenderLayers},
    render::render_resource::{TextureFormat, TextureUsages},
    picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
    prelude::*,
    window::{PrimaryWindow, WindowRef},
    ui_widgets::{SetSliderValue, Slider, SliderValueChange},
    render::view::screenshot::{Screenshot, save_to_disk},
};

use crate::{
    AppState, LaunchArgs,
    menu::MenuPage,
    net::{Connection, MenuStatus, Session},
    paint::{MyPaint, PlayerModel},
    paint_editor::PaintEditor,
    player::{Controls, DroppedMagazine, GunSlot, LocalPlayer, PlayerSystems},
    protocol::{ClientMsg, MODE_CONQUEST},
    map::PLAYER_HALF,
    loadout::{Choice, LoadoutScreen},
    remote::{Corpse, RemotePlayer},
    weapons::{GUN_COUNT, IRON_SIGHTS, RIFLE, SHOTGUN, TWO_X, WeaponId, weapon},
    ui::{MatchChoice, PickerOpen},
    world::{GameMap, MainCamera},
    settings::{Settings, SettingsMenu, SettingsTab},
};

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, screenshot_key);

        let selftest = app.world().resource::<LaunchArgs>().selftest.clone();
        if let Some(dir) = selftest {
            let dir = PathBuf::from(dir);
            let _ = std::fs::create_dir_all(&dir);
            app.insert_resource(SelfTest {
                dir,
                clock: 0.0,
                top_speed: 0.0,
                step: 0,
                canvas: None,
                pictures: VecDeque::new(),
            })
            .add_systems(First, draw_to_canvas)
            .add_systems(Last, take_picture)
            // Sounds are still created and decoded, just not heard.
            .insert_resource(GlobalVolume::new(bevy::audio::Volume::SILENT))
            .init_resource::<CueCount>()
            .add_systems(Update, count_cues)
            .add_systems(OnEnter(AppState::Playing), restart_clock)
            .add_systems(OnEnter(AppState::Lobby), restart_clock)
            .add_systems(
                Update,
                (
                    selftest_menu.run_if(in_state(AppState::Menu)),
                    selftest_lobby.run_if(in_state(AppState::Lobby)),
                    selftest_play
                        .before(PlayerSystems::Simulate)
                        .run_if(in_state(AppState::Playing)),
                ),
            );
        }
    }
}

fn screenshot_key(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::F12) {
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let _ = std::fs::create_dir_all("screenshots");
        let path = format!("screenshots/shot-{stamp}.png");
        info!("saving {path}");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}

#[derive(Resource)]
struct SelfTest {
    dir: PathBuf,
    /// Seconds spent in the current state.
    clock: f32,
    top_speed: f32,
    /// How far through the current state's script we are.
    step: u32,
    /// What the main camera draws into instead of the window (see
    /// `draw_to_canvas`), once the window's size is known.
    canvas: Option<ImageRenderTarget>,
    /// Pictures asked for and not yet taken: one is taken a frame, as a
    /// second picture of the same frame would be dropped.
    pictures: VecDeque<String>,
}

impl SelfTest {
    /// True once, the first time the clock passes `at` for script step `step`.
    fn reached(&mut self, step: u32, at: f32) -> bool {
        if self.step == step && self.clock >= at {
            self.step += 1;
            true
        } else {
            false
        }
    }

    /// Takes a picture, saved as `name`, this frame or soon after.
    fn capture(&mut self, name: &str) {
        self.pictures.push_back(name.to_string());
    }

    /// Where the pointer's clicks go: the canvas, where the menus are drawn.
    fn pointer_target(&self, window: Entity) -> NormalizedRenderTarget {
        match &self.canvas {
            Some(canvas) => NormalizedRenderTarget::Image(canvas.clone()),
            None => NormalizedRenderTarget::Window(WindowRef::Primary.normalize(Some(window)).expect("a window")),
        }
    }
}

/// Shows the canvas in the window, to watch the self-test by: a sprite the
/// size of the window, seen by a camera of its own on a layer of its own.
#[derive(Component)]
struct CanvasView;

const CANVAS_VIEW_LAYER: usize = 30;

/// Draws the main camera, and so the HUD and the menus, into an image the
/// size of the window, which the pictures are taken from, and shows that
/// image in the window. A window that cannot be seen is not drawn (macOS
/// skips one that is covered), so the pictures come out whatever is on
/// screen.
fn draw_to_canvas(
    mut commands: Commands,
    mut test: ResMut<SelfTest>,
    mut images: ResMut<Assets<Image>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<Entity, With<MainCamera>>,
    mut view: Query<&mut Sprite, With<CanvasView>>,
) {
    let size = window.physical_size();
    let scale_factor = window.scale_factor();
    let same = test.canvas.as_ref().is_some_and(|canvas| {
        canvas.scale_factor == scale_factor && images.get(&canvas.handle).is_some_and(|image| image.size() == size)
    });
    if same || size.min_element() == 0 {
        return;
    }
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8Unorm, Some(TextureFormat::Rgba8UnormSrgb));
    // So the pictures can be read back from it.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let canvas = ImageRenderTarget {
        handle: images.add(image),
        scale_factor,
    };
    commands
        .entity(*camera)
        // The UI is drawn by the camera that targets the window unless told
        // otherwise, and so is the detail of point lights' shadows.
        .insert((RenderTarget::Image(canvas.clone()), IsDefaultUiCamera, ShadowLodOrigin));
    // A 2D camera's units are the window's logical pixels.
    let sprite = Sprite {
        image: canvas.handle.clone(),
        custom_size: Some(Vec2::new(window.width(), window.height())),
        ..default()
    };
    if let Ok(mut view) = view.single_mut() {
        *view = sprite;
    } else {
        let layer = RenderLayers::layer(CANVAS_VIEW_LAYER);
        commands.spawn((Camera2d, Camera { order: 1, ..default() }, layer.clone()));
        // The real pointer, on the window, clicks nothing: the menus are on the canvas.
        commands.spawn((CanvasView, sprite, layer, Pickable::IGNORE));
    }
    test.canvas = Some(canvas);
}

fn take_picture(mut commands: Commands, mut test: ResMut<SelfTest>) {
    let Some(canvas) = test.canvas.clone() else {
        return;
    };
    if let Some(name) = test.pictures.pop_front() {
        // The camera's target exactly, scale factor and all: the picture is
        // taken from the camera that draws into it.
        commands
            .spawn(Screenshot(RenderTarget::Image(canvas)))
            .observe(save_to_disk(test.dir.join(name)));
    }
}

fn restart_clock(mut test: ResMut<SelfTest>) {
    test.clock = 0.0;
    test.step = 0;
}

fn selftest_menu(
    time: Res<Time>,
    status: Res<MenuStatus>,
    mut page: ResMut<MenuPage>,
    mut settings_menu: ResMut<SettingsMenu>,
    mut test: ResMut<SelfTest>,
    mut exit: MessageWriter<AppExit>,
) {
    test.clock += time.delta_secs();
    if test.reached(0, 1.5) {
        println!("[selftest] menu status: '{}'", status.0);
        test.capture("menu.png");
    }
    // Each of the other pages, a moment after it is put up.
    let pages = [
        (MenuPage::Online, "menu_online.png"),
        (MenuPage::Host, "menu_host.png"),
        (MenuPage::Join, "menu_join.png"),
    ];
    for (index, (to, file)) in pages.into_iter().enumerate() {
        let at = 2.0 + index as f32 * 0.8;
        if test.reached(1 + 2 * index as u32, at) {
            *page = to;
        }
        if test.reached(2 + 2 * index as u32, at + 0.6) {
            test.capture(file);
        }
    }
    // The settings' controls page (the match takes a picture of the other).
    let end = 2.0 + pages.len() as f32 * 0.8;
    let step = 1 + 2 * pages.len() as u32;
    if test.reached(step, end) {
        *page = MenuPage::Home;
        settings_menu.tab = SettingsTab::Controls;
        settings_menu.open = true;
    }
    if test.reached(step + 1, end + 0.6) {
        test.capture("settings_controls.png");
    }
    if test.reached(step + 2, end + 1.0) {
        settings_menu.open = false;
        settings_menu.tab = SettingsTab::General;
        exit.write(AppExit::Success);
    }
}

fn selftest_lobby(
    mut commands: Commands,
    time: Res<Time>,
    session: Res<Session>,
    mut test: ResMut<SelfTest>,
    mut connection: ResMut<Connection>,
    mut exit: MessageWriter<AppExit>,
    mut paint: ResMut<MyPaint>,
    mut editor: ResMut<PaintEditor>,
    model: Res<PlayerModel>,
    sliders: Query<Entity, With<Slider>>,
    (settings, map, mut picker): (Res<Settings>, Res<GameMap>, ResMut<PickerOpen>),
    mut loadout: ResMut<LoadoutScreen>,
    choices: Query<(&Choice, &UiGlobalTransform, &ComputedNode)>,
    match_choices: Query<(&MatchChoice, &UiGlobalTransform)>,
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    mut pointer: MessageWriter<PointerInput>,
) {
    test.clock += time.delta_secs();
    // The second visit to the lobby is after the match was ended: we are done.
    if session.last_result.is_some() {
        if test.reached(0, 1.0) {
            test.capture("lobby_after.png");
        }
        if test.reached(1, 2.0) {
            println!("[selftest] finished: {:?}", session.last_result);
            exit.write(AppExit::Success);
        }
        return;
    }
    if test.reached(0, 0.5) {
        connection.send(&ClientMsg::AddBot);
        connection.send(&ClientMsg::AddBot);
        connection.send(&ClientMsg::AddBot);
        // Play on the second map, which also exercises switching maps.
        connection.send(&ClientMsg::NextMap);
        // The lobby's picture shows the two teams.
        connection.send(&ClientMsg::SetMode(1));
    }
    if test.reached(1, 1.5) {
        test.capture("lobby.png");
    }
    // The mode and map are picked in the match picker, open while they are clicked.
    picker.0 = (1.6..3.0).contains(&test.clock);
    // The lobby's chips and cards, clicked: team deathmatch on Arena, then
    // back to free for all on Depot. Each click is checked once the server
    // has answered.
    let scale = window.1.scale_factor();
    let card_at = |target: Choice| {
        choices.iter().find(|(choice, ..)| **choice == target).map(|(_, at, _)| at.translation / scale)
    };
    let chip_at = |target: MatchChoice| {
        match_choices.iter().find(|(choice, _)| **choice == target).map(|(_, at)| at.translation / scale)
    };
    let clicks = [MatchChoice::Mode(1), MatchChoice::Map(0), MatchChoice::Mode(0), MatchChoice::Map(1)];
    for (index, target) in clicks.into_iter().enumerate() {
        let at = 1.7 + index as f32 * 0.3;
        // Picking sees where the pointer is a frame after it moves there.
        if test.reached(2 + 2 * index as u32, at) {
            if index == 2 {
                println!("[selftest] after clicking Team deathmatch and Arena: mode {} map {}", session.mode, map.0.id);
            }
            point(&test.pointer_target(window.0), &mut pointer, chip_at(target), false);
        }
        if test.reached(3 + 2 * index as u32, at + 0.1) {
            point(&test.pointer_target(window.0), &mut pointer, chip_at(target), true);
        }
    }
    if test.reached(10, 2.9) {
        println!("[selftest] after clicking the chips and cards: mode {} map {}", session.mode, map.0.id);
        point(&test.pointer_target(window.0), &mut pointer, None, false);
        test.capture("lobby_picker.png");
    }
    // Stripes on the paint job, and a look at them on the paint screen.
    if test.reached(11, 3.2) {
        let mut canvas = paint.canvas.clone();
        for (pixel, texel) in canvas.0.iter_mut().zip(&model.texels) {
            if let Some((point, _)) = texel
                && (point.y * 6.0).floor() as i32 % 2 == 0
            {
                *pixel = 1;
            }
        }
        paint.set(canvas);
        editor.open = true;
    }
    // The brush size slider (the only slider there is) moved to 12 cm.
    if test.reached(12, 3.7) {
        for entity in &sliders {
            commands.trigger(SetSliderValue {
                entity,
                change: SliderValueChange::Absolute(12.0),
            });
        }
    }
    if test.reached(13, 4.2) {
        test.capture("paint.png");
    }
    if test.reached(14, 4.5) {
        editor.open = false;
    }
    // The loadout screen, with the pointer: on the shotgun's card (the stage
    // shows it), a click on it, and back to the rifle.
    if test.reached(15, 4.8) {
        loadout.open = true;
    }
    if test.reached(16, 5.4) {
        point(&test.pointer_target(window.0), &mut pointer, card_at(Choice::Gun(0, SHOTGUN)), false);
    }
    if test.reached(17, 5.8) {
        test.capture("loadout_hover.png");
    }
    // Picking sees where the pointer is a frame after it moves there.
    if test.reached(18, 6.0) {
        point(&test.pointer_target(window.0), &mut pointer, card_at(Choice::Gun(0, SHOTGUN)), true);
    }
    if test.reached(19, 6.3) {
        println!("[selftest] after a click on the shotgun's card: {}", weapon(settings.primary).name);
        test.capture("loadout_clicked.png");
        point(&test.pointer_target(window.0), &mut pointer, card_at(Choice::Gun(0, RIFLE)), false);
    }
    if test.reached(20, 6.4) {
        point(&test.pointer_target(window.0), &mut pointer, card_at(Choice::Gun(0, RIFLE)), true);
    }
    if test.reached(21, 6.7) {
        println!("[selftest] and on the rifle's: {}", weapon(settings.primary).name);
        point(&test.pointer_target(window.0), &mut pointer, None, false);
        loadout.open = false;
    }
    if test.reached(22, 6.8) {
        println!("[selftest] roster: {:?}", session.roster.iter().map(|p| &p.name).collect::<Vec<_>>());
        connection.send(&ClientMsg::StartMatch);
    }
}

/// Moves the pointer to `position` (in the window's pixels; or off to a
/// corner, for `None`), and clicks there if `click`. A click only lands on
/// what was under the pointer the frame before.
fn point(target: &NormalizedRenderTarget, pointer: &mut MessageWriter<PointerInput>, position: Option<Vec2>, click: bool) {
    let position = position.unwrap_or(Vec2::splat(5.0));
    let location = Location {
        target: target.clone(),
        position,
    };
    let mut actions = vec![PointerAction::Move { delta: Vec2::ZERO }];
    if click {
        actions.extend([PointerAction::Press(PointerButton::Primary), PointerAction::Release(PointerButton::Primary)]);
    }
    for action in actions {
        pointer.write(PointerInput::new(PointerId::Mouse, location.clone(), action));
    }
}

/// Runs around, aims at the nearest bot and holds the trigger, and tries
/// every other move once along the way.
fn selftest_play(
    time: Res<Time>,
    session: Res<Session>,
    mut test: ResMut<SelfTest>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut player: ResMut<LocalPlayer>,
    mut controls: ResMut<Controls>,
    mut settings_menu: ResMut<SettingsMenu>,
    mut connection: ResMut<Connection>,
    remotes: Query<(&RemotePlayer, &Transform)>,
    (corpses, dropped): (Query<(Entity, &Corpse, &Transform)>, Query<(&DroppedMagazine, &Transform)>),
    map: Res<GameMap>,
    cue_count: Res<CueCount>,
    mut last_target: Local<Option<(u64, Vec3)>>,
    mut captured: Local<Captured>,
) {
    // The script starts once the countdown is over.
    if session.frozen() {
        if session.countdown < 2.5 && !captured.countdown {
            captured.countdown = true;
            println!("[selftest] countdown {:.1}, alive {}", session.countdown, player.alive);
            test.capture("countdown.png");
        }
        // Nothing can happen before the match starts: a look at every gun in
        // the first slot with the 2x sight (they all take it), from the hip,
        // aimed, aimed down its iron sights, and part way through a reload from
        // empty (a magazine coming out, or a shell going in), a few frames each.
        if session.countdown < 2.6 && player.alive && !session.team_mode() {
            const FRAMES: u32 = 6;
            const VIEWS: [&str; 4] = ["hip", "aimed", "irons", "reload"];
            let picture = (captured.gallery / FRAMES) as usize;
            let gun = (picture / VIEWS.len()) as WeaponId;
            let view = VIEWS[picture % VIEWS.len()];
            let first = *captured.gallery_gun.get_or_insert(player.guns[0]);
            if gun < GUN_COUNT {
                let stats = weapon(gun);
                player.slot = 0;
                player.guns[0].weapon = gun;
                player.guns[0].sight = if view == "irons" { IRON_SIGHTS } else { TWO_X };
                player.guns[0].ammo = if view == "reload" { 0 } else { stats.magazine };
                player.aim = (view == "aimed" || view == "irons") as u8 as f32;
                let progress = if stats.reload_per_round { 0.3 } else { 0.27 };
                player.reload = if view == "reload" { stats.reload * (1.0 - progress) } else { 0.0 };
                player.reload_empty = view == "reload";
                if captured.gallery % FRAMES == FRAMES - 3 {
                    test.capture(&format!("gun_{}_{view}.png", stats.name.to_lowercase()));
                }
                captured.gallery += 1;
            } else {
                player.guns[0] = first;
                player.aim = 0.0;
                player.reload = 0.0;
                player.reload_empty = false;
            }
        }
        // In the team modes, a look at the nearest teammate: outlined, named.
        let eye = player.eye();
        let mate = remotes
            .iter()
            .filter(|(remote, _)| remote.alive && session.team_mode() && !session.enemies(session.my_id, remote.id))
            .map(|(_, transform)| transform.translation)
            .min_by(|a, b| a.distance(eye).total_cmp(&b.distance(eye)));
        if let Some(mate) = mate
            && session.countdown < 1.8
        {
            let to = mate - eye;
            player.yaw = (-to.x).atan2(-to.z);
            player.pitch = to.y.atan2(to.with_y(0.0).length());
            if session.countdown < 1.2 && !captured.ally {
                captured.ally = true;
                println!("[selftest] nearest teammate {:.1} m away", to.length());
                test.capture("teammate.png");
            }
        }
        return;
    }
    if player.hit_marker > 0.0 && !captured.marker {
        captured.marker = true;
        test.capture("hit.png");
    }
    // Not while the scoreboard is held (below), which hides the death screen.
    let scores_held = (5.0..6.6).contains(&test.clock);
    if player.on_death_screen()
        && player.dead_for > 1.0
        && controls.captured
        && !settings_menu.open
        && !scores_held
        && !captured.dead
    {
        captured.dead = true;
        test.capture("dead.png");
    }
    if let Some(arc) = player.damage_arcs.first()
        && !captured.hurt
    {
        captured.hurt = true;
        println!(
            "[selftest] hurt from {:?}, {:.0} degrees off our aim",
            arc.from,
            player.bearing(arc.at).to_degrees()
        );
        test.capture("hurt.png");
    }
    test.clock += time.delta_secs();
    let t = test.clock;
    test.top_speed = test.top_speed.max(player.horizontal_speed());

    // Standing still on a conquest flag, at the end.
    let on_flag = session.mode == MODE_CONQUEST && test.step >= 14;
    if on_flag {
        keys.release(KeyCode::KeyW);
        keys.release(KeyCode::KeyD);
        if let Some(flag) = map.0.flags.iter().find(|flag| flag.holds(player.pos)) {
            let to = flag.pos + Vec3::Y * 6.0 - player.eye();
            player.yaw = (-to.x).atan2(-to.z);
            player.pitch = to.y.atan2(to.with_y(0.0).length());
        }
    } else {
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyD);
    }
    // Run for a while, then bunny hop, so both footsteps and landings happen.
    if t > 3.5 && !on_flag {
        keys.press(KeyCode::Space);
    } else {
        keys.release(KeyCode::Space);
    }
    if (2.2..2.3).contains(&t) {
        keys.press(KeyCode::ShiftLeft);
    } else {
        keys.release(KeyCode::ShiftLeft);
    }
    // The trigger is held, except with the pistol, which wants a click per
    // shot, and around the clicks that cook and throw grenades.
    let pistol_click = (4.4..5.0).contains(&t) && (t * 4.0).fract() > 0.5;
    let grenade_click = [1.0..1.2, 1.45..1.5, 1.6..2.0, 2.05..2.1].iter().any(|window| window.contains(&t));
    // Near the end, the shotgun is aimed and held there until it is choked.
    let shotgun_aim = (9.3..10.9).contains(&t);
    if pistol_click || grenade_click || shotgun_aim || on_flag {
        mouse.release(MouseButton::Left);
    } else {
        mouse.press(MouseButton::Left);
    }
    if (5.0..6.5).contains(&t) {
        keys.press(KeyCode::Tab);
    } else {
        keys.release(KeyCode::Tab);
    }
    let mut hold = |key: KeyCode, held: bool| {
        if held {
            keys.press(key);
        } else {
            keys.release(key);
        }
    };
    // A frag early on (taken out, a click to pull the pin, and lobbed with a
    // right-click), then the second one cooked and thrown with clicks, a
    // crouch that turns into a slide, a ping, and a reload by hand.
    hold(KeyCode::KeyG, (1.0..1.1).contains(&t) || (1.6..1.75).contains(&t));
    hold(KeyCode::KeyF, (3.0..3.05).contains(&t));
    hold(KeyCode::ControlLeft, (2.35..3.3).contains(&t));
    hold(KeyCode::KeyR, (6.6..6.7).contains(&t));
    // Every weapon of the default loadout in turn: pistol, knife, back to the rifle.
    hold(KeyCode::Digit2, (4.4..4.5).contains(&t));
    hold(KeyCode::Digit3, (5.0..5.1).contains(&t));
    hold(KeyCode::Digit1, (5.6..5.7).contains(&t));
    hold(KeyCode::Digit2, (9.35..9.45).contains(&t));
    if (1.4..2.1).contains(&t) || (9.4..10.9).contains(&t) {
        mouse.press(MouseButton::Right);
    } else {
        mouse.release(MouseButton::Right);
    }
    if test.reached(0, 1.9) {
        println!("[selftest] aim {:.2} ammo {} grenades {:.2?}", player.aim, player.guns[0].ammo, player.grenades);
        test.capture("aim.png");
    }
    if test.reached(1, 2.7) {
        println!(
            "[selftest] crouching {} sliding {} speed {:.1} eye height {:.2}",
            player.crouching,
            player.sliding,
            player.horizontal_speed(),
            player.eye().y - player.feet().y
        );
        test.capture("slide.png");
    }

    let eye = player.eye();
    let nearest = remotes
        .iter()
        .filter(|(remote, _)| remote.alive && session.enemies(session.my_id, remote.id))
        .map(|(remote, transform)| (remote.id, transform.translation))
        .min_by(|a, b| a.1.distance(eye).total_cmp(&b.1.distance(eye)));
    if let Some((id, target)) = nearest
        && !on_flag
    {
        // Bullets take time to arrive: aim where the target will be by then.
        let velocity = match *last_target {
            Some((last_id, last_pos)) if last_id == id && time.delta_secs() > 0.0 => {
                ((target - last_pos) / time.delta_secs()).clamp_length_max(12.0)
            }
            _ => Vec3::ZERO,
        };
        *last_target = Some((id, target));
        let flight = target.distance(eye) / player.weapon().speed.max(1.0);
        let to = target + velocity * flight - eye;
        player.yaw = (-to.x).atan2(-to.z);
        player.pitch = to.y.atan2(to.with_y(0.0).length());
    }

    // The first body to fall: we stand next to it while it falls, while it
    // lies there and while it sinks.
    const SHOTS: [(f32, &str); 3] = [(0.25, "body_falling.png"), (1.5, "body.png"), (8.9, "body_sinking.png")];
    let first = captured.body.or_else(|| {
        corpses
            .iter()
            .find(|(_, corpse, _)| corpse.age < 0.1 && player.alive)
            .map(|(entity, ..)| entity)
    });
    captured.body = first;
    if first.is_some() && !player.alive && captured.body_shots < SHOTS.len() {
        // The camera stays where we died: no more pictures of it.
        println!("[selftest] died while looking at a body");
        captured.body_shots = SHOTS.len();
    }
    if let Some((_, corpse, transform)) = first.and_then(|entity| corpses.get(entity).ok()) {
        // Where it fell: it sinks below the floor at the end.
        let lies = *captured.body_at.get_or_insert(transform.translation);
        // The rest of the script waits until the pictures are taken.
        if captured.body_shots < SHOTS.len() {
            test.clock -= time.delta_secs();
        }
        if let Some(&(at, name)) = SHOTS.get(captured.body_shots)
            && corpse.age > at - 0.2
        {
            // A free spot a few metres to one side of it, with a clear view of it.
            let side = transform.rotation * Vec3::X;
            let spot = (0..16)
                .map(|i| {
                    let around = Quat::from_rotation_y(i as f32 * std::f32::consts::TAU / 16.0);
                    lies + around * side * 3.5 + Vec3::Y * (PLAYER_HALF.y + 0.05)
                })
                .find(|spot| map.0.is_free(*spot, PLAYER_HALF) && map.0.line_of_sight(*spot, lies + Vec3::Y * 0.3));
            if let Some(spot) = spot {
                player.teleport(spot);
                let to = lies + Vec3::Y * 0.3 - player.eye();
                player.yaw = (-to.x).atan2(-to.z);
                player.pitch = to.y.atan2(to.with_y(0.0).length());
            }
            if corpse.age > at {
                captured.body_shots += 1;
                let seen = if spot.is_some() { "seen" } else { "no clear view" };
                println!("[selftest] body {:.1} s old, {seen}", corpse.age);
                test.capture(name);
            }
        }
    }

    // The weapon panel: its strip of slots just after a switch, and the ring
    // while reloading.
    if t > 5.75 && !captured.switch {
        captured.switch = true;
        test.capture("panel_switch.png");
    }
    if dropped.iter().any(|(magazine, _)| !magazine.landed) && !captured.drop {
        captured.drop = true;
        test.capture("magazine_drop.png");
    }
    if t > 7.0 && player.reloading() && !captured.reload {
        captured.reload = true;
        test.capture("panel_reload.png");
    }

    if test.reached(2, 3.3) {
        test.capture("ping.png");
    }
    if test.reached(3, 3.6) {
        test.capture("play.png");
    }
    if test.reached(4, 6.0) {
        test.capture("scores.png");
    }
    if test.reached(5, 7.5) {
        controls.captured = false;
    }
    if test.reached(6, 8.0) {
        test.capture("pause.png");
    }
    // A screenshot is taken a frame or two after it is asked for, so the
    // settings open a moment later, not at once.
    if test.reached(7, 8.3) {
        settings_menu.open = true;
    }
    if test.reached(8, 8.8) {
        test.capture("settings.png");
    }
    if test.reached(9, 9.2) {
        settings_menu.open = false;
    }
    if test.reached(10, 9.3) {
        controls.captured = true;
        player.guns[1].weapon = SHOTGUN;
        player.guns[1].ammo = weapon(SHOTGUN).magazine;
    }
    if test.reached(11, 10.0) {
        println!("[selftest] shotgun aim {:.2} choke {:.2} spread {:.3}", player.aim, player.choke(), player.spread());
        test.capture("choke_open.png");
    }
    if test.reached(12, 10.6) {
        println!("[selftest] shotgun aim {:.2} choke {:.2} spread {:.3}", player.aim, player.choke(), player.spread());
        test.capture("choke.png");
    }
    // Conquest: onto the nearest flag, looking at its pole, and a look once
    // it is partly ours.
    let conquest = session.mode == MODE_CONQUEST;
    if test.reached(13, 10.9)
        && conquest
        && let Some(flag) = map.0.flags.iter().min_by(|a, b| a.pos.distance(player.pos).total_cmp(&b.pos.distance(player.pos)))
    {
        // From the side: the cloth hangs out towards +x.
        let side = flag.spawns.iter().max_by(|a, b| (a.pos.z - flag.pos.z).abs().total_cmp(&(b.pos.z - flag.pos.z).abs()));
        player.teleport(side.map_or(flag.spawns[0].pos, |spawn| spawn.pos));
        let to = flag.pos + Vec3::Y * 4.0 - player.eye();
        player.yaw = (-to.x).atan2(-to.z);
        player.pitch = to.y.atan2(to.with_y(0.0).length());
    }
    if test.reached(14, if conquest { 17.0 } else { 11.0 }) && conquest {
        println!("[selftest] flags {:?}", session.flags);
        test.capture("flag.png");
    }
    // The death screen with its map to pick where to come back in (only on
    // our side: the server still has us alive).
    if test.reached(15, if conquest { 17.3 } else { 11.1 }) && conquest {
        player.alive = false;
        player.killed_by = Some("[bot] Clanker".to_string());
    }
    if test.reached(16, if conquest { 18.0 } else { 11.15 }) && conquest {
        test.capture("deploy.png");
    }
    if test.reached(17, if conquest { 18.5 } else { 11.2 }) {
        let me = session.roster.iter().find(|p| p.id == session.my_id);
        println!(
            "[selftest] pos {:.1?} top speed {:.1} health {} armor {} me {:?} ping {:.1}ms",
            player.pos,
            test.top_speed,
            player.health,
            player.armor,
            me.map(|p| (p.kills, p.deaths)),
            connection.ping_ms()
        );
        println!(
            "[selftest] ammo {} reloading {} grenades {:.2?}",
            player.guns[0].ammo,
            player.reloading(),
            player.grenades
        );
        println!("[selftest] sound cues (own, others): {:?}", *cue_count);
        println!("[selftest] dropped magazines: {:?}", dropped.iter().map(|(magazine, at)| (magazine.landed, at.translation.y)).collect::<Vec<_>>());
        println!("[selftest] mode {} team scores {:?} my team {}", session.mode, session.team_scores, session.team_of(session.my_id));
        connection.send(&ClientMsg::EndMatch);
    }
}

/// The screenshots of `selftest_play` taken when something first happens.
#[derive(Default)]
struct Captured {
    marker: bool,
    countdown: bool,
    ally: bool,
    hurt: bool,
    dead: bool,
    switch: bool,
    reload: bool,
    drop: bool,
    /// The first body to fall, and how many pictures of it have been taken.
    body: Option<Entity>,
    body_shots: usize,
    body_at: Option<Vec3>,
    /// Frames into the look at every gun, and the gun to put back after it.
    gallery: u32,
    gallery_gun: Option<GunSlot>,
}

/// Tallies the sound cues of a self-test run: (own, others') per kind.
#[derive(Resource, Default, Debug)]
struct CueCount {
    gunshots: (u32, u32),
    hits: (u32, u32),
    kills: (u32, u32),
    footsteps: (u32, u32),
    landings: (u32, u32),
    reloads: (u32, u32),
    throws: (u32, u32),
    bounces: (u32, u32),
    explosions: (u32, u32),
    armor: (u32, u32),
    /// Countdown beeps, pings and a choke closing.
    signals: (u32, u32),
}

fn count_cues(mut cues: MessageReader<crate::audio::SoundCue>, mut count: ResMut<CueCount>) {
    use crate::audio::Sound;
    for cue in cues.read() {
        let slot = match cue.sound {
            Sound::Gunshot(_) => &mut count.gunshots,
            Sound::Hit | Sound::Headshot => &mut count.hits,
            Sound::Kill => &mut count.kills,
            Sound::Footstep(_) => &mut count.footsteps,
            Sound::Land(_) => &mut count.landings,
            Sound::Draw
            | Sound::Rustle
            | Sound::MagOut(_)
            | Sound::MagIn(_)
            | Sound::Bolt(_)
            | Sound::MagDrop(..)
            | Sound::ShellIn
            | Sound::Pump
            | Sound::BoltBack
            | Sound::BoltHome => {
                &mut count.reloads
            }
            Sound::Pin | Sound::Throw => &mut count.throws,
            Sound::Bounce(..) | Sound::Roll(..) => &mut count.bounces,
            Sound::Explosion | Sound::Flashbang | Sound::SmokePop => &mut count.explosions,
            Sound::Ringing => &mut count.explosions,
            Sound::ArmorBreak | Sound::ArmorBroken | Sound::ArmorPickup => &mut count.armor,
            Sound::Countdown | Sound::Go | Sound::Ping | Sound::Choke => &mut count.signals,
        };
        if cue.at.is_none() {
            slot.0 += 1;
        } else {
            slot.1 += 1;
        }
    }
}
