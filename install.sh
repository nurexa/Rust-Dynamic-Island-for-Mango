#!/usr/bin/env bash
# Switch this Mango session to the island. Safe to re-run.
#
#   ./install.sh             everything: bar, launcher, notifications, clipboard, screenshots
#   ./install.sh --manual    the same, but asks about each part, each key and each extra first
#   ./install.sh --style=frame   start in a style: pill, frame, notch, bar, glass, split or minimal
#   ./install.sh --light     just the basics; island settings turns the rest back on
#   ./install.sh --blur      see-through, with Mango blurring behind it (turns on blur_layer)
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
ICFG=${XDG_CONFIG_HOME:-$HOME/.config}/island/config.toml
# From a source checkout the island is built and copied to $BIN_DIR; from a package
# (this script in /usr/share/island) it's already installed and used where it is.
if [ -f Cargo.toml ]; then source_tree=1 EXE=$BIN_DIR/island
else source_tree=0 EXE=$(command -v island || echo /usr/bin/island); fi
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

usage() { sed -n '2,16s/^# \{0,1\}//p' "$0"; exit 0; }
mode=install manual=0 light=0 blur=0 build=${ISLAND_BUILD:-1} look=''
STYLES='pill frame notch bar glass split minimal'
# What --light leaves out: everything src/prefs.rs doesn't mark as a basic.
LIGHT_OFF='title brightness stats media visualizer sidebar calendar controls nightlight bluetooth timer claude privacy wallpaper windows files clipboard screenshot wifi calc web'
is_style() { [[ " $STYLES " == *" $1 "* ]]; }
for a in "$@"; do
    case $a in
        --undo) mode=undo ;;
        --bar-only) mode=bar ;;
        --manual|-i) manual=1 ;;
        --light) light=1 ;;
        --blur) blur=1 ;;
        --style=*)
            look=${a#--style=}
            is_style "$look" || { echo "No style '$look'. There's $STYLES." >&2; exit 2; } ;;
        --no-build) build=0 ;;
        -h|--help) usage ;;
        *) echo "Unknown option $a (see --help)" >&2; exit 2 ;;
    esac
done

[ $light$manual != 11 ] || { echo "--light or --manual, not both: --manual asks about each part." >&2; exit 2; }
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

has_layer() { mmsg get all-layers | grep -qE "\"name\":\"$1(-blur)?\""; }
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
        "${SED[@]}" -E "/^[[:space:]]*\"[^\"]+\"[[:space:]]*=[[:space:]]*\"spawn(_shell)?,/{\\#/island #b;/$pattern/{h;s/^/$WAS/;p;g;s#=[[:space:]]*\"spawn(_shell)?,.*\$#= \"spawn,$EXE $command\"#}}" "$CONF"
    else
        "${SED[@]}" -E "/^bind[^=]*=/{\\#/island #b;/$pattern/{h;s/^/$WAS/;p;g;s#,spawn(_shell)?,.*\$#,spawn,$EXE $command#}}" "$CONF"
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
    if [ -f "$ICFG" ]; then
        "${SED[@]}" "/^$BEGIN\$/,/^$END\$/d" "$ICFG"
        "${SED[@]}" "s/^$OFF//" "$ICFG"
        [ -s "$ICFG" ] || rm -f "$ICFG"
    fi
    if [ -f "$BACKUP/claude" ]; then
        "$EXE" claude-setup --undo >/dev/null 2>&1 && info "took the island's hooks out of Claude Code"
        rm -f "$BACKUP/claude"
    fi
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
    "sidebar|open the sidebar|SUPER+ALT,s"
)
NOTIFIERS='dunst|mako|swaync'
# Extras: name in config.toml | what it is | how to tell this machine can (empty: any can).
EXTRAS=(
    "claude|Claude Code on the pill: live status, permission cards, done notes|command -v claude"
    "timer|a pomodoro timer|"
    "privacy|dots on the pill while an app uses the mic or camera|"
    "battery|the battery on the pill|grep -qs Battery /sys/class/power_supply/*/type"
    "nightlight|a night light that warms and dims the screen|"
    "bluetooth|Bluetooth in the panel and search|ls /sys/class/bluetooth/* >/dev/null 2>&1"
    "files|file search|command -v plocate || command -v fd"
    "calc|sums and unit conversion in the search|"
    "web|a web search row in the search|"
    "sidebar|a sidebar: player, calendar, quick controls, notifications|"
    "calendar|a calendar in the sidebar|"
    "visualizer|music visualizers: bars, wave and ring|command -v parec"
)
declare -A extra_on=()
declined=() night='' engine='' caldir=''
for e in "${EXTRAS[@]}"; do
    IFS='|' read -r n _ test <<< "$e"
    # ISLAND_EXTRAS="a b" says which are available, so the tests don't depend on this machine.
    if [ -n "${ISLAND_EXTRAS+set}" ]; then [[ " $ISLAND_EXTRAS " == *" $n "* ]] && extra_on[$n]=1 || extra_on[$n]=0
    elif [ -z "$test" ] || eval "$test" >/dev/null 2>&1; then extra_on[$n]=1; else extra_on[$n]=0; fi
done
declare -A take=() keys=()
extra=()   # new binds asked for in manual mode: "KEY|command"
for t in "${TOOLS[@]}"; do take[${t%%|*}]=$([ $mode = bar ] && echo 0 || echo 1); done
take[shots]=${take[apps]} take[notes]=${take[apps]}
for o in "${OWN[@]}"; do keys[${o%%|*}]=${o##*|}; done
if [ $light = 1 ]; then
    read -ra declined <<< "$LIGHT_OFF"
    for n in "${!extra_on[@]}"; do [[ " $LIGHT_OFF " != *" $n "* ]] || extra_on[$n]=0; done
    # No keys moved onto, or made for, what it leaves out.
    take[clipboard]=0 take[wallpaper]=0 take[shots]=0 keys[sidebar]=none
fi

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
    echo; info "Styles: pill floats and can be dragged; minimal is just the time and what needs you; split buds"
    info "a bubble off for music and timers; notch hangs from the top edge; bar is full width; frame borders"
    info "the screen with a rail down the left; glass is a see-through floating bar. Frame, bar and glass"
    info "take a little space from your windows."
    s=$(ask "Style (${STYLES// /, }), or 'keep' for the one it's in (pill at first)" "${look:-keep}")
    while [ "$s" != keep ] && ! is_style "$s"; do s=$(ask "One of ${STYLES// /, }, or 'keep'" keep); done
    if [ "$s" = keep ]; then look=''; else look=$s; fi
    echo; info "Extras. Each is on when this machine can run it; say n to leave one out:"
    for e in "${EXTRAS[@]}"; do
        IFS='|' read -r n what _ <<< "$e"
        if [ "${extra_on[$n]}" = 1 ]; then
            agree "Use $what?" y || { extra_on[$n]=0; declined+=("$n"); }
        else
            info "($what: not available on this machine)"
        fi
    done
    if [ "${extra_on[nightlight]}" = 1 ]; then
        night=$(ask "Turn the night light on by schedule? Like 20:00-07:00, or 'none' for a button only" none)
        while [ "$night" != none ] && ! [[ $night =~ ^[0-9]{1,2}:[0-9]{2}-[0-9]{1,2}:[0-9]{2}$ ]]; do
            night=$(ask "Write it as HH:MM-HH:MM, or 'none'" none)
        done
        [ "$night" = none ] && night=''
    fi
    # No sidebar, no key for it.
    [[ " ${declined[*]-} " != *" sidebar "* ]] || keys[sidebar]=none
    if [ "${extra_on[calendar]}" = 1 ]; then
        caldir=$(ask "A folder of .ics files for the calendar's events (vdirsyncer's, or exports), or 'none'" none)
        while [ "$caldir" != none ] && { [[ $caldir == *[\"\\]* ]] || [ ! -d "${caldir/#\~/$HOME}" ]; }; do
            caldir=$(ask "That isn't a folder. Another, or 'none'" none)
        done
        [ "$caldir" = none ] && caldir=''
    fi
    if [ "${extra_on[web]}" = 1 ]; then
        e=$(ask "Web search with duckduckgo, google, brave, kagi, startpage, or a URL with {} for the words" duckduckgo)
        case $e in
            duckduckgo) ;;
            google) engine='https://www.google.com/search?q={}' ;;
            brave) engine='https://search.brave.com/search?q={}' ;;
            kagi) engine='https://kagi.com/search?q={}' ;;
            startpage) engine='https://www.startpage.com/do/search?q={}' ;;
            *{}*) engine=$e ;;
            *) info "Didn't recognise that; keeping DuckDuckGo." ;;
        esac
    fi
    echo; step "About to:"
    info "replace your bar with the island, started with the session"
    for t in "${TOOLS[@]}"; do tool "$t"; [ "${take[${t%%|*}]}" = 1 ] && info "point your $label keys at the island"; done
    [ "${take[shots]}" = 1 ] && info "point your screenshot keys at the island"
    [ "${take[notes]}" = 1 ] && info "show notifications in the island and stop your notification daemon autostarting"
    for e in ${extra[@]+"${extra[@]}"}; do info "bind ${e%%|*} to island ${e#*|}"; done
    for o in "${OWN[@]}"; do c=${o%%|*}; [ "${keys[$c]}" = none ] || info "bind ${keys[$c]} to island $c"; done
    [ -z "$look" ] || info "switch to the $look style"
    [ ${#declined[@]} -eq 0 ] || info "leave out: ${declined[*]}"
    [ -z "$caldir" ] || info "show events from $caldir"
    [ -z "$night" ] || info "turn the night light on $night"
    [ -z "$engine" ] || info "search the web with $engine"
    [ "${extra_on[claude]}" = 0 ] || info "add the island's hooks to Claude Code (~/.claude/settings.json)"
    agree "Go ahead?" y || { echo "Nothing changed."; exit 0; }
fi

# ---------------------------------------------------------------------------- files
if [ $source_tree = 1 ]; then
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
else
    [ -x "$EXE" ] || fail "No island installed and no source here: install the package first."
    step "Using the installed island at $EXE"
fi

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
printf '#!/bin/sh\n# Bring the island back if it dies. A second copy exits 0, ending the loop.\nuntil %s; do sleep 1; done\n' "$EXE" > "$BAR"
chmod +x "$BAR"

# ---------------------------------------------------------------------------- config
step "Updating ${CONF/#$HOME/\~}"
strip_block
baseline=$(check)

blurring=$blur
grep -qsx "blur_layer=1" "$ISL" && blurring=1   # asked for before: a re-run keeps it
# Mango reloads on save, so the rule is live before the island maps. Without
# no_animation it would slide in and lag behind every drag.
{
    echo "# Written by the island's install.sh: re-running it rewrites this file, --undo removes it."
    echo "# Put island binds of your own in your main config instead."
    grep -qE "^[[:space:]]*exec(_once)?[[:space:]]*=.*scripts/bar" "$CONF" || echo "exec_once=$BAR"
    echo "layer_rule=no_animation:1,no_shadow:1,no_blur:1,layer_name:island"
    # Blurred surfaces have a name of their own. The last matching rule wins, so this follows.
    echo "layer_rule=no_animation:1,no_shadow:1,no_blur:0,layer_name:^island-blur$"
    if [ $blurring = 1 ]; then
        echo "blur=1"
        echo "blur_layer=1"
        # A screen-wide selection overlay would blur all that is being selected.
        echo "layer_rule=no_animation:1,no_shadow:1,no_blur:1,layer_name:^selection$"
    fi
    for o in "${OWN[@]}"; do c=${o%%|*}; [ "${keys[$c]}" = none ] || echo "bind=${keys[$c]},spawn,$EXE $c"; done
    for e in ${extra[@]+"${extra[@]}"}; do echo "bind=${e%%|*},spawn,$EXE ${e#*|}"; done
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

# The extras: config.toml only changes when you left something out or chose something.
lines=()
if [ ${#declined[@]} -gt 0 ]; then
    list=$(printf '"%s", ' "${declined[@]}"); lines+=("disable = [${list%, }]")
fi
[ -z "$engine" ] || lines+=("web_search = \"$engine\"")
if [ ${#lines[@]} -gt 0 ] || [ -n "$night" ] || [ -n "$caldir" ]; then
    step "Writing your choices to ${ICFG/#$HOME/\~}"
    mkdir -p "$(dirname "$ICFG")"; touch "$ICFG"
    "${SED[@]}" "/^$BEGIN\$/,/^$END\$/d" "$ICFG"
    # A key you'd set yourself is set aside, not lost: --undo puts it back.
    for key in disable web_search; do
        if [ ${#lines[@]} -gt 0 ] && printf '%s\n' "${lines[@]}" | grep -q "^$key ="; then
            "${SED[@]}" -E "s/^($key[[:space:]]*=.*)\$/$OFF\1/" "$ICFG"
        fi
    done
    if [ ${#lines[@]} -gt 0 ]; then
        # Top-level keys have to come before any [table], so these go first.
        { printf '%s\n' "$BEGIN" "${lines[@]}" "$END"; cat "$ICFG"; } > "$ICFG.new" && cat "$ICFG.new" > "$ICFG" && rm -f "$ICFG.new"
        for l in "${lines[@]}"; do info "$l"; done
    fi
    if [ -n "$night" ]; then
        if grep -q '^\[nightlight\]' "$ICFG"; then
            warn "Your config already has a [nightlight] section: set schedule = \"$night\" there yourself."
        else
            printf '%s\n[nightlight]\nschedule = "%s"\n%s\n' "$BEGIN" "$night" "$END" >> "$ICFG"
            info "night light on $night"
        fi
    fi
    if [ -n "$caldir" ]; then
        if grep -q '^\[calendar\]' "$ICFG"; then
            warn "Your config already has a [calendar] section: set dir = \"$caldir\" there yourself."
        else
            printf '%s\n[calendar]\ndir = "%s"\n%s\n' "$BEGIN" "$caldir" "$END" >> "$ICFG"
            info "calendar events from $caldir"
        fi
    fi
fi
if [ "${extra_on[claude]}" = 1 ] && [ $mode != bar ]; then
    step "Adding the island's hooks to Claude Code"
    if out=$("$EXE" claude-setup 2>&1); then
        touch "$BACKUP/claude"
        info "$out (in ${CLAUDE_CONFIG_DIR:-~/.claude}/settings.json; --undo takes them out)"
    else
        warn "Couldn't add Claude Code hooks: $out"
    fi
fi

step "Starting the island"
stop_bars
start_bar
for _ in $(seq 30); do has_layer island && break; sleep 0.1; done
if ! has_layer island; then
    undo >/dev/null
    fail "The island didn't come up, so everything was rolled back. Run $EXE by hand to see why."
fi
# The island remembers it, the same as when you run this yourself.
if [ -n "$look" ]; then
    if "$EXE" style "$look"; then info "style: $look"; else warn "Couldn't switch to the $look style: try island style $look."; fi
fi

if [ $blur = 1 ]; then
    sleep 0.5   # a style switch makes new surfaces
    if "$EXE" set blur on; then info "blur: on"; else warn "Couldn't turn blur on: try island set blur on."; fi
    [ $toml = 0 ] || ! grep -qE "^[[:space:]]*blur_layer[[:space:]]*=[[:space:]]*(0|false)" "$CONF" || warn "Your config.toml sets blur_layer off, which wins over island.conf: turn it on there for blur."
fi
search=$(sed -nE 's#^bind=([^,]*),([^,]*),spawn,.*/island open$#\1+\2#p' "$ISL" | head -1)
printf '\n%s✓ Island is running.%s Hover the pill for the panel%s. Change it with: island settings. Check this machine with: island doctor. Undo with: ./install.sh --undo\n' \
    "$G" "$N" "${search:+, $search opens search}"
if [ ${#NOTES[@]} -gt 0 ]; then
    printf '%s  Worth a look:%s\n' "$Y" "$N"
    for m in "${NOTES[@]}"; do info "- $m"; done
    notify normal "Island is installed" "${#NOTES[@]} thing(s) need a look: see the installer's output."
else
    notify normal "Island is installed" "Hover the pill for the panel${search:+, or press $search to search}."
fi
