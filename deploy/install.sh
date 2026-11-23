#!/bin/sh
# Installs the ShitShooter lobby server on a Raspberry Pi (or any Linux box
# with systemd) as an always-on service, and keeps it up to date with the
# game's releases. Run it on the Pi:
#
#   curl -fsSL https://raw.githubusercontent.com/badubies/ShitShooter/main/deploy/install.sh | sudo sh
#
# Other ports or more lobbies (lobbies use the ports right after PORT):
#
#   curl -fsSL .../install.sh | sudo PORT=7777 LOBBIES=8 sh
#
# Running it again is safe: it rewrites the service with the new settings and
# leaves everything else as it is.
set -eu

REPO=badubies/ShitShooter
DIR=/opt/shitshooter
PORT=${PORT:-7777}
LOBBIES=${LOBBIES:-8}

fail() {
    echo "error: $*" >&2
    exit 1
}

[ "$(id -u)" = 0 ] || fail "run this as root (... | sudo sh)"
command -v systemctl >/dev/null 2>&1 || fail "this needs systemd"
command -v curl >/dev/null 2>&1 || fail "this needs curl: sudo apt install curl"

case "$(uname -m)" in
    aarch64 | arm64) ARCH=arm64 ;;
    armv7l | armv8l) ARCH=armv7 ;;
    x86_64 | amd64) ARCH=x64 ;;
    *) fail "no server is built for this processor ($(uname -m)). A Pi 2 or newer is needed." ;;
esac

id shitshooter >/dev/null 2>&1 ||
    useradd --system --no-create-home --shell /usr/sbin/nologin shitshooter
mkdir -p "$DIR"
echo "$ARCH" >"$DIR/arch"
echo "$PORT" >"$DIR/port"

# The updater: installs the newest release, but never while people play. It
# runs every few minutes, so it gets its chance once they are done.
cat >"$DIR/update.sh" <<EOF
#!/bin/sh
# Written by install.sh: changes here are lost when it runs again.
set -eu
DIR=$DIR
REPO=$REPO
EOF
cat >>"$DIR/update.sh" <<'EOF'
ARCH=$(cat "$DIR/arch")
PORT=$(cat "$DIR/port")
latest=$(curl -fsSL "https://github.com/$REPO/releases/latest/download/version.txt")
installed=$(cat "$DIR/version.txt" 2>/dev/null || true)
[ -n "$latest" ] && [ "$latest" != "$installed" ] || exit 0
if [ -x "$DIR/shitshooter" ] && systemctl is-active --quiet shitshooter; then
    players=$("$DIR/shitshooter" --hub-players --port "$PORT" 2>/dev/null || echo 0)
    if [ "$players" -gt 0 ]; then
        echo "$latest is out: waiting for the $players people playing to finish"
        exit 0
    fi
fi
curl -fsSL -o "$DIR/shitshooter.new" \
    "https://github.com/$REPO/releases/download/$latest/shitshooter-server-linux-$ARCH"
chmod 755 "$DIR/shitshooter.new"
mv -f "$DIR/shitshooter.new" "$DIR/shitshooter"
echo "$latest" >"$DIR/version.txt"
systemctl restart shitshooter
echo "Updated to $latest"
EOF
chmod 755 "$DIR/update.sh"

cat >/etc/systemd/system/shitshooter.service <<EOF
[Unit]
Description=ShitShooter lobby server
Wants=network-online.target
After=network-online.target

[Service]
User=shitshooter
ExecStart=$DIR/shitshooter --hub --port $PORT --lobbies $LOBBIES
Restart=always
RestartSec=5
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
EOF

cat >/etc/systemd/system/shitshooter-update.service <<EOF
[Unit]
Description=Update the ShitShooter lobby server
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
ExecStart=$DIR/update.sh
EOF

cat >/etc/systemd/system/shitshooter-update.timer <<EOF
[Unit]
Description=Check for a new ShitShooter release every 5 minutes

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min

[Install]
WantedBy=timers.target
EOF

systemctl daemon-reload
echo "Downloading the server..."
"$DIR/update.sh" || fail "could not download the server from the latest release of $REPO"
systemctl enable --now shitshooter.service shitshooter-update.timer >/dev/null 2>&1
systemctl restart shitshooter.service

LAN_IP=$(hostname -I 2>/dev/null | cut -d' ' -f1)
LAST=$((PORT + LOBBIES))
cat <<EOF

The lobby server is running ($(cat "$DIR/version.txt")), and starts with the Pi.

Left to do:
  1. On your router, forward UDP ports $PORT-$LAST to this machine (${LAN_IP:-its LAN IP}),
     and give it a fixed LAN IP (a DHCP reservation) so the forward keeps working.
  2. In the game, put this server's address under ONLINE > Lobby server:
     your public IP, or a dynamic DNS name. On your own network ${LAN_IP:-its LAN IP} works.

It updates itself within 5 minutes of each release, once nobody is playing on it.
  Logs:       journalctl -u shitshooter -f
  Updates:    journalctl -u shitshooter-update
  Stop:       sudo systemctl disable --now shitshooter shitshooter-update.timer
EOF
