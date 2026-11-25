//! ShitShooter: a fast free-for-all arena shooter with self-hosted lobbies.
//!
//! Run with no arguments for the menu, or see `--help`.

// Release builds are a Windows GUI app, so no console window opens next to the
// game. Debug builds (`cargo run`) keep the console for the log output.
// The modes that print text (`--server`, `--help`, `--selftest`) attach to a
// console themselves; see `attach_console`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Without the `client` feature only the servers are built, and much of what
// they share with the game (map details, weapon stats...) goes unused.
#![cfg_attr(not(feature = "client"), allow(dead_code))]

// Shared by the game and the servers.
mod canvas;
mod hamlet;
mod hub;
mod map;
mod nav;
mod props;
mod protocol;
mod server;
mod terrain;
mod valley;
mod weapons;

// The game itself.
#[cfg(feature = "client")]
mod audio;
#[cfg(feature = "client")]
mod browser;
#[cfg(feature = "client")]
mod combat;
#[cfg(feature = "client")]
mod conquest;
#[cfg(feature = "client")]
mod devtools;
#[cfg(feature = "client")]
mod grenade;
#[cfg(feature = "client")]
mod gun_model;
#[cfg(feature = "client")]
mod loadout;
#[cfg(feature = "client")]
mod menu;
#[cfg(feature = "client")]
mod minimap;
#[cfg(feature = "client")]
mod net;
#[cfg(feature = "client")]
mod paint;
#[cfg(feature = "client")]
mod paint_editor;
#[cfg(feature = "client")]
mod ping;
#[cfg(feature = "client")]
mod player;
#[cfg(feature = "client")]
mod remote;
#[cfg(feature = "client")]
mod scenery;
#[cfg(feature = "client")]
mod settings;
#[cfg(feature = "client")]
mod ui;
#[cfg(feature = "client")]
mod weapon_panel;
#[cfg(feature = "client")]
mod world;

use std::sync::atomic::AtomicBool;

#[cfg(feature = "client")]
use bevy::prelude::*;

use crate::server::{Server, ServerSettings};

/// The release build name (e.g. `build-42`) when built by the GitHub release
/// workflow, otherwise the Cargo version. Shown in the log and crash message.
pub const VERSION: &str = match option_env!("SHITSHOOTER_BUILD") {
    Some(build) => build,
    None => env!("CARGO_PKG_VERSION"),
};

#[cfg(feature = "client")]
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppState {
    #[default]
    Menu,
    /// Waiting for the server to accept us.
    Connecting,
    Lobby,
    Playing,
}

/// Command line options.
#[cfg_attr(feature = "client", derive(Resource))]
#[derive(Clone, Default)]
pub struct LaunchArgs {
    pub dedicated: bool,
    /// Run a lobby server (`hub.rs`).
    pub hub: bool,
    /// Most lobbies the lobby server has open at once.
    pub max_lobbies: Option<usize>,
    /// Ask the lobby server on this machine how many people it has, and print it.
    pub hub_players: bool,
    pub host: bool,
    pub join: Option<String>,
    pub name: Option<String>,
    pub server: ServerSettings,
    /// Hidden developer option: play a scripted match and save screenshots here.
    pub selftest: Option<String>,
}

#[cfg(feature = "client")]
const HELP: &str = "\
ShitShooter

USAGE:
    shitshooter [OPTIONS]

With no options the game opens on the main menu.

GAME:
    --host               Host a lobby right away (skips the menu)
    --join <ADDR[:PORT]> Join a lobby right away (skips the menu)
    --name <NAME>        Player name
";

#[cfg(not(feature = "client"))]
const HELP: &str = "\
ShitShooter server (this build has no game, only the servers)

USAGE:
    shitshooter --hub [OPTIONS]
    shitshooter --server [OPTIONS]
";

const SERVER_HELP: &str = "
SERVERS:
    --server             Run one lobby, with no window
    --hub                Run a lobby server: players see its lobbies in the
                         game and open their own there. Lobbies get the
                         UDP ports right after --port.
    --lobbies <N>        Most lobbies a --hub has open at once [default: 8]
    --hub-players        Print how many people are in the lobbies of the
                         --hub on this machine (for update scripts)
    --port <PORT>        UDP port to host on              [default: 7777]
    --password <TEXT>    Lobby password (--host, --join and --server)

LOBBY SETTINGS (for --host, --server, and each lobby a --hub opens):
    --mode <NAME>        Game mode: ffa, teams, hill or conquest [default: ffa]
    --frags <N>          Kills needed to win (doubled for teams) [default: 20]
    --minutes <N>        Match time limit                 [default: 10]
    --bots <N>           Start with N bots
    --map <NAME>         Map: arena, depot, valley or hamlet [default: arena]
                         (hamlet: ffa and teams only)
    --airaccel <N>       Air acceleration: 10, 20, 30 or 100 [default: 20]

    --help               Show this text
";

fn parse_args() -> Result<LaunchArgs, String> {
    let mut args = LaunchArgs::default();
    let mut iter = std::env::args().skip(1);
    while let Some(flag) = iter.next() {
        let mut value = |what: &str| {
            iter.next()
                .ok_or_else(|| format!("{flag} needs a value ({what})"))
        };
        fn number<T: std::str::FromStr>(flag: &str, text: String) -> Result<T, String> {
            text.parse()
                .map_err(|_| format!("'{text}' is not a valid value for {flag}"))
        }
        match flag.as_str() {
            "--server" => args.dedicated = true,
            "--hub" => args.hub = true,
            "--hub-players" => args.hub_players = true,
            "--lobbies" => {
                let count: usize = number(&flag, value("count")?)?;
                if !(1..=hub::MAX_LOBBIES).contains(&count) {
                    return Err(format!("--lobbies must be between 1 and {}", hub::MAX_LOBBIES));
                }
                args.max_lobbies = Some(count);
            }
            "--host" => args.host = true,
            "--join" => args.join = Some(value("address")?),
            "--name" => args.name = Some(value("name")?),
            "--password" => args.server.password = value("text")?,
            "--port" => args.server.port = number(&flag, value("port")?)?,
            "--frags" => args.server.frag_limit = number::<u32>(&flag, value("count")?)?.max(1),
            "--minutes" => args.server.match_minutes = number::<f32>(&flag, value("minutes")?)?.max(0.5),
            "--bots" => args.server.bots = number(&flag, value("count")?)?,
            "--mode" => {
                let name = value("name")?;
                args.server.mode = ["ffa", "teams", "hill", "conquest"]
                    .iter()
                    .position(|known| known.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("unknown mode '{name}' (try: ffa, teams, hill, conquest)"))? as u8;
            }
            "--map" => {
                let name = value("name")?;
                args.server.map = map::MAP_NAMES
                    .iter()
                    .position(|known| known.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| format!("unknown map '{name}' (try: arena, depot, valley, hamlet)"))? as u8;
            }
            "--airaccel" => {
                let accel: u16 = number(&flag, value("number")?)?;
                if !protocol::AIR_ACCEL_CHOICES.contains(&accel) {
                    return Err(format!("--airaccel must be one of {:?}", protocol::AIR_ACCEL_CHOICES));
                }
                args.server.air_accel = accel;
            }
            "--selftest" => args.selftest = Some(value("directory")?),
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown option '{other}'")),
        }
    }
    if !map::plays(args.server.map, args.server.mode) {
        return Err(format!(
            "{} is not played in {}",
            map::MAP_NAMES[args.server.map as usize],
            protocol::MODE_NAMES[args.server.mode as usize].to_lowercase()
        ));
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if attach_console(false) {
                if !message.is_empty() {
                    eprintln!("error: {message}\n");
                }
                print!("{HELP}{SERVER_HELP}");
            } else {
                // Started without a terminal (e.g. a shortcut with bad options).
                let error = if message.is_empty() { String::new() } else { format!("error: {message}\n\n") };
                show_error_box(&format!("{error}{HELP}{SERVER_HELP}"));
            }
            return;
        }
    };

    if args.hub_players {
        attach_console(false);
        let address = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, args.server.port));
        match hub::ask(address, &hub::HubRequest::List) {
            Ok(hub::HubReply::Lobbies(lobbies)) => {
                println!("{}", lobbies.iter().map(|lobby| lobby.players as u32).sum::<u32>());
            }
            Ok(other) => {
                eprintln!("error: unexpected answer {other:?}");
                std::process::exit(1);
            }
            Err(err) => {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.hub {
        attach_console(true);
        let max_lobbies = args.max_lobbies.unwrap_or(hub::DEFAULT_MAX_LOBBIES);
        match hub::Hub::bind(args.server.clone(), max_lobbies) {
            Ok(hub) => {
                println!(
                    "[hub] lobby server {VERSION} listening on UDP port {}, lobbies on ports {}-{} (Ctrl+C to stop)",
                    args.server.port,
                    args.server.port as usize + 1,
                    args.server.port as usize + max_lobbies,
                );
                hub.run();
            }
            Err(err) => eprintln!("error: could not bind UDP port {}: {err}", args.server.port),
        }
        return;
    }

    if args.dedicated {
        // The server has no game window, so it always needs a console: the
        // terminal it was started from, or a new window.
        attach_console(true);
        match Server::bind(args.server.clone()) {
            Ok(server) => {
                println!(
                    "[server] dedicated server listening on UDP port {} (Ctrl+C to stop)",
                    args.server.port
                );
                server.run(&AtomicBool::new(false));
            }
            Err(err) => eprintln!("error: could not bind UDP port {}: {err}", args.server.port),
        }
        return;
    }

    #[cfg(feature = "client")]
    run_game(args);
    #[cfg(not(feature = "client"))]
    {
        attach_console(false);
        eprint!("error: this build has no game, only the servers.\n\n{HELP}{SERVER_HELP}");
    }
}

/// Opens the game window and runs the game until it is closed.
#[cfg(feature = "client")]
fn run_game(args: LaunchArgs) {
    if args.selftest.is_some() {
        attach_console(false);
    }

    open_log_file();
    install_crash_reporter();

    // The self-test's window opens behind whatever has focus, to watch or
    // not: it draws off screen (see `devtools`), whatever covers it.
    let focused = args.selftest.is_none();
    let exit = App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "ShitShooter".into(),
                        focused,
                        // Wayland's app ID and X11's window class, which tie the
                        // window to the launcher's app menu entry and its icon.
                        // Windows would take it as the window class: not needed.
                        name: (!cfg!(windows)).then(|| APP_ID.into()),
                        ..default()
                    }),
                    ..default()
                })
                .set(bevy::log::LogPlugin {
                    custom_layer: log_file_layer,
                    ..default()
                }),
        )
        .insert_resource(args)
        .add_systems(Update, set_window_icon)
        .init_state::<AppState>()
        .add_plugins((
            (net::NetPlugin, browser::BrowserPlugin),
            (world::WorldPlugin, scenery::SceneryPlugin),
            player::PlayerPlugin,
            remote::RemotePlugin,
            combat::CombatPlugin,
            grenade::GrenadePlugin,
            (ping::PingPlugin, conquest::ConquestPlugin, minimap::MinimapPlugin),
            paint::PaintPlugin,
            paint_editor::PaintEditorPlugin,
            gun_model::GunModelPlugin,
            loadout::LoadoutPlugin,
            // Before the UI, which starts from the saved settings.
            settings::SettingsPlugin,
            (ui::UiPlugin, menu::MenuPlugin, weapon_panel::WeaponPanelPlugin),
            audio::AudioPlugin,
            devtools::DevToolsPlugin,
        ))
        .run();
    log_line(&format!("exited: {exit:?}"));
}

/// Gives the game window the icon that build.rs embedded in the exe, for its
/// title bar and taskbar button. Bevy has no setting for this, so it goes
/// through winit directly, once the window exists.
#[cfg(all(windows, feature = "client"))]
fn set_window_icon(
    mut done: Local<bool>,
    window: Option<Single<Entity, With<bevy::window::PrimaryWindow>>>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    use winit::platform::windows::IconExtWindows;
    if *done {
        return;
    }
    let Some(window) = window else { return };
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let Some(winit_window) = windows.get_window(*window) else { return };
        // Resource ID 1, as set in build.rs. Windows picks the size it needs.
        match winit::window::Icon::from_resource(1, None) {
            Ok(icon) => winit_window.set_window_icon(Some(icon)),
            Err(err) => warn!("could not load the window icon: {err}"),
        }
        *done = true;
    });
}

/// The same name as the launcher's app menu entry on Linux
/// (`io.github.badubies.ShitShooter.desktop`).
#[cfg(feature = "client")]
const APP_ID: &str = "io.github.badubies.ShitShooter";

/// The largest picture in icon.ico, which build.rs copies out.
#[cfg(all(not(windows), feature = "client"))]
const ICON_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon.png"));

/// Gives the game its icon off Windows: the window's on Linux with X11 (on
/// Wayland it comes from the launcher's app menu entry), and the Dock's on a
/// Mac, where windows have none.
#[cfg(all(not(windows), feature = "client"))]
fn set_window_icon(
    mut done: Local<bool>,
    window: Option<Single<Entity, With<bevy::window::PrimaryWindow>>>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    if *done {
        return;
    }
    let Some(window) = window else { return };
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let Some(winit_window) = windows.get_window(*window) else { return };
        *done = true;
        #[cfg(target_os = "macos")]
        set_dock_icon();
        if cfg!(target_os = "macos") {
            return;
        }
        match icon_rgba() {
            Ok((rgba, width, height)) => match winit::window::Icon::from_rgba(rgba, width, height) {
                Ok(icon) => winit_window.set_window_icon(Some(icon)),
                Err(err) => warn!("could not load the window icon: {err}"),
            },
            Err(err) => warn!("could not load the window icon: {err}"),
        }
    });
}

/// `ICON_PNG` as RGBA pixels, with its width and height.
#[cfg(all(not(windows), feature = "client"))]
fn icon_rgba() -> Result<(Vec<u8>, u32, u32), String> {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    let image = Image::from_buffer(
        ICON_PNG,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD,
    )
    .map_err(|err| err.to_string())?;
    let (width, height) = (image.width(), image.height());
    image
        .data
        .filter(|data| data.len() == (width * height * 4) as usize)
        .map(|data| (data, width, height))
        .ok_or_else(|| "not 8-bit RGBA".to_string())
}

/// Sets the Dock icon, which a game started as a plain executable rather than
/// a .app does not have.
#[cfg(all(target_os = "macos", feature = "client"))]
fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(main_thread) = MainThreadMarker::new() else {
        warn!("could not set the Dock icon: not on the main thread");
        return;
    };
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON_PNG)) else {
        warn!("could not set the Dock icon: bad picture");
        return;
    };
    // SAFETY: called on the main thread (checked above) with a valid image.
    unsafe { NSApplication::sharedApplication(main_thread).setApplicationIconImage(Some(&image)) };
}


#[cfg(feature = "client")]
/// `shitshooter.log`, written next to the exe on every run. When the game will
/// not start on someone's PC, this file says how far it got and what went wrong.
static LOG_FILE: std::sync::OnceLock<(std::path::PathBuf, std::sync::Arc<std::fs::File>)> =
    std::sync::OnceLock::new();

#[cfg(feature = "client")]
fn open_log_file() {
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("shitshooter.log")));
    // Fall back to the temp folder if the exe sits somewhere read-only.
    let candidates = beside_exe
        .into_iter()
        .chain([std::env::temp_dir().join("shitshooter.log")]);
    for path in candidates {
        if let Ok(file) = std::fs::File::create(&path) {
            let _ = LOG_FILE.set((path, std::sync::Arc::new(file)));
            break;
        }
    }
    log_line(&format!(
        "ShitShooter {} starting on {} {}",
        VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
}

#[cfg(feature = "client")]
fn log_line(text: &str) {
    use std::io::Write;
    if let Some((_, file)) = LOG_FILE.get() {
        let _ = writeln!(&**file, "{text}");
    }
}

#[cfg(feature = "client")]
/// Copies everything Bevy logs (GPU chosen, warnings, errors) into the log file.
fn log_file_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    use bevy::log::tracing_subscriber::{Layer, fmt};
    let (_, file) = LOG_FILE.get()?;
    Some(fmt::layer().with_ansi(false).with_writer(file.clone()).boxed())
}

#[cfg(feature = "client")]
/// When the game crashes, say so: without this a double-clicked exe just
/// vanishes. The reason is shown in a message box and written to the log file.
fn install_crash_reporter() {
    use std::sync::atomic::Ordering;

    static REPORTED: AtomicBool = AtomicBool::new(false);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        if REPORTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut message = format!("ShitShooter {} crashed.\n\n{info}\n", VERSION);
        log_line(&format!("CRASH: {info}"));
        if let Some((path, _)) = LOG_FILE.get() {
            message.push_str(&format!("\nDetails were saved to:\n{}", path.display()));
        }
        message.push_str("\n\nIf it mentions the GPU, adapter or surface, update your graphics drivers.");
        show_error_box(&message);
        std::process::exit(1);
    }));
}

#[cfg(windows)]
fn show_error_box(text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(window: *mut std::ffi::c_void, text: *const u16, caption: *const u16, flags: u32) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (text, caption) = (wide(text), wide("ShitShooter"));
    const MB_ICONERROR: u32 = 0x10;
    // SAFETY: both strings are valid, null-terminated UTF-16 that outlive the call.
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_ICONERROR);
    }
}

#[cfg(not(windows))]
fn show_error_box(_text: &str) {}

/// Makes `println!` reach a console in release builds, which have none of
/// their own. Uses the console of the terminal that started the game; failing
/// that, opens a new console window if `open_new` is set. Returns whether there
/// is somewhere for text to go (always true in debug builds and off Windows).
#[cfg(all(windows, not(debug_assertions)))]
fn attach_console(open_new: bool) -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn AllocConsole() -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 calls without pointers; they fail harmlessly if the
    // process already has a console.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) != 0 || (open_new && AllocConsole() != 0) }
}

#[cfg(not(all(windows, not(debug_assertions))))]
fn attach_console(_open_new: bool) -> bool {
    true
}
