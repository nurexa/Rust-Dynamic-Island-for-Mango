#!/usr/bin/env bash
# Switch this Mango session to the island. Safe to re-run.
#
#   ./install.sh             everything: bar, launcher, notifications, clipboard, screenshots
#   ./install.sh --bar-only  only replace Waybar; rofi, dunst and your binds stay as they are
#   ./install.sh --undo      put it all back
#
# Your key bindings stay on the same keys: only what they run changes, and each
# original line is kept as a comment so --undo restores it exactly. Everything else
# added to mango/config.conf sits in a marked block. Installed packages aren't touched.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

MANGO=${MANGO_CONFIG_DIR:-$HOME/.config/mango}
CONF=$MANGO/config.conf
BAR=$MANGO/scripts/bar
BIN_DIR=${ISLAND_BIN_DIR:-$HOME/.local/bin}
BACKUP=${ISLAND_BACKUP:-$HOME/.local/share/island-backup}
BEGIN='# >>> island'
END='# <<< island'
WAS='#island-was# '
OFF='#island-off# '

[ -n "${MANGO_INSTANCE_SIGNATURE:-}" ] || { echo "Run this from inside the Mango session." >&2; exit 1; }
# Matched on this compositor's socket, so only this session's processes are touched.
mine=(--env "MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE")

has_layer() {
    mmsg get all-layers | grep -q "\"name\":\"$1\""
}

stop_bars() {
    pkill -f "$BAR" "${mine[@]}" || true   # first, or the loop restarts what we stop
    pkill -x waybar "${mine[@]}" || true
    pkill -x island "${mine[@]}" || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        pgrep -x waybar "${mine[@]}" >/dev/null || pgrep -x island "${mine[@]}" >/dev/null || break
        sleep 0.1
    done
}

start_bar() {
    setsid -f "$BAR" >/dev/null 2>&1 </dev/null
}

strip_block() {
    sed -i "/^$BEGIN\$/,/^$END\$/d" "$CONF"
}

# Point an existing bind at the island: same key, new command, old line kept above it.
swap_bind() {
    local pattern=$1 command=$2
    sed -i -E "/^bind[^=]*=/{\\#/island #b;/$pattern/{h;s/^/$WAS/;p;g;s#,spawn(_shell)?,.*\$#,spawn,$BIN_DIR/island $command#}}" "$CONF"
}

take_over() {
    swap_bind 'rofi -show drun' 'open apps'
    swap_bind 'rofi -show run' 'open run'
    swap_bind 'cliphist list' 'open clipboard'
    swap_bind 'wlogout' 'open power'
    swap_bind 'wall pick' 'open wallpaper'
    local kind
    for kind in output window region annotate color; do
        swap_bind "shot $kind\$" "shot $kind"
    done
    # Notification daemons: stop them starting, and stop D-Bus starting them on demand.
    sed -i -E "s/^(exec_once=(dunst|mako|swaync)( .*)?)\$/$OFF\1/" "$CONF"
    systemctl --user mask dunst.service >/dev/null 2>&1 || true
    pkill -x dunst "${mine[@]}" || true
}

undo() {
    stop_bars
    strip_block
    sed -i -E "/^$WAS/{s/^$WAS//;n;d}" "$CONF"
    systemctl --user unmask dunst.service >/dev/null 2>&1 || true
    if grep -q "^$OFF" "$CONF"; then
        sed -i "s/^$OFF//" "$CONF"
        setsid -f dunst >/dev/null 2>&1 </dev/null || true
    fi
    if [ -f "$BACKUP/bar" ]; then
        cp "$BACKUP/bar" "$BAR"
    else
        printf '#!/bin/sh\n# Waybar 0.15 can segfault in its mpris module, so bring the bar back if it dies.\nuntil waybar; do sleep 1; done\n' > "$BAR"
    fi
    start_bar
    echo "Your old setup is back."
}

mode=${1:-}
if [ "$mode" = --undo ]; then
    undo
    exit 0
fi

if [ "$mode" != --no-build ] && [ "${ISLAND_BUILD:-1}" = 1 ]; then
    cargo build --release --bins
fi
# Replace rather than overwrite, so a running island can't block the update.
install -Dm755 target/release/island "$BIN_DIR/island.new"
mv -f "$BIN_DIR/island.new" "$BIN_DIR/island"

# The wallpaper engine the panel's wallpaper settings drive (see wall/wall).
install -Dm755 wall/wall "$BIN_DIR/wall.new"
mv -f "$BIN_DIR/wall.new" "$BIN_DIR/wall"

mkdir -p "$BACKUP"
# Only ever back up the pre-island originals, so a re-run can't overwrite them.
[ ! -f "$BAR" ] || grep -q island "$BAR" || cp "$BAR" "$BACKUP/bar"
grep -q "^$BEGIN\$" "$CONF" || cp "$CONF" "$BACKUP/config.conf"

mkdir -p "$(dirname "$BAR")"
printf '#!/bin/sh\n# Bring the island back if it dies. A second copy exits 0, ending the loop.\nuntil %s/island; do sleep 1; done\n' "$BIN_DIR" > "$BAR"
chmod +x "$BAR"

# Mango reloads on save, so the rule is live before the island maps. Without
# no_animation it would slide in and lag behind every drag.
strip_block
# Stock configs don't run scripts/bar, so the block starts the island itself.
autostart=""
grep -q '^exec_once=.*scripts/bar' "$CONF" || autostart="exec_once=$BAR"$'\n'
[ "$mode" = --bar-only ] || take_over
cat >> "$CONF" <<BLOCK
$BEGIN
${autostart}layer_rule=no_animation:1,no_shadow:1,layer_name:island
bind=SUPER,i,spawn,$BIN_DIR/island pin
bind=SUPER+SHIFT,i,spawn,$BIN_DIR/island tuck
bind=SUPER+ALT,i,spawn,$BIN_DIR/island reset
bind=SUPER,space,spawn,$BIN_DIR/island open
$END
BLOCK

if ! env -u DBUS_SESSION_BUS_ADDRESS mango -c "$CONF" -p >/dev/null 2>&1; then
    echo "Mango rejected the new config, rolling back." >&2
    undo
    exit 1
fi
sleep 0.5

stop_bars
start_bar
sleep 1.5
if ! has_layer island; then
    echo "The island didn't come up, rolling back. Run $BIN_DIR/island by hand to see why." >&2
    undo
    exit 1
fi
echo "Island is running. Undo with: ./install.sh --undo"
