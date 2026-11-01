#!/usr/bin/env bash
# Switch this Mango session from Waybar to the island. Safe to re-run.
#
#   ./install.sh          build, install to ~/.local/bin, switch over
#   ./install.sh --undo   put Waybar back
#
# Touches exactly two files: mango/scripts/bar (the autostart loop) and a marked
# block at the end of mango/config.conf. Originals are kept in ISLAND_BACKUP.
# Waybar itself stays installed. Nothing here needs root.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

MANGO=${MANGO_CONFIG_DIR:-$HOME/.config/mango}
CONF=$MANGO/config.conf
BAR=$MANGO/scripts/bar
BIN_DIR=${ISLAND_BIN_DIR:-$HOME/.local/bin}
BACKUP=${ISLAND_BACKUP:-$HOME/.local/share/island-backup}
BEGIN='# >>> island'
END='# <<< island'

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

undo() {
    stop_bars
    strip_block
    if [ -f "$BACKUP/bar" ]; then
        cp "$BACKUP/bar" "$BAR"
    else
        printf '#!/bin/sh\n# Waybar 0.15 can segfault in its mpris module, so bring the bar back if it dies.\nuntil waybar; do sleep 1; done\n' > "$BAR"
    fi
    start_bar
    echo "Waybar restored."
}

if [ "${1:-}" = --undo ]; then
    undo
    exit 0
fi

if [ "${1:-}" != --no-build ]; then
    cargo build --release --bins
fi
install -Dm755 target/release/island "$BIN_DIR/island"

mkdir -p "$BACKUP"
# Only ever back up the pre-island originals, so a re-run can't overwrite them.
grep -q island "$BAR" || cp "$BAR" "$BACKUP/bar"
grep -q "^$BEGIN\$" "$CONF" || cp "$CONF" "$BACKUP/config.conf"

printf '#!/bin/sh\n# Bring the island back if it dies. A second copy exits 0, ending the loop.\nuntil %s/island; do sleep 1; done\n' "$BIN_DIR" > "$BAR"

# Mango reloads on save, so the rule is live before the island maps. Without
# no_animation it would slide in and lag behind every drag.
strip_block
cat >> "$CONF" <<BLOCK
$BEGIN
layer_rule=no_animation:1,no_shadow:1,layer_name:island
bind=SUPER,i,spawn,$BIN_DIR/island pin
bind=SUPER+SHIFT,i,spawn,$BIN_DIR/island tuck
bind=SUPER+ALT,i,spawn,$BIN_DIR/island reset
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
echo "Island is running, Waybar is stopped. Undo with: ./install.sh --undo"
