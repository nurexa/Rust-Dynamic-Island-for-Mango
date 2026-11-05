#!/usr/bin/env bash
# Switch this Mango session to the island. Safe to re-run.
#
#   ./install.sh             everything: bar, launcher, notifications, clipboard, screenshots
#   ./install.sh --manual    the same, but asks about each part and each key first
#   ./install.sh --bar-only  only replace the bar; launchers, notifications and binds stay as they are
#   ./install.sh --undo      put it all back
#
# Works with config.conf and config.toml (MANGO_CONFIG=/path/to/file for anything else).
# The island's own lines live in mango/island.conf, which your config sources from a
# marked block. Binds you already have keep their keys: only what they run changes, and
# each original line stays as a comment so --undo restores it exactly. A key the island
# wants that you already use is left to you and reported. Packages aren't touched.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

MANGO=${MANGO_CONFIG_DIR:-$HOME/.config/mango}
BIN_DIR=${ISLAND_BIN_DIR:-$HOME/.local/bin}
BACKUP=${ISLAND_BACKUP:-$HOME/.local/share/island-backup}
BEGIN='# >>> island'
END='# <<< island'
WAS='#island-was# '
OFF='#island-off# '
SED=(sed -i --follow-symlinks)   # dotfile managers symlink the config; keep the link

if [ -t 1 ]; then B=$'\e[1m' Y=$'\e[33m' R=$'\e[31m' G=$'\e[32m' N=$'\e[0m'; else B='' Y='' R='' G='' N=''; fi
NOTES=()
step() { printf '%s==>%s %s\n' "$B" "$N" "$*"; }
info() { printf '    %s\n' "$*"; }
warn() { printf '%s  ! %s%s\n' "$Y" "$*" "$N"; NOTES+=("$*"); }
notify() { command -v notify-send >/dev/null && notify-send -a Island -u "$1" "$2" "$3" >/dev/null 2>&1 || true; }
fail() {
    printf '%s  ✗ %s%s\n' "$R" "$*" "$N" >&2
    notify critical "Island install rolled back" "$*"
    exit 1
}

usage() { sed -n '2,13s/^# \{0,1\}//p' "$0"; exit 0; }
mode=install manual=0 build=${ISLAND_BUILD:-1}
for a in "$@"; do
    case $a in
        --undo) mode=undo ;;
        --bar-only) mode=bar ;;
        --manual|-i) manual=1 ;;
        --no-build) build=0 ;;
        -h|--help) usage ;;
        *) echo "Unknown option $a (see --help)" >&2; exit 2 ;;
    esac
done

[ -n "${MANGO_INSTANCE_SIGNATURE:-}" ] || { echo "Run this from inside the Mango session." >&2; exit 1; }
# Matched on this compositor's socket, so only this session's processes are touched.
mine=(--env "MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE")

# Mango reads config.conf first, then config.toml.
if [ -n "${MANGO_CONFIG:-}" ]; then CONF=$MANGO_CONFIG; MANGO=$(dirname "$CONF")
elif [ -f "$MANGO/config.conf" ]; then CONF=$MANGO/config.conf
elif [ -f "$MANGO/config.toml" ]; then CONF=$MANGO/config.toml
else CONF=''
fi
BAR=$MANGO/scripts/bar
ISL=$MANGO/island.conf
toml=0
case $CONF in *.toml) toml=1 ;; *.conf|'') ;; *) [ "$(head -c1 "$CONF")" = '[' ] && toml=1 ;; esac

has_layer() { mmsg get all-layers | grep -q "\"name\":\"$1\""; }
check() { env -u DBUS_SESSION_BUS_ADDRESS mango -c "$CONF" -p 2>&1 | sed 's/\x1b\[[0-9;]*m//g' || true; }

stop_bars() {
    pkill -f "$BAR" "${mine[@]}" || true   # first, or the loop restarts what we stop
    pkill -x waybar "${mine[@]}" || true
    pkill -x island "${mine[@]}" || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        pgrep -x waybar "${mine[@]}" >/dev/null || pgrep -x island "${mine[@]}" >/dev/null || break
        sleep 0.1
    done
}

start_bar() { setsid -f "$BAR" >/dev/null 2>&1 </dev/null; }
strip_block() { "${SED[@]}" "/^$BEGIN\$/,/^$END\$/d" "$CONF"; }

# Spawn binds that aren't the island's yet, in either format.
binds() {
    if [ $toml = 1 ]; then grep -E '^[[:space:]]*"[^"]+"[[:space:]]*=[[:space:]]*"spawn(_shell)?,' "$CONF"
    else grep -E '^bind[^=]*=[^,]*,[^,]*,spawn(_shell)?,' "$CONF"; fi | grep -v '/island ' || true
}

# Point existing binds at the island: same key, new command, old line kept above it.
swap_bind() {
    local pattern=$1 command=$2 line
    while IFS= read -r line; do
        [ -n "$line" ] && info "${line#"${line%%[![:space:]]*}"}  ->  island $command"
    done < <(binds | grep -E -- "$pattern")
    if [ $toml = 1 ]; then
        "${SED[@]}" -E "/^[[:space:]]*\"[^\"]+\"[[:space:]]*=[[:space:]]*\"spawn(_shell)?,/{\\#/island #b;/$pattern/{h;s/^/$WAS/;p;g;s#=[[:space:]]*\"spawn(_shell)?,.*\$#= \"spawn,$BIN_DIR/island $command\"#}}" "$CONF"
    else
        "${SED[@]}" -E "/^bind[^=]*=/{\\#/island #b;/$pattern/{h;s/^/$WAS/;p;g;s#,spawn(_shell)?,.*\$#,spawn,$BIN_DIR/island $command#}}" "$CONF"
    fi
}

# Stop something starting with the session. What was stopped is noted so --undo can start it again.
silence() {
    local re=$1 cmd
    while IFS= read -r cmd; do
        [ -n "$cmd" ] || continue
        info "won't autostart: $cmd"
        grep -qxF -- "$cmd" "$BACKUP/relaunch" 2>/dev/null || echo "$cmd" >> "$BACKUP/relaunch"
    done < <(if [ $toml = 1 ]; then grep -E '^[[:space:]]*exec(_once)?[[:space:]]*=' "$CONF" | grep -oE "\"[^\" ]*($re)[^\"]*\"" | tr -d '"'
             else grep -E "^exec(_once)?=[^ ]*($re)" "$CONF" | sed -E 's/^exec(_once)?=//'; fi)
    if [ $toml = 1 ]; then
        "${SED[@]}" -E "/^[[:space:]]*exec(_once)?[[:space:]]*=.*\"[^\" ]*($re)/{h;s/^/$WAS/;p;g;s/\"[^\" ]*($re)[^\"]*\"[[:space:]]*,?[[:space:]]*//g;s/=[[:space:]]*(\\[[[:space:]]*\\])?\$/= []/}" "$CONF"
        # Arrays spread over several lines aren't rewritten: say so instead of guessing.
        if grep -qE "^[[:space:]]*\"[^\" ]*($re)[^\"]*\"[[:space:]]*,?[[:space:]]*\$" "$CONF"; then
            warn "Your config still lists something matching '$re' in a multi-line exec array. Remove it there, or it starts again at login."
        fi
    else
        "${SED[@]}" -E "s/^(exec(_once)?=[^ ]*($re)[^ ]*( .*)?)\$/$OFF\1/" "$CONF"
    fi
}

undo() {
    [ -n "$CONF" ] || { echo "No Mango config found, nothing to undo."; return; }
    step "Restoring your setup"
    stop_bars
    strip_block
    "${SED[@]}" -E "/^$WAS/{s/^$WAS//;n;d}" "$CONF"
    "${SED[@]}" "s/^$OFF//" "$CONF"
    rm -f "$ISL"
    systemctl --user unmask dunst.service >/dev/null 2>&1 || true
    if [ -f "$BACKUP/bar" ]; then
        cp "$BACKUP/bar" "$BAR"
        start_bar
    elif [ -f "$BAR" ] && grep -q '/island;' "$BAR"; then
        rm -f "$BAR"   # ours: nothing ran a bar script before
    fi
    if [ -f "$BACKUP/relaunch" ]; then
        while IFS= read -r cmd; do
            info "starting $cmd"
            setsid -f sh -c "$cmd" >/dev/null 2>&1 </dev/null || true
        done < "$BACKUP/relaunch"
        rm -f "$BACKUP/relaunch"
    fi
    # A config we copied from /etc goes again, unless you've edited it since.
    if [ -f "$BACKUP/created" ] && cmp -s "$CONF" "$(cat "$BACKUP/created")"; then
        rm -f "$CONF"
        info "removed the config copied from $(cat "$BACKUP/created")"
    fi
    rm -f "$BACKUP/created"
    mmsg dispatch reload_config >/dev/null 2>&1 || true
    echo "Your old setup is back."
}

if [ $mode = undo ]; then
    undo
    exit 0
fi

# ---------------------------------------------------------------------------- plan
# name | patterns in the bind | island command | what it is | key for a new bind (manual mode)
# Pickers go before launchers: a clipboard bind piping into fuzzel isn't the app launcher.
TOOLS=(
    "clipboard|cliphist list|clipman pick|open clipboard|clipboard history|SUPER+SHIFT,c"
    "power|wlogout|wleave|open power|power menu|SUPER,m"
    "wallpaper|wall pick|waypaper|open wallpaper|wallpaper picker|SUPER,w"
    "apps|rofi -show drun|wofi --show drun|fuzzel|tofi-drun|anyrun|walker|open apps|app launcher|SUPER,d"
    "run|rofi -show run|wofi --show run|tofi-run|open run|run prompt|SUPER,r"
)
OWN=(  # island command | what it does | default key
    "pin|pin the panel open|SUPER,i"
    "tuck|tuck the pill away|SUPER+SHIFT,i"
    "reset|send the pill home|SUPER+ALT,i"
    "open|open search|SUPER,space"
)
NOTIFIERS='dunst|mako|swaync'
declare -A take=() keys=()
extra=()   # new binds asked for in manual mode: "KEY|command"
for t in "${TOOLS[@]}"; do take[${t%%|*}]=$([ $mode = bar ] && echo 0 || echo 1); done
take[shots]=${take[apps]} take[notes]=${take[apps]}
for o in "${OWN[@]}"; do keys[${o%%|*}]=${o##*|}; done

# Splits a TOOLS row into: pattern, command, label, default key.
tool() {
    local IFS='|' f; read -ra f <<< "$1"
    local n=${#f[@]}
    pattern=$(IFS='|'; echo "${f[*]:1:n-4}") command=${f[n-3]} label=${f[n-2]} newkey=${f[n-1]}
}

ask() {  # question default -> answer
    local ans
    printf '    %s [%s] ' "$1" "$2" >&2
    IFS= read -r ans || ans=''
    echo "${ans:-$2}"
}
agree() { case $(ask "$1 (y/n)" "$2") in [yY]*) return 0 ;; *) return 1 ;; esac; }
valid_key() { [[ $1 =~ ^[A-Za-z0-9_+]+,[^,[:space:]]+$ ]]; }

if [ $manual = 1 ]; then
    step "Manual setup: press Enter to take the suggestion in brackets"
    info "Config: ${CONF:-none yet, so the default from /etc/mango will be copied}"
    if [ -n "$CONF" ]; then
        for t in "${TOOLS[@]}"; do
            tool "$t"; name=${t%%|*}
            found=$(binds | grep -E -- "$pattern" || true)
            if [ -n "$found" ]; then
                echo; info "Your $label:"; while IFS= read -r l; do info "  ${l#"${l%%[![:space:]]*}"}"; done <<< "$found"
                agree "Open the island's $label from these keys instead?" y && take[$name]=1 || take[$name]=0
            else
                take[$name]=0
                k=$(ask "No $label key found. Add one for the island's (like $newkey), or 'none'" none)
                while [ "$k" != none ] && ! valid_key "$k"; do k=$(ask "Write it as MODS,key, like $newkey" none); done
                [ "$k" = none ] || extra+=("$k|$command")
            fi
        done
        echo
        if binds | grep -qE 'shot (output|window|region|annotate|color)'; then
            agree "Move your screenshot keys to the island's capture tool?" y && take[shots]=1 || take[shots]=0
        else take[shots]=0; fi
        if grep -qE "($NOTIFIERS)" "$CONF" || pgrep -x "$NOTIFIERS" "${mine[@]}" >/dev/null; then
            agree "Show notifications in the island instead of your current daemon?" y && take[notes]=1 || take[notes]=0
        fi
    fi
    echo; info "The island's own keys, written as MODS,key ('none' for no key):"
    for o in "${OWN[@]}"; do
        IFS='|' read -r c what def <<< "$o"
        k=$(ask "$what" "$def")
        while [ "$k" != none ] && ! valid_key "$k"; do k=$(ask "Write it as MODS,key, like $def" "$def"); done
        keys[$c]=$k
    done
    echo; step "About to:"
    info "replace your bar with the island, started with the session"
    for t in "${TOOLS[@]}"; do tool "$t"; [ "${take[${t%%|*}]}" = 1 ] && info "point your $label keys at the island"; done
    [ "${take[shots]}" = 1 ] && info "point your screenshot keys at the island"
    [ "${take[notes]}" = 1 ] && info "show notifications in the island and stop your notification daemon autostarting"
    for e in ${extra[@]+"${extra[@]}"}; do info "bind ${e%%|*} to island ${e#*|}"; done
    for o in "${OWN[@]}"; do c=${o%%|*}; [ "${keys[$c]}" = none ] || info "bind ${keys[$c]} to island $c"; done
    agree "Go ahead?" y || { echo "Nothing changed."; exit 0; }
fi

# ---------------------------------------------------------------------------- files
if [ $build = 1 ]; then
    step "Building the island (cargo build --release)"
    cargo build --release --bins
fi
step "Installing to $BIN_DIR"
# Replace rather than overwrite, so a running island can't block the update.
install -Dm755 target/release/island "$BIN_DIR/island.new"
mv -f "$BIN_DIR/island.new" "$BIN_DIR/island"
# The wallpaper engine the panel's wallpaper settings drive (see wall/wall).
install -Dm755 wall/wall "$BIN_DIR/wall.new"
mv -f "$BIN_DIR/wall.new" "$BIN_DIR/wall"
info "island, wall"

mkdir -p "$BACKUP"
if [ -z "$CONF" ]; then
    src=$(ls /etc/mango/config.conf /etc/mango/config.toml 2>/dev/null | head -1) \
        || fail "No Mango config in $MANGO and none in /etc/mango. Set MANGO_CONFIG=/path/to/your/config."
    CONF=$MANGO/${src##*/}
    mkdir -p "$MANGO"
    cp "$src" "$CONF"
    echo "$src" > "$BACKUP/created"
    case $CONF in *.toml) toml=1 ;; esac
    step "You had no config of your own, so Mango's default (the one you were running) is now $CONF"
fi

step "Backing up to $BACKUP"
# Only ever back up the pre-island originals, so a re-run can't overwrite them.
[ ! -f "$BAR" ] || grep -q island "$BAR" || { cp "$BAR" "$BACKUP/bar"; info "scripts/bar"; }
grep -qE "^($BEGIN|$WAS)" "$CONF" || { cp "$CONF" "$BACKUP/${CONF##*/}"; info "${CONF##*/}"; }

mkdir -p "$(dirname "$BAR")"
printf '#!/bin/sh\n# Bring the island back if it dies. A second copy exits 0, ending the loop.\nuntil %s/island; do sleep 1; done\n' "$BIN_DIR" > "$BAR"
chmod +x "$BAR"

# ---------------------------------------------------------------------------- config
step "Updating ${CONF/#$HOME/\~}"
strip_block
baseline=$(check)

# Mango reloads on save, so the rule is live before the island maps. Without
# no_animation it would slide in and lag behind every drag.
{
    echo "# Written by the island's install.sh: re-running it rewrites this file, --undo removes it."
    echo "# Put island binds of your own in your main config instead."
    grep -qE "^[[:space:]]*exec(_once)?[[:space:]]*=.*scripts/bar" "$CONF" || echo "exec_once=$BAR"
    echo "layer_rule=no_animation:1,no_shadow:1,layer_name:island"
    for o in "${OWN[@]}"; do c=${o%%|*}; [ "${keys[$c]}" = none ] || echo "bind=${keys[$c]},spawn,$BIN_DIR/island $c"; done
    for e in ${extra[@]+"${extra[@]}"}; do echo "bind=${e%%|*},spawn,$BIN_DIR/island ${e#*|}"; done
} > "$ISL"

if [ $toml = 0 ]; then
    printf '%s\nsource=%s\n%s\n' "$BEGIN" "$ISL" "$END" >> "$CONF"
elif grep -qE '^[[:space:]]*source_optional[[:space:]]*=[[:space:]]*\[' "$CONF"; then
    "${SED[@]}" -E "0,/^[[:space:]]*source_optional[[:space:]]*=[[:space:]]*\\[/{//{h;s/^/$WAS/;p;g;s#\\[#[\"$ISL\", #}}" "$CONF"
elif grep -q '^\[global\]' "$CONF"; then
    "${SED[@]}" "0,/^\[global\]/{/^\[global\]/a $BEGIN\nsource_optional = [\"$ISL\"]\n$END
}" "$CONF"
else
    "${SED[@]}" "1i $BEGIN\n[global]\nsource_optional = [\"$ISL\"]\n$END" "$CONF"
fi
info "island.conf: the island's keys and window rule, sourced from a marked block"

# One pass, so a line holding both is backed up once and --undo restores it.
if [ "${take[notes]}" = 1 ]; then silence "waybar|$NOTIFIERS"; else silence waybar; fi
if [ "${take[notes]}" = 1 ]; then
    # And stop D-Bus starting one on demand.
    systemctl --user mask dunst.service >/dev/null 2>&1 || true
    for d in dunst mako swaync; do pkill -x $d "${mine[@]}" || true; done
fi
for t in "${TOOLS[@]}"; do
    tool "$t"
    [ "${take[${t%%|*}]}" = 1 ] && swap_bind "$pattern" "$command"
done
if [ "${take[shots]}" = 1 ]; then
    eol=$([ $toml = 1 ] && echo '"' || echo '$')
    for kind in output window region annotate color; do swap_bind "shot $kind$eol" "shot $kind"; done
fi

# Let Mango judge the result. Problems in island.conf are ours to fix: a key you
# already use is skipped (or, with --manual, swapped for one you pick), and a line
# this Mango doesn't know is left out. Anything else new means roll back.
step "Checking the config with mango -p"
for _ in $(seq 20); do
    out=$(check)
    hit=$( { awk '/Key binding conflict/ { if (g) print g; g = ""; next }
                 /File "/ { s = $0; sub(/.*File "/, "", s); f = s; sub(/".*/, "", f); sub(/.*line /, "", s); g = g (g ? "\t" : "") f ":" s }
                 END { if (g) print g }' <<< "$out"
             sed -nE 's/.*\[Index\] (.*:[0-9]+).*/\1/p' <<< "$out"; } | grep -F "$ISL:" | head -1 || true)
    [ -n "$hit" ] || break
    n=$(tr '\t' '\n' <<< "$hit" | grep -F "$ISL:" | head -1); n=${n##*:}
    line=$(sed -n "${n}p" "$ISL")
    other=$(tr '\t' '\n' <<< "$hit" | grep -vF "$ISL:" | head -1 || true)
    if [[ $line == bind=* ]]; then
        key=${line#bind=}; key=${key%%,spawn*}; what=${line##*/island }
        if [ -n "$other" ]; then
            f=${other%:*} l=${other##*:}
            why="$key is already yours: ${f/#$HOME/\~} line $l: $(sed -n "${l}p" "$f" 2>/dev/null | sed 's/^[[:space:]]*//')"
        else
            why="Mango won't take $key: $(grep -m1 -E 'ERROR|WARN' <<< "$out" | sed 's/^\[[A-Z]*\]: //')"
        fi
        if [ $manual = 1 ]; then
            echo "$why"
            k=$(ask "Another key for 'island $what', or 'none'" none)
            while [ "$k" != none ] && ! valid_key "$k"; do k=$(ask "Write it as MODS,key" none); done
            if [ "$k" != none ]; then "${SED[@]}" "${n}s#^bind=[^,]*,[^,]*,#bind=$k,#" "$ISL"; continue; fi
        fi
        warn "$why. Left 'island $what' without a key; bind it yourself, or run ./install.sh --manual."
    else
        warn "This Mango doesn't accept '$line' ($(grep -m1 ERROR <<< "$out" | sed 's/^\[ERROR\]: //')). Left it out; updating Mango brings it back."
    fi
    "${SED[@]}" "${n}d" "$ISL"
done
norm() { sed -E 's/(line |:)[0-9]+/\1N/g; /^[[:space:]]*$/d' | sort -u; }
new=$(comm -13 <(norm <<< "$baseline") <(norm <<< "$out"))
if [ -n "$new" ]; then
    printf '%s\n' "$new" | sed 's/^/    /' >&2
    undo >/dev/null
    fail "Mango rejected the new config (above), so everything was rolled back. Please report it with the lines above."
fi
[ -z "$baseline" ] || info "(your config already had warnings of its own; those were left alone)"
mmsg dispatch reload_config >/dev/null 2>&1 || true
sleep 0.5

step "Starting the island"
stop_bars
start_bar
for _ in $(seq 30); do has_layer island && break; sleep 0.1; done
if ! has_layer island; then
    undo >/dev/null
    fail "The island didn't come up, so everything was rolled back. Run $BIN_DIR/island by hand to see why."
fi

search=$(sed -nE 's#^bind=([^,]*),([^,]*),spawn,.*/island open$#\1+\2#p' "$ISL" | head -1)
printf '\n%s✓ Island is running.%s Hover the pill for the panel%s. Undo with: ./install.sh --undo\n' \
    "$G" "$N" "${search:+, $search opens search}"
if [ ${#NOTES[@]} -gt 0 ]; then
    printf '%s  Worth a look:%s\n' "$Y" "$N"
    for m in "${NOTES[@]}"; do info "- $m"; done
    notify normal "Island is installed" "${#NOTES[@]} thing(s) need a look: see the installer's output."
else
    notify normal "Island is installed" "Hover the pill for the panel${search:+, or press $search to search}."
fi
