//! Player settings: mouse, field of view, volume, window and key bindings, plus the
//! name and address last typed into the menu. They are saved to a small text
//! file so they survive restarts, and edited in the settings panel that opens
//! from the main menu, the lobby and the pause menu.

use std::path::PathBuf;

use bevy::{
    audio::Volume,
    ecs::system::SystemParam,
    input::{
        InputSystems,
        mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    },
    input_focus::InputFocus,
    picking::hover::Hovered,
    prelude::*,
    ui_widgets::{
        Activate, Button, Slider, SliderRange, SliderStep, SliderThumb, SliderValue, TrackClick, ValueChange,
        observe, slider_self_update,
    },
    window::{PrimaryWindow, WindowMode},
};

use crate::{
    AppState, LaunchArgs,
    player::Controls,
    protocol::{FRAG, GRENADE_KINDS, GRENADES, GrenadeKind},
    weapons::{
        DMR, GUN_COUNT, IRON_SIGHTS, PISTOL, REFLEX, RIFLE, SCOPE, SIGHTS, SNIPER, SightId, TWO_X, WeaponId, next_gun,
        sight, step_gun, sight_on, sights_for, takes_sights, weapon,
    },
    ui::{self, ACCENT, BUTTON, DIM, TEXT},
};

/// The lobby server the main menu lists until another is typed in: set when
/// the game is built (the release workflow takes it from the repository
/// variable `SHITSHOOTER_LOBBY_SERVER`), or none.
pub const DEFAULT_LOBBY_SERVER: &str = match option_env!("SHITSHOOTER_LOBBY_SERVER") {
    Some(address) => address,
    None => "",
};

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let args = app.world().resource::<LaunchArgs>();
        // The scripted self-test always runs with the defaults and saves nothing.
        let settings = if args.selftest.is_some() {
            Settings::default()
        } else {
            Settings::load()
        };
        // Set before the window is opened, so the game does not start in a
        // window and then jump to full screen.
        let window_mode = settings.window_mode();
        let world = app.world_mut();
        for mut window in world.query::<&mut Window>().iter_mut(world) {
            window.mode = window_mode;
        }
        app.insert_resource(settings)
            .init_resource::<SettingsMenu>()
            .init_resource::<WheelSteps>()
            .add_systems(PreUpdate, read_wheel.after(InputSystems))
            .add_systems(OnExit(AppState::Playing), close_menu)
            .add_systems(
                Update,
                (
                    escape_key.run_if(not(in_state(AppState::Playing))),
                    capture_bind,
                    fullscreen_key,
                    sync_panel,
                    style_panel,
                    place_dial_thumbs,
                    update_panel,
                    apply_volume,
                    apply_display,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------- bindings

/// Everything the player can do that has a key or mouse button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Crouch,
    Dash,
    Fire,
    Aim,
    Reload,
    Grenade,
    Ping,
    Primary,
    Secondary,
    Knife,
    NextWeapon,
    PreviousWeapon,
    Scores,
    Minimap,
}

impl Action {
    pub const ALL: [Action; 19] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Jump,
        Action::Crouch,
        Action::Dash,
        Action::Fire,
        Action::Aim,
        Action::Reload,
        Action::Grenade,
        Action::Ping,
        Action::Primary,
        Action::Secondary,
        Action::Knife,
        Action::NextWeapon,
        Action::PreviousWeapon,
        Action::Scores,
        Action::Minimap,
    ];

    /// (name in the settings panel, name in the settings file, default binding)
    fn info(self) -> (&'static str, &'static str, Bind) {
        use Bind::{Key, Mouse, WheelDown, WheelUp};
        match self {
            Action::Forward => ("Move forward", "forward", Key(KeyCode::KeyW)),
            Action::Back => ("Move back", "back", Key(KeyCode::KeyS)),
            Action::Left => ("Move left", "left", Key(KeyCode::KeyA)),
            Action::Right => ("Move right", "right", Key(KeyCode::KeyD)),
            Action::Jump => ("Jump", "jump", Key(KeyCode::Space)),
            Action::Crouch => ("Crouch / slide", "crouch", Key(KeyCode::ControlLeft)),
            Action::Dash => ("Dash", "dash", Key(KeyCode::ShiftLeft)),
            Action::Fire => ("Fire", "fire", Mouse(MouseButton::Left)),
            Action::Aim => ("Aim down sights", "aim", Mouse(MouseButton::Right)),
            Action::Reload => ("Reload", "reload", Key(KeyCode::KeyR)),
            Action::Grenade => ("Grenade", "grenade", Key(KeyCode::KeyG)),
            Action::Ping => ("Ping enemy", "ping", Key(KeyCode::KeyF)),
            Action::Primary => ("Primary gun", "primary", Key(KeyCode::Digit1)),
            Action::Secondary => ("Secondary gun", "secondary", Key(KeyCode::Digit2)),
            Action::Knife => ("Knife", "knife", Key(KeyCode::Digit3)),
            Action::NextWeapon => ("Next weapon", "next_weapon", WheelDown),
            Action::PreviousWeapon => ("Previous weapon", "previous_weapon", WheelUp),
            Action::Scores => ("Scoreboard", "scores", Key(KeyCode::Tab)),
            Action::Minimap => ("Toggle minimap", "minimap", Key(KeyCode::KeyM)),
        }
    }

    pub fn label(self) -> &'static str {
        self.info().0
    }
}

/// How many keys or buttons each action can have.
const BIND_SLOTS: usize = 2;

/// Shown for an action with nothing bound.
const UNBOUND: &str = "-";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Bind {
    Key(KeyCode),
    Mouse(MouseButton),
    /// One notch of the scroll wheel: "pressed" for the frame it turns.
    WheelUp,
    WheelDown,
}

/// Keys that can be bound. Escape (pause) and F12 (screenshot) are reserved.
#[rustfmt::skip]
const BINDABLE_KEYS: &[KeyCode] = {
    use KeyCode::*;
    &[
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
        KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
        Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11,
        ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
        Space, Tab, Enter, Backspace, CapsLock,
        ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight,
        Backquote, Minus, Equal, BracketLeft, BracketRight, Backslash,
        Semicolon, Quote, Comma, Period, Slash,
        Insert, Delete, Home, End, PageUp, PageDown,
        Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
        NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadEnter, NumpadDecimal,
    ]
};

const BINDABLE_BUTTONS: &[(MouseButton, &str)] = &[
    (MouseButton::Left, "MouseLeft"),
    (MouseButton::Right, "MouseRight"),
    (MouseButton::Middle, "MouseMiddle"),
    (MouseButton::Back, "MouseBack"),
    (MouseButton::Forward, "MouseForward"),
];

impl Bind {
    /// The name used in the settings file, e.g. `KeyW`, `ShiftLeft`, `MouseRight`.
    fn file_name(self) -> String {
        match self {
            Bind::Key(key) => format!("{key:?}"),
            Bind::Mouse(button) => BINDABLE_BUTTONS
                .iter()
                .find(|(b, _)| *b == button)
                .map_or("MouseLeft", |(_, name)| name)
                .to_string(),
            Bind::WheelUp => "WheelUp".to_string(),
            Bind::WheelDown => "WheelDown".to_string(),
        }
    }

    fn parse(name: &str) -> Option<Bind> {
        match name {
            "WheelUp" => return Some(Bind::WheelUp),
            "WheelDown" => return Some(Bind::WheelDown),
            _ => {}
        }
        if let Some((button, _)) = BINDABLE_BUTTONS.iter().find(|(_, n)| *n == name) {
            return Some(Bind::Mouse(*button));
        }
        BINDABLE_KEYS
            .iter()
            .find(|key| format!("{key:?}") == name)
            .map(|key| Bind::Key(*key))
    }

    /// The name shown to the player, e.g. `W`, `LEFT SHIFT`, `MOUSE RIGHT`.
    pub fn display(self) -> String {
        let name = self.file_name();
        let spaced = |rest: &str, prefix: &str| format!("{prefix} {}", rest.to_uppercase());
        if let Some(rest) = name.strip_prefix("Mouse") {
            spaced(rest, "MOUSE")
        } else if let Some(rest) = name.strip_prefix("Wheel") {
            spaced(rest, "WHEEL")
        } else if let Some(rest) = name.strip_prefix("Key").or(name.strip_prefix("Digit")) {
            rest.to_string()
        } else if let Some(rest) = name.strip_prefix("Numpad") {
            spaced(rest, "NUM")
        } else if let Some(rest) = name.strip_prefix("Arrow") {
            spaced(rest, "ARROW")
        } else if let Some(rest) = name.strip_suffix("Left") {
            spaced(rest, "LEFT")
        } else if let Some(rest) = name.strip_suffix("Right") {
            spaced(rest, "RIGHT")
        } else {
            name.to_uppercase()
        }
    }
}

// ---------------------------------------------------------------- the settings

#[derive(Resource, Clone, PartialEq, Debug)]
pub struct Settings {
    /// Last name, join address and host port typed into the main menu.
    pub name: String,
    pub address: String,
    pub port: String,
    /// The lobby server (`hub.rs`) whose lobbies the main menu lists.
    pub lobby_server: String,
    /// Mouse sensitivity: thousandths of a radian per pixel of mouse movement.
    pub sensitivity: f32,
    /// Extra sensitivity factor while aiming down sights.
    pub ads_sensitivity: f32,
    /// Vertical field of view in degrees.
    pub fov: f32,
    /// Master volume, 0 to 1.
    pub volume: f32,
    /// How big the HUD is, on top of the size the whole interface is scaled to
    /// for the window (see `ui::scale_hud`): 1 is as designed.
    pub ui_scale: f32,
    /// Borderless full screen, rather than a window.
    pub fullscreen: bool,
    /// The map in the top left corner while playing (see `minimap.rs`).
    pub show_minimap: bool,
    /// The guns carried in the two slots. Never the same gun twice.
    pub primary: WeaponId,
    pub secondary: WeaponId,
    /// The sight picked for each gun, by weapon id. Guns without a sight
    /// mount always have their iron sights.
    pub sights: [SightId; GUN_COUNT as usize],
    /// The kind of grenade carried.
    pub grenade: GrenadeKind,
    /// Each action's main binding and its optional second one. The main one is
    /// only empty when the action has nothing bound at all.
    binds: [[Option<Bind>; BIND_SLOTS]; Action::ALL.len()],
    /// False during the self-test, which must not touch the player's file.
    persist: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: "Player".into(),
            address: "127.0.0.1".into(),
            port: crate::protocol::DEFAULT_PORT.to_string(),
            lobby_server: DEFAULT_LOBBY_SERVER.to_string(),
            sensitivity: 2.2,
            ads_sensitivity: 1.0,
            fov: 75.0,
            volume: 0.8,
            ui_scale: 1.0,
            fullscreen: false,
            show_minimap: true,
            primary: RIFLE,
            secondary: PISTOL,
            sights: default_sights(),
            grenade: FRAG,
            binds: Action::ALL.map(|action| [Some(action.info().2), None]),
            persist: false,
        }
    }
}

/// A number in the settings panel that is changed with a slider.
#[derive(Clone, Copy, PartialEq)]
enum Dial {
    Sensitivity,
    AdsSensitivity,
    Fov,
    Volume,
    UiScale,
}

impl Dial {
    const ALL: [Dial; 5] = [Dial::Sensitivity, Dial::AdsSensitivity, Dial::Fov, Dial::Volume, Dial::UiScale];

    /// (label, step, minimum, maximum)
    fn info(self) -> (&'static str, f32, f32, f32) {
        match self {
            Dial::Sensitivity => ("Mouse sensitivity", 0.1, 0.2, 12.0),
            Dial::AdsSensitivity => ("Aim sensitivity", 0.05, 0.2, 2.0),
            Dial::Fov => ("Field of view", 5.0, 50.0, 100.0),
            Dial::Volume => ("Volume", 0.05, 0.0, 1.0),
            Dial::UiScale => ("HUD size", 0.05, 0.75, 1.5),
        }
    }
}

impl Settings {
    /// What is bound in one of an action's slots (0 = main, 1 = second).
    pub fn bind(&self, action: Action, slot: usize) -> Option<Bind> {
        self.binds[action as usize][slot]
    }

    /// Everything bound to an action.
    pub fn binds(&self, action: Action) -> impl Iterator<Item = Bind> + '_ {
        self.binds[action as usize].iter().flatten().copied()
    }

    /// The name of an action's main binding, for hints on screen.
    pub fn key_name(&self, action: Action) -> String {
        self.bind(action, 0).map_or_else(|| UNBOUND.to_string(), Bind::display)
    }

    /// The names of all of an action's bindings, e.g. `W / ARROW UP`.
    pub fn key_names(&self, action: Action) -> String {
        let names: Vec<String> = self.binds(action).map(Bind::display).collect();
        if names.is_empty() { UNBOUND.to_string() } else { names.join(" / ") }
    }

    /// Puts `bind` in one of `action`'s slots. Whatever slot held that key
    /// before (this action's other one, or another action's) gets what this
    /// slot held, so a key is never bound twice. Taking a key into an empty
    /// second slot leaves its old action without it.
    fn set_bind(&mut self, action: Action, slot: usize, bind: Bind) {
        let previous = self.binds[action as usize][slot];
        if let Some(other) = self.binds.iter_mut().flatten().find(|b| **b == Some(bind)) {
            *other = previous;
        }
        self.binds[action as usize][slot] = Some(bind);
        self.tidy_binds();
    }

    fn clear_bind(&mut self, action: Action, slot: usize) {
        self.binds[action as usize][slot] = None;
        self.tidy_binds();
    }

    /// A lone binding always sits in the main slot.
    fn tidy_binds(&mut self) {
        for slots in &mut self.binds {
            if slots[0].is_none() {
                slots.swap(0, 1);
            }
        }
    }

    fn dial(&mut self, dial: Dial) -> &mut f32 {
        match dial {
            Dial::Sensitivity => &mut self.sensitivity,
            Dial::AdsSensitivity => &mut self.ads_sensitivity,
            Dial::Fov => &mut self.fov,
            Dial::Volume => &mut self.volume,
            Dial::UiScale => &mut self.ui_scale,
        }
    }

    fn dial_text(&self, dial: Dial) -> String {
        match dial {
            Dial::Sensitivity => format!("{:.1}", self.sensitivity),
            Dial::AdsSensitivity => format!("x {:.2}", self.ads_sensitivity),
            Dial::Fov => format!("{:.0}", self.fov),
            Dial::Volume => format!("{:.0}%", self.volume * 100.0),
            Dial::UiScale => format!("{:.0}%", self.ui_scale * 100.0),
        }
    }

    /// Puts every number back inside its allowed range.
    fn clamp(&mut self) {
        for dial in Dial::ALL {
            let (_, _, min, max) = dial.info();
            let value = self.dial(dial);
            *value = if value.is_finite() { value.clamp(min, max) } else { min };
        }
    }

    fn window_mode(&self) -> WindowMode {
        if self.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        }
    }

    /// Mouse, sound and keys go back to their defaults; who you are, what you
    /// carry and the window stay as they are.
    fn reset_controls(&mut self) {
        let defaults = Settings::default();
        *self = Settings {
            name: std::mem::take(&mut self.name),
            address: std::mem::take(&mut self.address),
            port: std::mem::take(&mut self.port),
            lobby_server: std::mem::take(&mut self.lobby_server),
            fullscreen: self.fullscreen,
            show_minimap: self.show_minimap,
            primary: self.primary,
            secondary: self.secondary,
            sights: self.sights,
            grenade: self.grenade,
            persist: self.persist,
            ..defaults
        };
    }

    /// Puts the next gun in a slot (0 = primary, 1 = secondary).
    pub fn cycle_gun(&mut self, slot: usize) {
        self.step_gun(slot, 1);
    }

    /// Puts the gun `step` places on in the list in a slot (back, for a
    /// negative step), never the one in the other slot.
    pub fn step_gun(&mut self, slot: usize, step: i32) {
        if slot == 0 {
            self.primary = step_gun(self.primary, self.secondary, step);
        } else {
            self.secondary = step_gun(self.secondary, self.primary, step);
        }
    }

    /// Puts `gun` in a slot, unless it is the gun in the other one.
    pub fn set_gun(&mut self, slot: usize, gun: WeaponId) {
        if gun >= GUN_COUNT || gun == self.gun(1 - slot.min(1)) {
            return;
        }
        if slot == 0 {
            self.primary = gun;
        } else {
            self.secondary = gun;
        }
    }

    /// Puts `sight` on the gun in a slot, if it takes it.
    pub fn set_sight(&mut self, slot: usize, sight: SightId) {
        let gun = self.gun(slot);
        if sight != IRON_SIGHTS && !sights_for(gun).contains(&sight) {
            return;
        }
        if let Some(on) = self.sights.get_mut(gun as usize) {
            *on = sight;
        }
    }

    pub fn set_grenade(&mut self, kind: GrenadeKind) {
        if (kind as usize) < GRENADE_KINDS {
            self.grenade = kind;
        }
    }

    /// The gun in a slot (0 = primary, 1 = secondary).
    pub fn gun(&self, slot: usize) -> WeaponId {
        if slot == 0 { self.primary } else { self.secondary }
    }

    /// Picks the next kind of grenade to carry.
    pub fn cycle_grenade(&mut self) {
        self.step_grenade(1);
    }

    /// Picks the kind of grenade `step` places on (back, for a negative step).
    pub fn step_grenade(&mut self, step: i32) {
        self.grenade = (self.grenade as i32 + step).rem_euclid(GRENADE_KINDS as i32) as GrenadeKind;
    }

    /// The sight a gun is handed out with.
    pub fn sight_for(&self, gun: WeaponId) -> SightId {
        sight_on(gun, self.sights.get(gun as usize).copied().unwrap_or(IRON_SIGHTS))
    }

    /// Reads the settings file. Anything missing or unreadable keeps its default.
    fn load() -> Settings {
        let text = settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        Settings {
            persist: true,
            ..Settings::parse(&text)
        }
    }

    pub fn save(&self) {
        if !self.persist {
            return;
        }
        let Some(path) = settings_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(err) = std::fs::write(&path, self.to_text()) {
            warn!("could not save settings to {}: {err}", path.display());
        }
    }

    fn parse(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let number = value.parse::<f32>().ok();
            match key {
                "name" => settings.name = value.to_string(),
                "address" => settings.address = value.to_string(),
                "port" => settings.port = value.to_string(),
                // Left empty: the default, which a later build may have.
                "lobby_server" if !value.is_empty() => settings.lobby_server = value.to_string(),
                "sensitivity" => settings.sensitivity = number.unwrap_or(settings.sensitivity),
                "ads_sensitivity" => settings.ads_sensitivity = number.unwrap_or(settings.ads_sensitivity),
                "fov" => settings.fov = number.unwrap_or(settings.fov),
                "volume" => settings.volume = number.unwrap_or(settings.volume),
                "ui_scale" => settings.ui_scale = number.unwrap_or(settings.ui_scale),
                "fullscreen" => settings.fullscreen = value.parse().unwrap_or(settings.fullscreen),
                "minimap" => settings.show_minimap = value.parse().unwrap_or(settings.show_minimap),
                "primary" => settings.primary = gun_named(value).unwrap_or(settings.primary),
                "secondary" => settings.secondary = gun_named(value).unwrap_or(settings.secondary),
                "grenade" => {
                    if let Some(kind) = GRENADES.iter().position(|g| g.name.eq_ignore_ascii_case(value)) {
                        settings.grenade = kind as GrenadeKind;
                    }
                }
                _ if key.starts_with("sight.") => {
                    let gun = gun_named(&key["sight.".len()..]);
                    let picked = SIGHTS.iter().position(|s| s.name.eq_ignore_ascii_case(value));
                    if let (Some(gun), Some(picked)) = (gun, picked) {
                        settings.sights[gun as usize] = picked as SightId;
                    }
                }
                _ => {
                    let action = key
                        .strip_prefix("bind.")
                        .and_then(|name| Action::ALL.into_iter().find(|a| a.info().1 == name));
                    if let Some(action) = action {
                        settings.parse_binds(action, value);
                    }
                }
            }
        }
        settings.clamp();
        if settings.secondary == settings.primary {
            settings.secondary = next_gun(settings.secondary, settings.primary);
        }
        settings
    }

    /// Reads `bind.<action> = Main, Second` (or one key, or nothing for unbound).
    fn parse_binds(&mut self, action: Action, value: &str) {
        let binds: Vec<Bind> = value.split(',').filter_map(|name| Bind::parse(name.trim())).collect();
        // Nothing readable in a line that is not empty: keep the default.
        if binds.is_empty() && !value.is_empty() {
            return;
        }
        for slot in 0..BIND_SLOTS {
            // Swapped in, so that a key saved for one action never ends
            // up doubling the default of an action added since.
            match binds.get(slot) {
                Some(bind) => self.set_bind(action, slot, *bind),
                None => self.clear_bind(action, slot),
            }
        }
    }

    fn to_text(&self) -> String {
        let mut text = format!(
            "name = {}\naddress = {}\nport = {}\nlobby_server = {}\nsensitivity = {}\nads_sensitivity = {}\nfov = {}\nvolume = {}\nui_scale = {}\n",
            self.name,
            self.address,
            self.port,
            self.lobby_server,
            self.sensitivity,
            self.ads_sensitivity,
            self.fov,
            self.volume,
            self.ui_scale
        );
        text.push_str(&format!(
            "fullscreen = {}\nminimap = {}\nprimary = {}\nsecondary = {}\ngrenade = {}\n",
            self.fullscreen,
            self.show_minimap,
            weapon(self.primary).name,
            weapon(self.secondary).name,
            GRENADES[self.grenade as usize % GRENADE_KINDS].name
        ));
        for gun in (0..GUN_COUNT).filter(|gun| takes_sights(*gun)) {
            text.push_str(&format!("sight.{} = {}
", weapon(gun).name, sight(self.sights[gun as usize]).name));
        }
        for action in Action::ALL {
            let names: Vec<String> = self.binds(action).map(Bind::file_name).collect();
            text.push_str(&format!("bind.{} = {}\n", action.info().1, names.join(", ")));
        }
        text
    }
}

/// The rifle comes with the reflex, the DMR with the 2x and the sniper with a
/// scope; every other gun with its iron sights.
fn default_sights() -> [SightId; GUN_COUNT as usize] {
    let mut sights = [IRON_SIGHTS; GUN_COUNT as usize];
    sights[RIFLE as usize] = REFLEX;
    sights[DMR as usize] = TWO_X;
    sights[SNIPER as usize] = SCOPE;
    sights
}

fn gun_named(name: &str) -> Option<WeaponId> {
    (0..GUN_COUNT).find(|id| weapon(*id).name.eq_ignore_ascii_case(name))
}

/// `%APPDATA%\ShitShooter\settings.txt` (the same for every copy of the exe, so
/// a new build keeps your settings), or next to the exe if that is not available.
pub fn settings_path() -> Option<PathBuf> {
    let config_dir = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    config_dir
        .map(|dir| dir.join("ShitShooter"))
        .or_else(|| std::env::current_exe().ok()?.parent().map(PathBuf::from))
        .map(|dir| dir.join("settings.txt"))
}

// ---------------------------------------------------------------- reading input

/// The player's bound keys and buttons. Everything reads as "not pressed"
/// while the game is paused.
#[derive(SystemParam)]
pub struct ActionInput<'w> {
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    wheel: Res<'w, WheelSteps>,
    settings: Res<'w, Settings>,
    controls: Res<'w, Controls>,
}

impl ActionInput<'_> {
    pub fn pressed(&self, action: Action) -> bool {
        self.controls.captured
            && self.settings.binds(action).any(|bind| match bind {
                Bind::Key(key) => self.keys.pressed(key),
                Bind::Mouse(button) => self.mouse.pressed(button),
                wheel => self.wheel.step == Some(wheel),
            })
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        self.controls.captured
            && self.settings.binds(action).any(|bind| match bind {
                Bind::Key(key) => self.keys.just_pressed(key),
                Bind::Mouse(button) => self.mouse.just_pressed(button),
                wheel => self.wheel.step == Some(wheel),
            })
    }
}

/// The scroll wheel turned into notches: `step` is the one this frame, if any.
#[derive(Resource, Default)]
pub struct WheelSteps {
    step: Option<Bind>,
    /// Scrolled since the last notch. Smooth scrolling (touchpads, free-spinning
    /// wheels) comes in small amounts that add up to a notch.
    owed: f32,
}

/// At most one notch a frame, so a hard flick does not race through every weapon.
fn read_wheel(scroll: Res<AccumulatedMouseScroll>, mut wheel: ResMut<WheelSteps>) {
    let (delta, notch) = match scroll.unit {
        MouseScrollUnit::Line => (scroll.delta.y, 1.0),
        MouseScrollUnit::Pixel => (scroll.delta.y, 40.0),
    };
    // Turning the other way starts afresh.
    if delta * wheel.owed < 0.0 {
        wheel.owed = 0.0;
    }
    wheel.owed += delta;
    wheel.step = None;
    if wheel.owed.abs() >= notch * 0.99 {
        wheel.step = Some(if wheel.owed > 0.0 { Bind::WheelUp } else { Bind::WheelDown });
        wheel.owed = (wheel.owed.abs() - notch).clamp(0.0, notch) * wheel.owed.signum();
    }
}

// ---------------------------------------------------------------- the panel

/// Whether the settings panel is open, on which page, and which binding is
/// waiting for a key.
#[derive(Resource, Default)]
pub struct SettingsMenu {
    pub open: bool,
    pub tab: SettingsTab,
    capturing: Option<Capture>,
}

/// The two pages of the settings.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SettingsTab {
    #[default]
    General,
    Controls,
}

struct Capture {
    action: Action,
    slot: usize,
    /// Set once every key and button has been let go, so the click that
    /// started the capture is not itself taken as the new binding.
    armed: bool,
}

impl SettingsMenu {
    /// What Escape does while the panel is open: cancel a pending rebind, or close.
    pub fn back(&mut self) {
        if self.capturing.take().is_none() {
            self.open = false;
        }
    }
}

#[derive(Component)]
struct SettingsPanel(SettingsTab);
/// The button that shows one page of the settings.
#[derive(Component)]
struct TabButton(SettingsTab);
/// The button for a window (`false`) or borderless full screen (`true`).
#[derive(Component)]
struct WindowChoice(bool);
/// The button that shows (`true`) or hides (`false`) the minimap.
#[derive(Component)]
struct MinimapChoice(bool);
#[derive(Component)]
struct DialSlider(Dial);
/// The part of a dial's rail up to its thumb.
#[derive(Component)]
struct DialFill(Dial);
#[derive(Component)]
struct DialText(Dial);
#[derive(Component)]
struct BindText(Action, usize);

/// Observer for any button that opens the settings panel.
pub fn on_open_settings(_: On<Activate>, mut menu: ResMut<SettingsMenu>) {
    menu.open = true;
}

fn close_menu(mut menu: ResMut<SettingsMenu>) {
    menu.open = false;
    menu.capturing = None;
}

fn escape_key(keys: Res<ButtonInput<KeyCode>>, mut menu: ResMut<SettingsMenu>) {
    if menu.open && keys.just_pressed(KeyCode::Escape) {
        menu.back();
    }
}

/// While a binding waits for input, the next key or mouse button becomes it.
fn capture_bind(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    wheel: Res<WheelSteps>,
    mut menu: ResMut<SettingsMenu>,
    mut settings: ResMut<Settings>,
) {
    let Some(capture) = &mut menu.capturing else {
        return;
    };
    if !capture.armed {
        capture.armed = keys.get_pressed().next().is_none() && mouse.get_pressed().next().is_none();
        return;
    }
    let key = keys
        .get_just_pressed()
        .find(|key| BINDABLE_KEYS.contains(key))
        .map(|key| Bind::Key(*key));
    // Taken when the button comes back up: that release is what clicks a key
    // button in the panel, and it would otherwise start a new capture.
    let button = mouse
        .get_just_released()
        .find(|button| BINDABLE_BUTTONS.iter().any(|(b, _)| b == *button))
        .map(|button| Bind::Mouse(*button));
    if let Some(bind) = key.or(button).or(wheel.step) {
        settings.set_bind(capture.action, capture.slot, bind);
        menu.capturing = None;
    }
}

/// Spawns the panel when it is opened; removes it and saves when it is closed.
fn sync_panel(
    mut commands: Commands,
    mut menu: ResMut<SettingsMenu>,
    settings: Res<Settings>,
    mut focus: ResMut<InputFocus>,
    panel: Query<(Entity, &SettingsPanel)>,
) {
    match (menu.open, panel.iter().next()) {
        (true, None) => {
            // Otherwise a menu text field behind the panel keeps taking the typing.
            focus.clear();
            spawn_panel(&mut commands, &settings, menu.tab);
        }
        // The other page: put it up instead.
        (true, Some((entity, shown))) if shown.0 != menu.tab => {
            commands.entity(entity).despawn();
            menu.capturing = None;
            spawn_panel(&mut commands, &settings, menu.tab);
        }
        (false, Some((entity, _))) => {
            commands.entity(entity).despawn();
            menu.capturing = None;
            settings.save();
        }
        _ => {}
    }
}

fn spawn_panel(commands: &mut Commands, settings: &Settings, tab: SettingsTab) {
    commands
        .spawn((
            SettingsPanel(tab),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Above everything, the pause menu included, and dark enough to hide it.
            GlobalZIndex(20),
            BackgroundColor(ui::SCRIM.with_alpha(1.0)),
        ))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    width: percent(100),
                    max_width: px(1240),
                    height: percent(100),
                    min_height: px(0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(48), px(36)),
                    row_gap: px(22),
                    ..default()
                })
                .with_children(|page| {
                    page.spawn((
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: px(20),
                            ..default()
                        },
                        children![
                            (ui::button("‹  Back"), observe(close)),
                            ui::heading("Settings", 40.0, TEXT),
                            (
                                Node {
                                    flex_grow: 1.0,
                                    justify_content: JustifyContent::FlexEnd,
                                    column_gap: px(6),
                                    ..default()
                                },
                                children![tab_button("General", SettingsTab::General), tab_button("Controls", SettingsTab::Controls)],
                            ),
                        ],
                    ));
                    match tab {
                        SettingsTab::General => general_page(page, settings),
                        SettingsTab::Controls => controls_page(page),
                    }
                    page.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|buttons| {
                        if tab == SettingsTab::Controls {
                            buttons
                                .spawn(ui::button("Reset controls to defaults"))
                                .observe(|_: On<Activate>, mut settings: ResMut<Settings>| settings.reset_controls());
                        } else {
                            buttons.spawn(Node::default());
                        }
                        buttons.spawn((ui::primary_button("Done"), observe(close)));
                    });
                });
        });
}

fn close(_: On<Activate>, mut menu: ResMut<SettingsMenu>) {
    menu.open = false;
}

fn tab_button(text: &str, tab: SettingsTab) -> impl Bundle + use<> {
    (
        TabButton(tab),
        ui::button(text),
        observe(move |_: On<Activate>, mut menu: ResMut<SettingsMenu>| menu.tab = tab),
    )
}

/// A box on a page of the settings, laid out as `node` says.
fn settings_box(node: Node) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(22)),
            ..node
        },
        BackgroundColor(ui::PANEL),
        BorderColor::all(ui::PANEL_EDGE),
        ui::soft_shadow(),
    )
}

/// The mouse, sound, window and HUD.
fn general_page(page: &mut ChildSpawnerCommands, settings: &Settings) {
    page.spawn(Node {
        // Down to the buttons at the bottom.
        flex_grow: 1.0,
        column_gap: px(24),
        align_items: AlignItems::FlexStart,
        ..default()
    })
    .with_children(|row| {
        row.spawn(settings_box(Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            padding: UiRect::axes(px(26), px(12)),
            ..default()
        }))
        .with_children(|dials| {
            for (index, dial) in Dial::ALL.into_iter().enumerate() {
                let (name, step, low, high) = dial.info();
                let value = *settings.clone().dial(dial);
                dials
                    .spawn(Node {
                        height: px(72),
                        align_items: AlignItems::Center,
                        column_gap: px(24),
                        border: UiRect::bottom(px(if index + 1 < Dial::ALL.len() { 1 } else { 0 })),
                        ..default()
                    })
                    .insert(BorderColor::all(ui::PANEL_EDGE))
                    .with_children(|line| {
                        line.spawn((
                            ui::weighted(name, 16.0, 600, TEXT),
                            Node {
                                width: px(190),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                        line.spawn(dial_slider(dial, value, low, high, step))
                            .observe(slider_self_update)
                            .observe(move |change: On<ValueChange<f32>>, mut settings: ResMut<Settings>| {
                                *settings.dial(dial) = change.value;
                                settings.clamp();
                            });
                        line.spawn((
                            Node {
                                width: px(84),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                padding: UiRect::vertical(px(9)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(10)),
                                ..default()
                            },
                            BackgroundColor(ui::FIELD),
                            BorderColor::all(ui::PANEL_EDGE),
                            children![(ui::weighted("", 15.0, 700, TEXT), DialText(dial))],
                        ));
                    });
            }
        });
        row.spawn((
            settings_box(Node {
                width: px(380),
                flex_shrink: 0.0,
                row_gap: px(12),
                padding: UiRect::all(px(22)),
                ..default()
            }),
            children![
                ui::section("Window"),
                (WindowChoice(false), ui::button("Windowed"), observe(pick_window(false))),
                (WindowChoice(true), ui::button("Borderless fullscreen"), observe(pick_window(true))),
                (
                    Node {
                        margin: UiRect::top(px(4)),
                        ..default()
                    },
                    children![ui::label("Alt + Enter switches at any time.", 13.0, DIM)],
                ),
                (
                    Node {
                        margin: UiRect::top(px(14)),
                        ..default()
                    },
                    children![ui::section("Minimap")],
                ),
                (MinimapChoice(true), ui::button("Shown"), observe(pick_minimap(true))),
                (MinimapChoice(false), ui::button("Hidden"), observe(pick_minimap(false))),
                (
                    Node {
                        margin: UiRect::top(px(4)),
                        ..default()
                    },
                    children![ui::label(minimap_hint(settings), 13.0, DIM)],
                ),
            ],
        ));
    });
}

fn pick_minimap(shown: bool) -> impl Fn(On<Activate>, ResMut<Settings>) {
    move |_, mut settings| {
        if settings.show_minimap != shown {
            settings.show_minimap = shown;
        }
    }
}

/// Which key shows and hides the minimap in a game. The page is put up again
/// when the Controls tab is left, so this follows a change of binding there.
fn minimap_hint(settings: &Settings) -> String {
    if settings.binds(Action::Minimap).next().is_some() {
        format!("{} switches it in a game.", settings.key_names(Action::Minimap))
    } else {
        "Bind a key under Controls to switch it in a game.".to_string()
    }
}

fn pick_window(fullscreen: bool) -> impl Fn(On<Activate>, ResMut<Settings>) {
    move |_, mut settings| {
        if settings.fullscreen != fullscreen {
            settings.fullscreen = fullscreen;
        }
    }
}

/// A rail with a round thumb dragged along it, for one of the dials.
fn dial_slider(dial: Dial, value: f32, low: f32, high: f32, step: f32) -> impl Bundle {
    (
        DialSlider(dial),
        Slider {
            track_click: TrackClick::Snap,
            ..default()
        },
        SliderValue(value),
        SliderRange::new(low, high),
        SliderStep(step),
        Hovered::default(),
        Node {
            flex_grow: 1.0,
            height: px(22),
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                Node {
                    width: percent(100),
                    height: px(6),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(BUTTON),
                Pickable::IGNORE,
                children![(
                    DialFill(dial),
                    Node {
                        height: percent(100),
                        border_radius: BorderRadius::all(px(3)),
                        ..default()
                    },
                    BackgroundColor(ACCENT),
                    Pickable::IGNORE,
                )],
            ),
            (
                SliderThumb,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(DIAL_THUMB),
                    height: px(DIAL_THUMB),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(TEXT),
                BoxShadow::new(Color::BLACK.with_alpha(0.5), px(0), px(2), px(0), px(8)),
            ),
        ],
    )
}

const DIAL_THUMB: f32 = 22.0;

/// Puts each dial's thumb where its value is along the rail, and fills the
/// rail up to it.
fn place_dial_thumbs(
    sliders: Query<(&DialSlider, &SliderValue, &SliderRange, &Hovered, &ComputedNode, &Children)>,
    mut thumbs: Query<(&mut Node, &mut BackgroundColor), (With<SliderThumb>, Without<DialFill>)>,
    mut fills: Query<(&DialFill, &mut Node), Without<SliderThumb>>,
) {
    for (dial, value, range, hovered, computed, children) in &sliders {
        let along = range.thumb_position(value.0);
        let width = computed.size().x * computed.inverse_scale_factor;
        for child in children.iter() {
            if let Ok((mut node, mut background)) = thumbs.get_mut(child) {
                let left = px(along * (width - DIAL_THUMB).max(0.0));
                if node.left != left {
                    node.left = left;
                }
                background.set_if_neq(BackgroundColor(if hovered.get() { ACCENT } else { TEXT }));
            }
        }
        for (fill, mut node) in &mut fills {
            let wanted = percent(along * 100.0);
            if fill.0 == dial.0 && node.width != wanted {
                node.width = wanted;
            }
        }
    }
}

/// The key bindings, in three groups.
fn controls_page(page: &mut ChildSpawnerCommands) {
    page.spawn(ui::label(
        "Click a key to change it, then press the new key, mouse button or scroll. Each action takes two. Right-click a key to clear it.",
        14.0,
        DIM,
    ));
    page.spawn(Node {
        flex_grow: 1.0,
        min_height: px(0),
        column_gap: px(18),
        align_items: AlignItems::FlexStart,
        ..default()
    })
    .with_children(|groups| {
        for (title, actions) in CONTROL_GROUPS {
            groups
                .spawn(settings_box(Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    row_gap: px(4),
                    padding: UiRect::axes(px(18), px(16)),
                    ..default()
                }))
                .with_children(|group| {
                    group.spawn((
                        Node {
                            margin: UiRect::bottom(px(6)),
                            ..default()
                        },
                        children![ui::section(title)],
                    ));
                    for &action in *actions {
                        group
                            .spawn(Node {
                                align_items: AlignItems::Center,
                                column_gap: px(6),
                                min_height: px(40),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn((
                                    ui::weighted(action.label(), 14.0, 600, TEXT),
                                    Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    },
                                ));
                                for slot in 0..BIND_SLOTS {
                                    row.spawn((
                                        Button,
                                        Hovered::default(),
                                        ui::ButtonKind::Normal,
                                        Node {
                                            width: px(BIND_BUTTON_WIDTH),
                                            flex_shrink: 0.0,
                                            justify_content: JustifyContent::Center,
                                            padding: UiRect::axes(px(4), px(7)),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(8)),
                                            ..default()
                                        },
                                        BackgroundColor(BUTTON),
                                        BorderColor::all(ui::BUTTON_EDGE),
                                        children![(ui::weighted("", 12.0, 700, TEXT), Pickable::IGNORE, BindText(action, slot))],
                                    ))
                                    .observe(click_bind(action, slot));
                                }
                            });
                    }
                });
        }
    });
}

/// The controls page's groups of actions.
const CONTROL_GROUPS: &[(&str, &[Action])] = &[
    (
        "Move",
        &[Action::Forward, Action::Back, Action::Left, Action::Right, Action::Jump, Action::Crouch, Action::Dash],
    ),
    (
        "Fight",
        &[Action::Fire, Action::Aim, Action::Reload, Action::Grenade, Action::Ping, Action::Scores, Action::Minimap],
    ),
    (
        "Weapons",
        &[Action::Primary, Action::Secondary, Action::Knife, Action::NextWeapon, Action::PreviousWeapon],
    ),
];

/// Each of an action's key buttons.
const BIND_BUTTON_WIDTH: i32 = 104;

/// An observer for a key button: a click waits for the key to put in that
/// slot, a right-click empties it.
fn click_bind(
    action: Action,
    slot: usize,
) -> impl Fn(On<Activate>, Res<ButtonInput<MouseButton>>, ResMut<SettingsMenu>, ResMut<Settings>) {
    move |_, mouse, mut menu, mut settings| {
        // A click while a binding waits is that binding; `capture_bind` takes it.
        if menu.capturing.as_ref().is_some_and(|capture| capture.armed) {
            return;
        }
        if mouse.just_released(MouseButton::Right) {
            settings.clear_bind(action, slot);
            menu.capturing = None;
        } else {
            menu.capturing = Some(Capture {
                action,
                slot,
                armed: false,
            });
        }
    }
}

/// Marks the page shown and the window mode picked, and keeps the sliders
/// where the settings are (Alt + Enter, and a dial's clamping, change them too).
fn style_panel(
    mut commands: Commands,
    menu: Res<SettingsMenu>,
    mut settings: ResMut<Settings>,
    mut buttons: Query<(
        Option<&TabButton>,
        Option<&WindowChoice>,
        Option<&MinimapChoice>,
        &mut ui::ButtonKind,
        &mut BorderColor,
    )>,
    sliders: Query<(Entity, &DialSlider, &SliderValue)>,
) {
    for (tab, window, minimap, mut kind, mut border) in &mut buttons {
        let picked = match (tab, window, minimap) {
            (Some(tab), _, _) => tab.0 == menu.tab,
            (_, Some(window), _) => window.0 == settings.fullscreen,
            (_, _, Some(minimap)) => minimap.0 == settings.show_minimap,
            _ => continue,
        };
        let wanted = if picked { ui::ButtonKind::Picked } else { ui::ButtonKind::Normal };
        if *kind != wanted {
            *kind = wanted;
            *border = BorderColor::all(wanted.edge());
        }
    }
    for (entity, slider, value) in &sliders {
        let wanted = *settings.bypass_change_detection().dial(slider.0);
        if (value.0 - wanted).abs() > 1e-4 {
            commands.entity(entity).insert(SliderValue(wanted));
        }
    }
}

fn update_panel(
    settings: Res<Settings>,
    menu: Res<SettingsMenu>,
    mut dials: Query<(&DialText, &mut Text), Without<BindText>>,
    mut binds: Query<(&BindText, &mut Text, &mut TextColor), Without<DialText>>,
) {
    for (dial, mut text) in &mut dials {
        ui::set_text(&mut text, settings.dial_text(dial.0));
    }
    let capturing = menu.capturing.as_ref().map(|capture| (capture.action, capture.slot));
    for (&BindText(action, slot), mut text, mut color) in &mut binds {
        let waiting = capturing == Some((action, slot));
        let bound = settings.bind(action, slot);
        let shown = if waiting {
            "press a key...".to_string()
        } else {
            bound.map_or_else(|| UNBOUND.to_string(), Bind::display)
        };
        ui::set_text(&mut text, shown);
        let wanted = if waiting {
            ACCENT
        } else if bound.is_some() {
            TEXT
        } else {
            DIM
        };
        if color.0 != wanted {
            color.0 = wanted;
        }
    }
}

/// Alt + Enter switches between a window and full screen, wherever you are.
fn fullscreen_key(keys: Res<ButtonInput<KeyCode>>, menu: Res<SettingsMenu>, mut settings: ResMut<Settings>) {
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    // Not while a binding waits for a key: that key press is for the binding.
    if alt && keys.just_pressed(KeyCode::Enter) && menu.capturing.is_none() {
        settings.fullscreen = !settings.fullscreen;
        settings.save();
    }
}

fn apply_display(settings: Res<Settings>, mut window: Single<&mut Window, With<PrimaryWindow>>) {
    let wanted = settings.window_mode();
    if window.mode != wanted {
        window.mode = wanted;
    }
}

fn apply_volume(settings: Res<Settings>, args: Res<LaunchArgs>, mut global: ResMut<GlobalVolume>) {
    // The self-test runs muted.
    if settings.is_changed() && args.selftest.is_none() {
        // Squared, so the steps sound evenly spaced.
        global.volume = Volume::Linear(settings.volume * settings.volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weapons::{REFLEX, THREE_X, TWO_X};

    #[test]
    fn settings_survive_a_save_and_load() {
        let mut settings = Settings {
            name: "Some = Name".into(),
            address: "example.org:7000".into(),
            lobby_server: "pi.example.org".into(),
            sensitivity: 3.4,
            fov: 90.0,
            volume: 0.35,
            ui_scale: 1.25,
            fullscreen: true,
            show_minimap: false,
            primary: 3,
            secondary: 1,
            sights: [IRON_SIGHTS, REFLEX, IRON_SIGHTS, TWO_X, REFLEX, THREE_X],
            grenade: crate::protocol::SMOKE,
            ..default()
        };
        settings.set_bind(Action::Grenade, 0, Bind::Mouse(MouseButton::Middle));
        settings.set_bind(Action::Crouch, 0, Bind::Key(KeyCode::KeyC));
        settings.set_bind(Action::Forward, 1, Bind::Key(KeyCode::ArrowUp));
        settings.set_bind(Action::Minimap, 0, Bind::Key(KeyCode::KeyN));
        // Taken from reload, which is left with nothing.
        settings.set_bind(Action::Jump, 1, Bind::Key(KeyCode::KeyR));
        assert_eq!(settings.binds(Action::Reload).count(), 0);
        assert_eq!(Settings::parse(&settings.to_text()), settings);
    }

    #[test]
    fn sights_go_only_on_guns_with_a_mount() {
        let shotgun = gun_named("Shotgun").unwrap();
        let smg = gun_named("SMG").unwrap();
        let mut settings = Settings::parse("primary = Shotgun\nsecondary = SMG\nsight.Shotgun = 3x sight\nsight.SMG = REFLEX\n");
        assert_eq!(settings.sight_for(shotgun), IRON_SIGHTS);
        assert_eq!(settings.sight_for(smg), REFLEX);
        assert_eq!(settings.sight_for(PISTOL), IRON_SIGHTS);
        assert_eq!(settings.sight_for(RIFLE), REFLEX, "the rifle's default");
        settings.set_sight(0, THREE_X);
        assert_eq!(settings.sight_for(shotgun), IRON_SIGHTS, "the shotgun takes no 3x");
        settings.set_sight(0, TWO_X);
        assert_eq!(settings.sight_for(shotgun), TWO_X);
        settings.set_sight(0, IRON_SIGHTS);
        assert_eq!(settings.sight_for(shotgun), IRON_SIGHTS);
        settings.set_sight(1, REFLEX);
        assert_eq!(settings.sight_for(smg), REFLEX);
        assert_eq!(settings.sight_for(shotgun), IRON_SIGHTS, "each gun keeps its own");
    }

    #[test]
    fn the_long_guns_take_the_3x_and_the_dmr_and_sniper_the_scope() {
        let mut settings = Settings::parse("primary = Sniper
secondary = Pistol
sight.Pistol = Scope
");
        assert_eq!(settings.sight_for(PISTOL), IRON_SIGHTS);
        assert_eq!(settings.sight_for(SNIPER), SCOPE, "the sniper's default");
        for sight in [IRON_SIGHTS, REFLEX, TWO_X, THREE_X, SCOPE] {
            settings.set_sight(0, sight);
            assert_eq!(settings.sight_for(SNIPER), sight);
        }
        settings.set_sight(1, REFLEX);
        assert_eq!(settings.sight_for(PISTOL), REFLEX);
        for sight in [SCOPE, THREE_X] {
            settings.set_sight(1, sight);
            assert_eq!(settings.sight_for(PISTOL), REFLEX, "the pistol takes no {}", crate::weapons::sight(sight).name);
        }
        // The DMR takes both; the rifle the 3x but not the scope.
        settings.set_gun(0, DMR);
        for sight in [THREE_X, SCOPE] {
            settings.set_sight(0, sight);
            assert_eq!(settings.sight_for(DMR), sight);
        }
        settings.set_gun(0, RIFLE);
        settings.set_sight(0, THREE_X);
        assert_eq!(settings.sight_for(RIFLE), THREE_X);
        settings.set_sight(0, SCOPE);
        assert_eq!(settings.sight_for(RIFLE), THREE_X, "the rifle takes no scope");
    }

    #[test]
    fn cycling_the_grenade_goes_through_every_kind() {
        let mut settings = Settings::default();
        let mut seen = Vec::new();
        for _ in 0..GRENADE_KINDS {
            settings.cycle_grenade();
            seen.push(settings.grenade);
        }
        seen.sort();
        assert_eq!(seen, (0..GRENADE_KINDS as GrenadeKind).collect::<Vec<_>>());
        assert_eq!(Settings::parse("grenade = flash\n").grenade, crate::protocol::FLASH);
        assert_eq!(Settings::parse("grenade = banana\n").grenade, FRAG);
    }

    #[test]
    fn a_damaged_file_falls_back_to_defaults() {
        let parsed = Settings::parse("sensitivity = lots\nfov = 9000\nbind.jump = Banana\nnonsense\n");
        assert_eq!(parsed.sensitivity, Settings::default().sensitivity);
        assert_eq!(parsed.fov, 100.0);
        assert_eq!(parsed.bind(Action::Jump, 0), Some(Bind::Key(KeyCode::Space)));
    }

    #[test]
    fn every_binding_has_a_name_that_reads_back() {
        let keys = BINDABLE_KEYS.iter().map(|key| Bind::Key(*key));
        let buttons = BINDABLE_BUTTONS.iter().map(|(button, _)| Bind::Mouse(*button));
        let wheel = [Bind::WheelUp, Bind::WheelDown];
        for bind in keys.chain(buttons).chain(wheel) {
            assert_eq!(Bind::parse(&bind.file_name()), Some(bind));
            assert!(!bind.display().is_empty());
        }
        assert_eq!(Bind::Key(KeyCode::ShiftLeft).display(), "LEFT SHIFT");
        assert_eq!(Bind::Key(KeyCode::KeyW).display(), "W");
        assert_eq!(Bind::Mouse(MouseButton::Right).display(), "MOUSE RIGHT");
        assert_eq!(Bind::WheelDown.display(), "WHEEL DOWN");
    }

    #[test]
    fn the_defaults_bind_every_action_to_something_different() {
        let settings = Settings::default();
        for (i, a) in Action::ALL.iter().enumerate() {
            for b in &Action::ALL[i + 1..] {
                assert_ne!(settings.bind(*a, 0), settings.bind(*b, 0), "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn a_saved_key_never_ends_up_bound_twice() {
        // Crouch saved on the key that is the grenade's default.
        let parsed = Settings::parse("bind.crouch = KeyG\n");
        assert_eq!(parsed.bind(Action::Crouch, 0), Some(Bind::Key(KeyCode::KeyG)));
        assert!(parsed.binds(Action::Grenade).all(|bind| bind != Bind::Key(KeyCode::KeyG)));
        // Files from before second bindings still read, and nothing is added.
        assert_eq!(parsed.bind(Action::Crouch, 1), None);
    }

    #[test]
    fn rebinding_to_a_used_key_swaps() {
        let mut settings = Settings::default();
        settings.set_bind(Action::Reload, 0, Bind::Key(KeyCode::KeyG));
        assert_eq!(settings.bind(Action::Reload, 0), Some(Bind::Key(KeyCode::KeyG)));
        assert_eq!(settings.bind(Action::Grenade, 0), Some(Bind::Key(KeyCode::KeyR)));
    }

    #[test]
    fn an_action_takes_two_keys() {
        let mut settings = Settings::default();
        settings.set_bind(Action::Jump, 1, Bind::Mouse(MouseButton::Back));
        assert_eq!(
            settings.binds(Action::Jump).collect::<Vec<_>>(),
            [Bind::Key(KeyCode::Space), Bind::Mouse(MouseButton::Back)]
        );
        assert_eq!(settings.key_names(Action::Jump), "SPACE / MOUSE BACK");
        // Putting the main key in the second slot swaps the two.
        settings.set_bind(Action::Jump, 1, Bind::Key(KeyCode::Space));
        assert_eq!(
            settings.binds(Action::Jump).collect::<Vec<_>>(),
            [Bind::Mouse(MouseButton::Back), Bind::Key(KeyCode::Space)]
        );
        // Clearing the main one moves the second one up.
        settings.set_bind(Action::Jump, 1, Bind::Key(KeyCode::KeyV));
        settings.clear_bind(Action::Jump, 0);
        assert_eq!(settings.bind(Action::Jump, 0), Some(Bind::Key(KeyCode::KeyV)));
        assert_eq!(settings.bind(Action::Jump, 1), None);
        settings.clear_bind(Action::Jump, 0);
        assert_eq!(settings.key_name(Action::Jump), UNBOUND);
        let parsed = Settings::parse(&settings.to_text());
        assert_eq!(parsed.binds(Action::Jump).count(), 0, "unbound is saved as unbound");
    }
}
