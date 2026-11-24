//! ShitShooter launcher: installs the newest game build from the GitHub
//! releases, then starts it. Friends only ever download this once.
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

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
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
        // The release workflow uses this to write launcher-version.txt.
        println!("{VERSION}");
        return;
    }
    println!("ShitShooter launcher {VERSION}\n");
    if let Err(message) = run(&args) {
        println!("\n{message}");
        pause();
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent("ShitShooter-Launcher")
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .build()
        .into();

    if std::env::var_os(JUST_UPDATED_ENV).is_none()
        && let Err(e) = update_launcher(&agent, args)
    {
        println!("Could not update the launcher ({e}). Carrying on.\n");
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

    println!("Checking for updates...");
    match fetch_text(&agent, GAME_VERSION_FILE) {
        // No asset list means the assets never finished installing (or an old
        // launcher installed this build), so that counts as out of date.
        Ok(latest)
            if installed.as_deref() == Some(latest.as_str())
                && dir.join(ASSETS_MANIFEST).exists() =>
        {
            println!("Up to date ({latest}).");
        }
        Ok(latest) => {
            match &installed {
                Some(old) => println!("Updating {old} -> {latest}"),
                None => println!("Installing {latest}"),
            }
            if let Err(e) = install_game(&agent, &dir, &latest) {
                if !game.exists() {
                    return Err(format!("Could not download the game: {e}"));
                }
                println!("\nCould not download the update: {e}\nStarting the installed version.");
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        Err(e) if game.exists() => {
            println!("Could not check for updates ({e}). Starting the installed version.");
            std::thread::sleep(Duration::from_secs(2));
        }
        Err(e) => {
            return Err(format!(
                "Could not download the game: {e}\nCheck your internet connection and try again."
            ));
        }
    }

    launch_game(&game, &dir, args)
}

/// Where the game is installed: `%LOCALAPPDATA%\ShitShooter` on Windows,
/// `~/Library/Application Support/ShitShooter` on a Mac and
/// `$XDG_DATA_HOME/ShitShooter` (`~/.local/share/ShitShooter`) on Linux, or
/// next to the launcher if those variables are somehow missing.
fn install_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| home().map(|home| home.join(".local").join("share")))
    };
    base.map(|base| base.join("ShitShooter"))
        .or_else(|| Some(std::env::current_exe().ok()?.parent()?.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
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

/// Downloads `url` to `dest`, printing progress.
fn download(agent: &ureq::Agent, url: &str, dest: &Path) -> Result<()> {
    let mut response = agent.get(url).call().map_err(describe)?;
    let total: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let mut reader = response.body_mut().as_reader();
    let mut out = fs::File::create(dest).map_err(|e| format!("could not write {}: {e}", dest.display()))?;

    let mut buffer = vec![0; 256 * 1024];
    let mut done: u64 = 0;
    let mut last_print = Instant::now();
    loop {
        let n = reader.read(&mut buffer).map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n]).map_err(|e| format!("could not write {}: {e}", dest.display()))?;
        done += n as u64;
        if last_print.elapsed() > Duration::from_millis(100) {
            print_progress(done, total);
            last_print = Instant::now();
        }
    }
    print_progress(done, total);
    println!();
    out.flush().map_err(|e| format!("could not write {}: {e}", dest.display()))?;
    if total.is_some_and(|total| done != total) {
        return Err("the download was cut short".into());
    }
    Ok(())
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn print_progress(done: u64, total: Option<u64>) {
    match total {
        Some(total) if total > 0 => print!(
            "\r  {:.1} / {:.1} MB ({}%)   ",
            mb(done),
            mb(total),
            done * 100 / total
        ),
        _ => print!("\r  {:.1} MB   ", mb(done)),
    }
    let _ = std::io::stdout().flush();
}

/// Brings the assets up to date, then downloads the latest game next to the
/// installed one and swaps it in.
fn install_game(agent: &ureq::Agent, dir: &Path, version: &str) -> Result<()> {
    sync_assets(agent, dir)?;
    let partial = dir.join(format!("{GAME_EXE}.download"));
    let result = download(agent, &download_url(&game_download()), &partial)
        .and_then(|()| make_executable(&partial));
    if let Err(e) = result {
        let _ = fs::remove_file(&partial);
        return Err(e);
    }
    let game = dir.join(GAME_EXE);
    // Fails on Windows while the game is running, since it locks a running exe.
    while let Err(e) = fs::rename(&partial, &game) {
        println!("\nCould not replace the game ({e}).");
        println!("If ShitShooter is still running, close it, then press Enter to try again.");
        pause_quietly();
    }
    // If this fails the next start just downloads the build again.
    let _ = fs::write(dir.join(GAME_VERSION_FILE), version);
    println!("Done.");
    Ok(())
}

struct AssetEntry {
    hash: String,
    size: u64,
    /// Relative to the assets folder, with `/` separators.
    path: String,
}

/// Makes `assets\` match the latest build's asset list: downloads files that
/// are missing or changed, and deletes files an earlier build installed that
/// this one no longer lists. Files the list does not know about are left alone.
fn sync_assets(agent: &ureq::Agent, dir: &Path) -> Result<()> {
    let text = fetch_text(agent, ASSETS_MANIFEST)
        .map_err(|e| format!("could not get the asset list: {e}"))?;
    let wanted = parse_manifest(&text)?;
    let manifest_path = dir.join(ASSETS_MANIFEST);
    let installed = fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|old| parse_manifest(&old).ok())
        .unwrap_or_default();
    let known: HashMap<&str, &str> = installed
        .iter()
        .map(|entry| (entry.path.as_str(), entry.hash.as_str()))
        .collect();

    let missing: Vec<&AssetEntry> = wanted
        .iter()
        .filter(|entry| !is_installed(dir, entry, &known))
        .collect();
    if !missing.is_empty() {
        let total: u64 = missing.iter().map(|entry| entry.size).sum();
        println!("Downloading {} asset file(s), {:.1} MB", missing.len(), mb(total));
        for entry in missing {
            install_asset(agent, dir, entry)?;
        }
    }

    let kept: HashSet<&str> = wanted.iter().map(|entry| entry.path.as_str()).collect();
    for old in installed.iter().filter(|entry| !kept.contains(entry.path.as_str())) {
        let _ = fs::remove_file(asset_path(dir, &old.path));
    }
    fs::write(&manifest_path, text)
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
fn install_asset(agent: &ureq::Agent, dir: &Path, entry: &AssetEntry) -> Result<()> {
    println!("  {}", entry.path);
    let dest = asset_path(dir, &entry.path);
    let mut partial = dest.clone().into_os_string();
    partial.push(".download");
    let partial = PathBuf::from(partial);

    let result = (|| {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        download(agent, &asset_url(&entry.hash), &partial)?;
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
    println!("Starting ShitShooter...");
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
/// differs, then runs the new one in its place and exits with its exit code.
/// Returns `Ok` when no update is needed.
fn update_launcher(agent: &ureq::Agent, args: &[String]) -> Result<()> {
    // Offline, or a release without the file: stay quiet, the game check reports it.
    let Ok(latest) = fetch_text(agent, LAUNCHER_VERSION_FILE) else {
        return Ok(());
    };
    if latest == VERSION {
        return Ok(());
    }
    println!("Updating the launcher to {latest}...");
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = with_suffix(&me, ".new");
    let old = with_suffix(&me, ".old");
    let result = download(agent, &download_url(&launcher_download()), &new)
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
    println!();
    let status = Command::new(&me)
        .args(args)
        .env(JUST_UPDATED_ENV, "1")
        .status()
        .map_err(|e| e.to_string())?;
    std::process::exit(status.code().unwrap_or(1));
}

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

fn pause() {
    println!("\nPress Enter to close.");
    pause_quietly();
}

fn pause_quietly() {
    let _ = std::io::stdin().read_line(&mut String::new());
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
