//! ShitShooter launcher: installs the newest game build from the GitHub
//! releases, then starts it. Friends only ever download this once.
//!
//! It shows a window (window.rs) with the build it updates from and to and the
//! files it downloads, then closes it once the game starts. The work happens on
//! its own thread, which reports through `Progress` (progress.rs). Where no
//! window can open (no display, no OpenGL) it prints to a terminal instead.
//! On a Mac it ships as ShitShooter.app, so no Terminal window opens.
//!
//! The game lives in `%LOCALAPPDATA%\ShitShooter` (on a Mac in
//! `~/Library/Application Support/ShitShooter`, on Linux in
//! `~/.local/share/ShitShooter`), so the launcher itself can sit anywhere
//! (desktop, downloads). Without internet it starts whatever is installed. Any
//! arguments are passed on to the game (e.g. `--join <addr>`).
//!
//! Each platform has its own game and launcher files in the release (see
//! `game_download` and `launcher_download`). Windows keeps the original names,
//! so launchers from before the Mac and Linux builds still find them.
//!
//! Game assets (models, textures, sounds) go to `assets\` next to the game, where
//! Bevy looks for them. Each build lists its assets in `assets.txt`, and only the
//! files that changed are downloaded. The files themselves live in one long-lived
//! release (tag `assets`), named by their SHA-256 hash.

// No console window on Windows. Where the window cannot open, `terminal`
// makes one.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod progress;
mod window;

use progress::{Choice, Progress};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const REPO: &str = "badubies/ShitShooter";
/// The game's file name in the install folder.
const GAME_EXE: &str = if cfg!(windows) { "shitshooter.exe" } else { "shitshooter" };
/// Release asset holding the game build name (e.g. `build-42`).
const GAME_VERSION_FILE: &str = "version.txt";
/// Release asset holding the launcher version, for the launcher's self-update.
const LAUNCHER_VERSION_FILE: &str = "launcher-version.txt";
/// Release asset listing the build's asset files, one per line:
/// `<sha256> <size in bytes> <path under assets/>`. The installed build's copy
/// is kept in the install folder, so the next update knows what is there.
const ASSETS_MANIFEST: &str = "assets.txt";
/// Tag of the release that holds every asset file, named by its hash.
const ASSETS_TAG: &str = "assets";
/// Folder next to the game exe that Bevy loads assets from.
const ASSETS_DIR: &str = "assets";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Set on the freshly updated launcher so it never tries to update itself again
/// in the same run (guards against a loop if a release is ever inconsistent).
const JUST_UPDATED_ENV: &str = "SHITSHOOTER_LAUNCHER_UPDATED";

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("{VERSION}");
        return;
    }
    let started = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let opened = {
        let (started, failed, args) = (started.clone(), failed.clone(), args.clone());
        window::run(
            move |progress| {
                started.store(true, Ordering::SeqCst);
                if !work(&progress, &args) {
                    failed.store(true, Ordering::SeqCst);
                }
            },
            !test_install_dir_set(),
        )
    };
    match opened {
        Err(e) if !started.load(Ordering::SeqCst) => terminal(&args, &e.to_string()),
        _ if failed.load(Ordering::SeqCst) => {
            close_terminal_window();
            std::process::exit(1);
        }
        _ => close_terminal_window(),
    }
}

/// Does the work without a window, printing to a terminal (a new console on
/// Windows), for when the window could not be opened.
fn terminal(args: &[String], why: &str) {
    open_console();
    println!("ShitShooter launcher {VERSION}\n(no window: {why})\n");
    let ok = work(&Progress::terminal(), args);
    close_terminal_window();
    if !ok {
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn open_console() {
    unsafe extern "system" {
        fn AllocConsole() -> i32;
    }
    // Rust opens stdout and stdin lazily, so they go to the new console.
    unsafe { AllocConsole() };
}

#[cfg(not(windows))]
fn open_console() {}

/// Updates and starts the game, offering to try again when that fails. Returns
/// false when the player gave up. Closes the window either way.
fn work(progress: &Progress, args: &[String]) -> bool {
    loop {
        match run(progress, args) {
            Ok(()) => {
                progress.close();
                return true;
            }
            Err(message) => {
                progress.headline("");
                if progress.ask(&message, &[Choice::TryAgain, Choice::Close]) == Choice::Close {
                    progress.close();
                    return false;
                }
                progress.reset();
            }
        }
    }
}

fn run(progress: &Progress, args: &[String]) -> Result<()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent("ShitShooter-Launcher")
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .build()
        .into();

    progress.headline("Checking for updates…");
    if std::env::var_os(JUST_UPDATED_ENV).is_none() {
        match update_launcher(&agent, progress, args) {
            // The new launcher carries on from here.
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => progress.warn(&format!("Could not update the launcher ({e}). Carrying on.")),
        }
    }
    remove_old_launcher();

    let dir = install_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let game = dir.join(GAME_EXE);
    let installed = game
        .exists()
        .then(|| fs::read_to_string(dir.join(GAME_VERSION_FILE)).ok())
        .flatten()
        .map(|v| v.trim().to_string());

    progress.headline("Checking for updates…");
    match fetch_text(&agent, GAME_VERSION_FILE) {
        // No asset list means the assets never finished installing (or an old
        // launcher installed this build), so that counts as out of date.
        Ok(latest)
            if installed.as_deref() == Some(latest.as_str())
                && dir.join(ASSETS_MANIFEST).exists() =>
        {
            progress.headline(&format!("Up to date ({latest})."));
        }
        Ok(latest) => {
            progress.builds(installed.as_deref(), &latest);
            if let Err(e) = install_game(&agent, progress, &dir, &latest) {
                if !game.exists() {
                    return Err(format!("Could not download the game: {e}"));
                }
                progress.warn(&format!("Could not download the update: {e}\nStarting the installed version."));
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        Err(e) if game.exists() => {
            progress.warn(&format!("Could not check for updates ({e}). Starting the installed version."));
            std::thread::sleep(Duration::from_secs(2));
        }
        Err(e) => {
            return Err(format!(
                "Could not download the game: {e}\nCheck your internet connection and try again."
            ));
        }
    }

    if cfg!(target_os = "linux")
        && let Err(e) = install_menu_entry(&dir)
    {
        progress.warn(&format!("Could not add ShitShooter to the app menu ({e})."));
    }
    progress.headline("Starting ShitShooter…");
    launch_game(&game, &dir, args)
}

/// Set in debug builds to install somewhere else than the real install folder,
/// for trying the launcher out. The window then opens without taking focus.
const TEST_INSTALL_DIR_ENV: &str = "SHITSHOOTER_LAUNCHER_DIR";

fn test_install_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) { std::env::var_os(TEST_INSTALL_DIR_ENV).map(PathBuf::from) } else { None }
}

fn test_install_dir_set() -> bool {
    test_install_dir().is_some()
}

/// Where the game is installed: `%LOCALAPPDATA%\ShitShooter` on Windows,
/// `~/Library/Application Support/ShitShooter` on a Mac and
/// `$XDG_DATA_HOME/ShitShooter` (`~/.local/share/ShitShooter`) on Linux, or
/// next to the launcher if those variables are somehow missing.
fn install_dir() -> PathBuf {
    if let Some(dir) = test_install_dir() {
        return dir;
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library").join("Application Support"))
    } else {
        xdg_data_home()
    };
    base.map(|base| base.join("ShitShooter"))
        .or_else(|| Some(std::env::current_exe().ok()?.parent()?.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `$XDG_DATA_HOME`, by default `~/.local/share`: where Linux keeps programs'
/// data, and their app menu entries in `applications/`.
fn xdg_data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local").join("share")))
}

/// The game's app ID, which its window carries (`APP_ID` in the game's
/// src/main.rs), and the name of its app menu entry on Linux.
const APP_ID: &str = "io.github.badubies.ShitShooter";

/// The largest picture in the game's icon.ico, which build.rs copies out.
const ICON_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon.png"));

/// On Linux, adds ShitShooter to the app menu, starting this launcher. Wayland
/// also takes the game window's icon from here, from the entry named after the
/// window's app ID. Rewritten on every start, so it follows the launcher when
/// it is moved.
fn install_menu_entry(dir: &Path) -> std::io::Result<()> {
    let Some(data_home) = xdg_data_home() else { return Ok(()) };
    let icon = dir.join("icon.png");
    write_if_changed(&icon, ICON_PNG)?;
    let apps = data_home.join("applications");
    fs::create_dir_all(&apps)?;
    write_if_changed(
        &apps.join(format!("{APP_ID}.desktop")),
        menu_entry(&std::env::current_exe()?, &icon).as_bytes(),
    )
}

/// The `.desktop` file for `install_menu_entry`. The launcher opens its own
/// window, so it needs no terminal.
fn menu_entry(launcher: &Path, icon: &Path) -> String {
    // Values escape backslashes. The command line also quotes its argument,
    // escaping ", `, $ and \ inside the quotes, and doubles %.
    let value = |text: &str| text.replace('\\', "\\\\");
    let mut quoted = String::from("\"");
    for c in launcher.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' | '\\' => quoted.extend(['\\', c]),
            '%' => quoted.push_str("%%"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=ShitShooter\n\
         Comment=Fast free-for-all arena shooter\n\
         Exec={}\n\
         Icon={}\n\
         Terminal=false\n\
         Categories=Game;ActionGame;\n\
         StartupWMClass={APP_ID}\n",
        value(&quoted),
        value(&icon.to_string_lossy()),
    )
}

/// Writes `path` unless it already holds exactly `contents`.
fn write_if_changed(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if fs::read(path).is_ok_and(|old| old == contents) {
        return Ok(());
    }
    fs::write(path, contents)
}

/// This platform's name in release file names, e.g. `macos-arm64`.
fn platform() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

/// The game's file in the release, e.g. `shitshooter-macos-arm64`.
fn game_download() -> String {
    if cfg!(windows) { GAME_EXE.to_string() } else { format!("shitshooter-{}", platform()) }
}

/// The launcher's file in the release, e.g. `ShitShooterLauncher-linux-x64`.
fn launcher_download() -> String {
    if cfg!(windows) {
        "ShitShooterLauncher.exe".to_string()
    } else {
        format!("ShitShooterLauncher-{}", platform())
    }
}

/// `path` with `suffix` added to the end of its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Downloads are written without the executable bit; Windows needs none.
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("could not make {} executable: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn download_url(file: &str) -> String {
    format!("https://github.com/{REPO}/releases/latest/download/{file}")
}

fn asset_url(hash: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{ASSETS_TAG}/{hash}")
}

fn fetch_text(agent: &ureq::Agent, file: &str) -> Result<String> {
    let text = agent
        .get(download_url(file))
        .call()
        .and_then(|mut response| response.body_mut().read_to_string())
        .map_err(describe)?;
    Ok(text.trim().to_string())
}

fn content_length(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    response.headers().get("content-length")?.to_str().ok()?.parse().ok()
}

/// The size of the file at `url`, without downloading it.
fn download_size(agent: &ureq::Agent, url: &str) -> Option<u64> {
    content_length(&agent.head(url).call().ok()?)
}

/// Downloads `url` to `dest` as file `file` of the download plan in `progress`.
fn download(agent: &ureq::Agent, progress: &Progress, file: usize, url: &str, dest: &Path) -> Result<()> {
    progress.file_started(file);
    let mut response = agent.get(url).call().map_err(describe)?;
    let total = content_length(&response);
    progress.file_progress(file, 0, total);
    let mut reader = response.body_mut().as_reader();
    let mut out = fs::File::create(dest).map_err(|e| format!("could not write {}: {e}", dest.display()))?;

    let mut buffer = vec![0; 256 * 1024];
    let mut done: u64 = 0;
    let mut last_report = Instant::now();
    loop {
        let n = reader.read(&mut buffer).map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n]).map_err(|e| format!("could not write {}: {e}", dest.display()))?;
        done += n as u64;
        if last_report.elapsed() > Duration::from_millis(50) {
            progress.file_progress(file, done, total);
            last_report = Instant::now();
        }
    }
    progress.file_progress(file, done, total);
    out.flush().map_err(|e| format!("could not write {}: {e}", dest.display()))?;
    if total.is_some_and(|total| done != total) {
        return Err("the download was cut short".into());
    }
    progress.file_done(file);
    Ok(())
}

/// Brings the assets up to date, then downloads the latest game next to the
/// installed one and swaps it in. The window lists all of these files first.
fn install_game(agent: &ureq::Agent, progress: &Progress, dir: &Path, version: &str) -> Result<()> {
    progress.headline("Checking the installed files…");
    let assets = check_assets(agent, dir)?;
    let game_url = download_url(&game_download());
    let mut files: Vec<(String, Option<u64>)> =
        assets.missing.iter().map(|entry| (entry.path.clone(), Some(entry.size))).collect();
    files.push((format!("{GAME_EXE} (the game)"), download_size(agent, &game_url)));
    progress.plan_files(files);
    progress.headline(if dir.join(GAME_EXE).exists() { "Downloading the update…" } else { "Downloading the game…" });

    for (index, entry) in assets.missing.iter().enumerate() {
        install_asset(agent, progress, index, dir, entry)?;
    }
    finish_assets(dir, &assets)?;

    let partial = dir.join(format!("{GAME_EXE}.download"));
    let result = download(agent, progress, assets.missing.len(), &game_url, &partial)
        .and_then(|()| make_executable(&partial));
    if let Err(e) = result {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    let game = dir.join(GAME_EXE);
    // Fails on Windows while the game is running, since it locks a running exe.
    while let Err(e) = fs::rename(&partial, &game) {
        progress.ask(
            &format!("Could not replace the game ({e}).\nIf ShitShooter is still running, close it, then try again."),
            &[Choice::TryAgain],
        );
    }
    // If this fails the next start just downloads the build again.
    let _ = fs::write(dir.join(GAME_VERSION_FILE), version);
    progress.headline("Done.");
    Ok(())
}

struct AssetEntry {
    hash: String,
    size: u64,
    /// Relative to the assets folder, with `/` separators.
    path: String,
}

/// What it takes to make `assets\` match the latest build's asset list.
struct AssetUpdate {
    /// The latest build's asset list, as downloaded.
    list: String,
    /// Files that are missing or changed.
    missing: Vec<AssetEntry>,
    /// Files an earlier build installed that this one no longer lists. Files no
    /// list knows about are left alone.
    removed: Vec<String>,
}

fn check_assets(agent: &ureq::Agent, dir: &Path) -> Result<AssetUpdate> {
    let list = fetch_text(agent, ASSETS_MANIFEST)
        .map_err(|e| format!("could not get the asset list: {e}"))?;
    let wanted = parse_manifest(&list)?;
    let installed = fs::read_to_string(dir.join(ASSETS_MANIFEST))
        .ok()
        .and_then(|old| parse_manifest(&old).ok())
        .unwrap_or_default();
    let known: HashMap<&str, &str> = installed
        .iter()
        .map(|entry| (entry.path.as_str(), entry.hash.as_str()))
        .collect();
    let kept: HashSet<&str> = wanted.iter().map(|entry| entry.path.as_str()).collect();
    let removed = installed
        .iter()
        .filter(|entry| !kept.contains(entry.path.as_str()))
        .map(|entry| entry.path.clone())
        .collect();
    let missing = wanted.into_iter().filter(|entry| !is_installed(dir, entry, &known)).collect();
    Ok(AssetUpdate { list, missing, removed })
}

/// Once the missing files are in: deletes the removed ones and records the list.
fn finish_assets(dir: &Path, update: &AssetUpdate) -> Result<()> {
    for path in &update.removed {
        let _ = fs::remove_file(asset_path(dir, path));
    }
    let manifest_path = dir.join(ASSETS_MANIFEST);
    fs::write(&manifest_path, &update.list)
        .map_err(|e| format!("could not write {}: {e}", manifest_path.display()))
}

fn parse_manifest(text: &str) -> Result<Vec<AssetEntry>> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let bad = || format!("bad line in {ASSETS_MANIFEST}: {line}");
            let mut parts = line.splitn(3, ' ');
            let (Some(hash), Some(size), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(bad());
            };
            let size = size.parse().map_err(|_| bad())?;
            let hash_ok = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
            if !hash_ok || !is_safe_asset_path(path) {
                return Err(bad());
            }
            Ok(AssetEntry { hash: hash.to_ascii_lowercase(), size, path: path.to_string() })
        })
        .collect()
}

/// Only plain relative paths, so a bad list can never write outside `assets\`.
fn is_safe_asset_path(path: &str) -> bool {
    path.split('/').all(|part| {
        !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':'])
    })
}

fn asset_path(dir: &Path, path: &str) -> PathBuf {
    let mut full = dir.join(ASSETS_DIR);
    full.extend(path.split('/'));
    full
}

/// True when the file is already there with the right contents. Files the last
/// install recorded with the same hash only get a size check; anything else is
/// hashed, so a half-finished earlier install does not download everything again.
fn is_installed(dir: &Path, entry: &AssetEntry, known: &HashMap<&str, &str>) -> bool {
    let path = asset_path(dir, &entry.path);
    let Ok(meta) = fs::metadata(&path) else {
        return false;
    };
    if meta.len() != entry.size {
        return false;
    }
    known.get(entry.path.as_str()) == Some(&entry.hash.as_str())
        || file_sha256(&path).is_ok_and(|hash| hash == entry.hash)
}

/// Downloads one asset next to its final place, checks its hash, then moves it in.
fn install_asset(agent: &ureq::Agent, progress: &Progress, file: usize, dir: &Path, entry: &AssetEntry) -> Result<()> {
    let dest = asset_path(dir, &entry.path);
    let mut partial = dest.clone().into_os_string();
    partial.push(".download");
    let partial = PathBuf::from(partial);

    let result = (|| {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        download(agent, progress, file, &asset_url(&entry.hash), &partial)?;
        if file_sha256(&partial)? != entry.hash {
            return Err("the download was corrupted".to_string());
        }
        fs::rename(&partial, &dest).map_err(|e| format!("could not replace {}: {e}", dest.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map_err(|e| format!("{}: {e}", entry.path))
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn launch_game(game: &Path, dir: &Path, args: &[String]) -> Result<()> {
    let mut command = Command::new(game);
    command.args(args).current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Current game builds are GUI apps and get no console from this. Older
        // builds (still installed when offline) had one: they get their own
        // window, as if double-clicked, so this one can close.
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        // The launcher runs in a terminal, which closes once it exits. Its own
        // process group and no hold on the terminal keep the game from being
        // closed with it. The game writes its own log file.
        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start {}: {e}", game.display()))
}

/// Replaces this launcher with the one in the latest release if its version
/// differs, then starts the new one in its place. Returns true when it did, and
/// this launcher should close; false when no update is needed.
///
/// On a Mac the launcher is the exe inside ShitShooter.app, and the exe alone is
/// replaced.
fn update_launcher(agent: &ureq::Agent, progress: &Progress, args: &[String]) -> Result<bool> {
    // Offline, or a release without the file: stay quiet, the game check reports it.
    let Ok(latest) = fetch_text(agent, LAUNCHER_VERSION_FILE) else {
        return Ok(false);
    };
    if latest == VERSION {
        return Ok(false);
    }
    progress.headline(&format!("Updating the launcher to {latest}…"));
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = with_suffix(&me, ".new");
    let old = with_suffix(&me, ".old");
    let name = me.file_name().map_or_else(launcher_download, |name| name.to_string_lossy().into_owned());
    progress.plan_files(vec![(name, None)]);
    let result = download(agent, progress, 0, &download_url(&launcher_download()), &new)
        .and_then(|()| make_executable(&new));
    if let Err(e) = result {
        let _ = fs::remove_file(&new);
        return Err(e);
    }
    // A running exe cannot be overwritten on Windows, but it can be renamed out
    // of the way.
    let _ = fs::remove_file(&old);
    fs::rename(&me, &old).map_err(|e| e.to_string())?;
    if let Err(e) = fs::rename(&new, &me) {
        let _ = fs::rename(&old, &me);
        return Err(e.to_string());
    }
    if cfg!(target_os = "macos") {
        // Unlike Windows, a Mac can delete a running exe. Gone before the
        // signing, it is not counted as part of ShitShooter.app.
        let _ = fs::remove_file(&old);
        sign_app(&me);
    }
    let mut command = Command::new(&me);
    command.args(args).env(JUST_UPDATED_ENV, "1");
    if progress.in_window() {
        // The new launcher opens its own window as this one closes.
        command.spawn().map_err(|e| e.to_string())?;
        return Ok(true);
    }
    println!();
    let status = command.status().map_err(|e| e.to_string())?;
    std::process::exit(status.code().unwrap_or(1));
}

/// On a Mac, signs ShitShooter.app again (ad hoc, as the release does) after
/// its exe was replaced, so the app's signature matches what is inside it.
/// Nothing happens for a launcher that is not inside a .app.
fn sign_app(exe: &Path) {
    let Some(app) = exe.ancestors().nth(3).filter(|app| app.extension().is_some_and(|ext| ext == "app")) else {
        return;
    };
    let _ = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(app)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Finds this launcher's Terminal window: the one-tab window on our tty.
#[cfg(target_os = "macos")]
const FIND_TERMINAL_WINDOW: &str = r#"on run argv
    tell application "Terminal"
        repeat with w in windows
            if (count of tabs of w) is 1 and tty of tab 1 of w is item 1 of argv then return id of w
        end repeat
    end tell
    return ""
end run"#;

/// Closes Terminal window `item 1 of argv` once nothing runs in it any more,
/// giving up after 10 seconds.
#[cfg(target_os = "macos")]
const CLOSE_TERMINAL_WINDOW: &str = r#"on run argv
    set windowId to (item 1 of argv) as integer
    tell application "Terminal"
        repeat 100 times
            delay 0.1
            try
                if (count of processes of tab 1 of window id windowId) is 0 then
                    close window id windowId
                    return
                end if
            on error
                return
            end try
        end repeat
    end tell
end run"#;

/// On a Mac, closes the Terminal window the launcher was double-clicked into,
/// as a Windows console closes by itself. Terminal keeps it open by default,
/// showing "[Process completed]". The window closes only once nothing runs in
/// it: started from a shell someone typed into, it stays, since the shell does.
#[cfg(target_os = "macos")]
fn close_terminal_window() {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let in_terminal = std::env::var_os("TERM_PROGRAM").is_some_and(|program| program == "Apple_Terminal");
    if !in_terminal {
        return;
    }
    let output = |command: &mut Command| {
        let output = command.stderr(Stdio::null()).output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && !text.is_empty()).then_some(text)
    };
    let Some(tty) = output(Command::new("/usr/bin/tty").stdin(Stdio::inherit())) else { return };
    let Some(window) = output(Command::new("/usr/bin/osascript").args(["-e", FIND_TERMINAL_WINDOW, &tty]))
    else {
        return;
    };
    // Outlives the launcher, in its own process group with no hold on the
    // terminal, so it is not closed with it.
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", CLOSE_TERMINAL_WINDOW, &window])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn close_terminal_window() {}

/// Deletes the previous launcher left behind by a self-update. It may still be
/// running for a moment, in which case this is retried on the next start.
fn remove_old_launcher() {
    if let Ok(me) = std::env::current_exe() {
        let _ = fs::remove_file(with_suffix(&me, ".old"));
    }
}

fn describe(error: ureq::Error) -> String {
    match error {
        ureq::Error::StatusCode(404) => "no release found on GitHub yet".into(),
        ureq::Error::StatusCode(code) => format!("GitHub answered with error {code}"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const M4: &str = "5fe4098259c581a5aa5f7dbf6aa0d8c5a4be9cb384480b88e303e87c4cf97b30";

    /// These must match the file names release.yml uploads.
    #[test]
    fn names_release_files() {
        let (game, launcher) = (game_download(), launcher_download());
        if cfg!(windows) {
            assert_eq!((game.as_str(), launcher.as_str()), ("shitshooter.exe", "ShitShooterLauncher.exe"));
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(game, "shitshooter-macos-arm64");
            assert_eq!(launcher, "ShitShooterLauncher-macos-arm64");
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(game, "shitshooter-linux-x64");
            assert_eq!(launcher, "ShitShooterLauncher-linux-x64");
        }
        assert_eq!(with_suffix(Path::new("a/ShitShooterLauncher.exe"), ".old"), Path::new("a/ShitShooterLauncher.exe.old"));
    }

    #[test]
    fn writes_menu_entry() {
        let entry = menu_entry(Path::new("/home/a b/50% \"x\"/ShitShooterLauncher"), Path::new("/i/icon.png"));
        assert!(entry.contains("\nExec=\"/home/a b/50%% \\\\\"x\\\\\"/ShitShooterLauncher\"\n"), "{entry}");
        assert!(entry.contains("\nIcon=/i/icon.png\n"));
        assert!(entry.ends_with("StartupWMClass=io.github.badubies.ShitShooter\n"));
        assert!(ICON_PNG.starts_with(b"\x89PNG"));
    }

    #[test]
    fn parses_manifest_lines() {
        let text = format!("{M4} 729090 models/M4A1.gltf\n\n{M4} 1 sounds/a b.wav\n");
        let entries = parse_manifest(&text).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].size, 729090);
        assert_eq!(entries[1].path, "sounds/a b.wav");
        assert!(parse_manifest("").unwrap().is_empty());
    }

    #[test]
    fn rejects_bad_lines() {
        for line in [
            format!("{M4} 1 ../evil.exe"),
            format!("{M4} 1 models/../../evil.exe"),
            format!("{M4} 1 C:/evil.exe"),
            format!("{M4} 1 /evil.exe"),
            format!("{M4} 1 models\\x.gltf"),
            format!("{M4} big models/x.gltf"),
            "abc 1 models/x.gltf".to_string(),
            M4.to_string(),
        ] {
            assert!(parse_manifest(&line).is_err(), "accepted {line}");
        }
    }

    #[test]
    fn hashes_and_checks_installed_files() {
        let dir = std::env::temp_dir().join(format!("shitshooter-launcher-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = asset_path(&dir, "models/x.bin");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"abc").unwrap();
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(file_sha256(&file).unwrap(), abc);

        let entry = |hash: &str, size| AssetEntry { hash: hash.into(), size, path: "models/x.bin".into() };
        let none = HashMap::new();
        assert!(is_installed(&dir, &entry(abc, 3), &none));
        assert!(!is_installed(&dir, &entry(abc, 4), &none));
        assert!(!is_installed(&dir, &entry(M4, 3), &none));
        // Recorded by the last install: trusted on size alone.
        let known = HashMap::from([("models/x.bin", M4)]);
        assert!(is_installed(&dir, &entry(M4, 3), &known));
        assert!(!is_installed(&dir, &AssetEntry { path: "models/missing".into(), ..entry(abc, 3) }, &none));
        let _ = fs::remove_dir_all(&dir);
    }
}
