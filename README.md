# ShitShooter

A fast free-for-all arena shooter with self-hosted lobbies.

## Playing

Download the launcher for your computer from the [latest release](https://github.com/badubies/ShitShooter/releases/latest). It downloads the game, and on every start after that it checks for a new build and installs it before starting the game.

- **Windows:** download **ShitShooterLauncher.exe**, right-click it → **Properties** → tick **Unblock** → **OK**, then run it. (Windows blocks downloaded exes that aren't signed. Without this, double-clicking it does nothing.)
- **Mac (Apple Silicon):** download **ShitShooterLauncher-macos-arm64.zip**, unzip it and double-click **ShitShooterLauncher**; it runs in a Terminal window. The first time, macOS refuses to open it because it isn't notarized: click **Done**, go to System Settings → **Privacy & Security**, click **Open Anyway** next to ShitShooterLauncher, then double-click it again.
- **Linux (x86-64):** download **ShitShooterLauncher-linux-x64.tar.gz**, then in a terminal: `tar xzf ShitShooterLauncher-linux-x64.tar.gz && ./ShitShooterLauncher`. The game needs glibc 2.39 or newer (Ubuntu 24.04, Debian 13, Fedora 40, Mint 22 or newer).

The game is installed to `%LOCALAPPDATA%\ShitShooter` on Windows, `~/Library/Application Support/ShitShooter` on a Mac and `~/.local/share/ShitShooter` on Linux, so the launcher can live anywhere (desktop, downloads...). Without internet it starts the version already installed. Any arguments given to the launcher are passed on to the game, e.g. `ShitShooterLauncher.exe --join 1.2.3.4`.

## Running a lobby server (Raspberry Pi)

A lobby server is an always-on machine where players open and join lobbies without anyone having to host on their own PC. In the game it's **Play online** on the main menu: it lists the server's lobbies (any lobby with someone in it), and **Open a lobby** opens one of your own. The server's address goes in the box at the top of that page. Whoever opens a lobby leads it and picks the mode, map and bots as usual. A lobby closes 30 seconds after the last person leaves.

A Raspberry Pi 3, 4 or 5 is plenty. The server does no graphics, and each lobby is a light thread. On a home connection your **upload speed** runs out first: a full 16-player lobby sends about 2.5 Mbit/s.

1. On the Pi (Raspberry Pi OS, 64-bit or 32-bit), run:
   ```sh
   curl -fsSL https://raw.githubusercontent.com/badubies/ShitShooter/main/deploy/install.sh | sudo sh
   ```
   This installs the server to `/opt/shitshooter` as a service that starts on boot. Every 5 minutes it checks for a new release and installs it, but never while people are playing on it. Clients and the server must run the same game version: when they don't, the game says the server is updating.
2. On your router, give the Pi a fixed LAN IP, and forward **UDP ports 7777–7785** to it. The server listens on 7777, and the lobbies use the ports after it.
3. Players need an address that reaches the Pi: your public IP, or better a free dynamic DNS name (e.g. [DuckDNS](https://www.duckdns.org)), since home IPs change. If your ISP uses CGNAT, port forwarding won't work from outside. In that case run the server on a cheap Linux VPS instead (the same script works there).
4. To put the address in everyone's game, set the repository variable `SHITSHOOTER_LOBBY_SERVER` (Settings → Secrets and variables → Actions → Variables), e.g. `mypi.duckdns.org`. Every build after that has it filled in. Players can also type it in themselves.

Other ports or more lobbies: `curl ... | sudo PORT=7777 LOBBIES=8 sh` (at most 32 lobbies). Logs: `journalctl -u shitshooter -f`. To stop it for good: `sudo systemctl disable --now shitshooter shitshooter-update.timer`.

The server is the same program as the game, built without the game: `shitshooter --hub` runs a lobby server anywhere, Windows included, and `shitshooter --server` runs a single lobby (see `--help`).

## Releases

Every push to `main` is built by GitHub Actions ([.github/workflows/release.yml](.github/workflows/release.yml)) for Windows, macOS (Apple Silicon) and Linux (x86-64), and published as a release named `build-<number>`. The last 10 are kept. If any platform fails to build, nothing is published. Each platform has its own files in the release: `shitshooter.exe` and `ShitShooterLauncher.exe` for Windows, `shitshooter-macos-arm64` and `ShitShooterLauncher-macos-arm64` for the Mac, `shitshooter-linux-x64` and `ShitShooterLauncher-linux-x64` for Linux. The launchers download these. New players download the `.zip` and `.tar.gz`, which keep the launcher executable.

Models, textures and sounds go in [assets/](assets/), and the game loads them with paths relative to that folder (e.g. `models/weapons/M4A1.gltf`). Each build lists its asset files with their hashes in `assets.txt`. The files themselves are uploaded once to a release tagged `assets`, named by hash, so an asset that doesn't change is never uploaded or downloaded again. The launcher downloads only the files that are new or changed. Don't delete the `assets` release.

To ship a new launcher, bump `version` in [launcher/Cargo.toml](launcher/Cargo.toml). Launchers already installed see the new version in the next release and replace themselves.

## Building locally

- `cargo run`: run the game.
- `.\build-release.ps1`: build `dist\shitshooter.exe` with the C runtime linked in, plus a copy of `dist\assets`.
- `cargo build --release -p shitshooter-launcher`: build the launcher.
- `cargo build --release --no-default-features --target aarch64-unknown-linux-musl`: build only the servers, for a Pi on a 64-bit OS (`armv7-unknown-linux-musleabihf` for 32-bit, `x86_64-unknown-linux-musl` for a PC or VPS). Add the target first with `rustup target add`. The exe is static and linked by Rust's own linker ([.cargo/config.toml](.cargo/config.toml)), so this works from Windows too.
