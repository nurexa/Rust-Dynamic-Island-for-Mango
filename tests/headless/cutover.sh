#!/bin/bash
H=$(dirname "$(readlink -f "$0")")
# Rehearse install.sh on a copy of the mango config, against a real Waybar running
# in the test compositor. Everything it kills is matched on this compositor's socket.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sorted(l['name'] for l in json.load(sys.stdin)['layers'] if l['layer'] in ('top','bottom')))"; }
winy(){ mmsg get all-clients | python3 -c "import json,sys;print([c['y'] for c in json.load(sys.stdin)['clients'] if c['appid']=='kitty'][0])"; }
# A fixture, so this doesn't depend on what the real config looks like today.
C=$R/cfg; rm -rf $C; mkdir -p $C/scripts
cat > $C/config.conf <<'E'
tag_num=10
exec_once=dunst
exec_once=hypridle
bind=SUPER,m,spawn_shell,pidof wlogout || wlogout -b 5
bind=SUPER,r,spawn,rofi -show run
bind=SUPER,d,spawn,rofi -show drun
bind=SUPER+SHIFT,c,spawn_shell,cliphist list | rofi -dmenu | cliphist decode | wl-copy
bind=SUPER,Print,spawn,~/.config/mango/scripts/shot output
bind=SHIFT,Print,spawn,~/.config/mango/scripts/shot region
bind=SUPER,w,spawn,~/.config/mango/scripts/wall pick
bind=SUPER+SHIFT,w,spawn,~/.config/mango/scripts/wall next
bind=SUPER,q,spawn,kitty
E
cp $C/config.conf $R/config.original
# Waybar's own default config, so this doesn't depend on whoever runs it.
WB="waybar -c /etc/xdg/waybar/config.jsonc -s /etc/xdg/waybar/style.css"
printf '#!/bin/sh\n# Test copy of the real bar script.\nuntil %s; do sleep 1; done\n' "$WB" > $C/scripts/bar; chmod +x $C/scripts/bar
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH
nohup setsid $C/scripts/bar >/dev/null 2>&1 &
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do [ "$(layers)" = "['waybar']" ] && break; sleep .25; done
ok "before: waybar is up" "$(layers)" "['waybar']"
export MANGO_CONFIG_DIR=$C ISLAND_BIN_DIR=$R/bin ISLAND_BACKUP=$R/backup
$H/../../install.sh --no-build
sleep 1
ok "after install: island replaced waybar" "$(layers)" "['island']"
ok "island reserves no space: the window takes the bar's old strip" "$(winy)" 12
ok "autostart now runs the island" "$(grep -c '^until .*/island;' $C/scripts/bar)" 1
ok "config block added once" "$(grep -c '^source=.*/island.conf$' $C/config.conf) $(grep -c 'layer_name:island' $C/island.conf)" "1 1"
ok "binds keep their keys and run the island" "$(grep -c '^bind=SUPER,d,spawn,.*/island open apps$' $C/config.conf)" 1
ok "shell-pipeline binds are replaced too" "$(grep -c '^bind=SUPER+SHIFT,c,spawn,.*/island open clipboard$' $C/config.conf)" 1
ok "screenshots move over" "$(grep -c '^bind=SHIFT,Print,spawn,.*/island shot region$' $C/config.conf)" 1
ok "wallpaper picker moves over, the rest of wall stays" "$(grep -c '/island open wallpaper$' $C/config.conf) $(grep -c '^bind=SUPER+SHIFT,w,spawn,.*wall next$' $C/config.conf)" "1 1"
ok "unrelated binds are left alone" "$(grep -c '^bind=SUPER,q,spawn,kitty$' $C/config.conf)" 1
ok "dunst won't autostart" "$(grep -c '^exec_once=dunst' $C/config.conf)" 0
ok "dunst can't be activated over D-Bus" "$(grep -c 'mask dunst' $R/shim.log)" 1
ok "hypridle untouched" "$(grep -c '^exec_once=hypridle' $C/config.conf)" 1
$H/../../install.sh --no-build >/dev/null; sleep 1
ok "second run changes nothing" "$(grep -c 'island.conf' $C/config.conf) $(grep -c '^#island-was#' $C/config.conf) $(layers)" "1 7 ['island']"
$H/../../install.sh --undo
sleep 2.5
ok "undo brings waybar back" "$(layers)" "['waybar']"
ok "undo removes the config block" "$(grep -c island $C/config.conf) $(ls $C/island.conf 2>/dev/null | wc -l)" "0 0"
ok "undo restores the config byte for byte" "$(cmp $C/config.conf $R/config.original && echo same)" same
ok "undo lets dunst run again" "$(grep -c 'unmask dunst' $R/shim.log)" 1
ok "undo restores the bar script" "$(grep -c waybar $C/scripts/bar)" 1
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x waybar --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE

# A stock config: no bar script, nothing to back up, so the block has to start the island.
pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3
rm -rf $R/backup $C/scripts; printf 'tag_num=9\n' > $C/config.conf
$H/../../install.sh --no-build
sleep 1
ok "stock config: island comes up" "$(layers)" "['island']"
ok "stock config: autostart added" "$(grep -c '^exec_once=.*scripts/bar' $C/island.conf)" 1
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3

# Bar only: nothing else changes hands
rm -rf $R/backup $C/scripts; cp $R/config.original $C/config.conf; : > $R/shim.log
$H/../../install.sh --no-build > /dev/null
$H/../../install.sh --undo > /dev/null; sleep .3
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x waybar --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3
: > $R/shim.log; ISLAND_BUILD=0 $H/../../install.sh --bar-only > /dev/null
ok "--bar-only leaves rofi, dunst and binds alone" "$(grep -c 'rofi -show drun$' $C/config.conf) $(grep -c '^exec_once=dunst' $C/config.conf) $(grep -c mask $R/shim.log)" "1 1 0"
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE

kill_all(){ pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x waybar --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3; }
fresh(){ kill_all; rm -rf $R/backup $C/scripts $C/island.conf $C/config.conf $C/config.toml $C/real; : > $R/shim.log; }

# Someone else's config: SUPER+space and SUPER+i already do something, Waybar starts
# straight from exec_once, and the config already has a warning of its own.
fresh
cat > $C/config.conf <<E
tag_num=9
exec_once=$WB
bind=SUPER,space,togglefloating
bind=SUPER,i,spawn,kitty
bind=SUPER,o,spawn,foot
bind=SUPER,o,spawn,kitty
bind=SUPER,d,spawn,fuzzel
bind=SUPER+SHIFT,v,spawn_shell,cliphist list | fuzzel --dmenu | cliphist decode | wl-copy
E
cp $C/config.conf $R/conflict.original
nohup setsid $WB >/dev/null 2>&1 &
sleep 1.5
ISLAND_BUILD=0 $H/../../install.sh > $R/install.out 2>&1
sleep .5
ok "taken keys: install still goes through" "$(layers)" "['island']"
ok "taken keys: the island's binds for them are left out" "$(grep -c 'island open$' $C/island.conf) $(grep -c 'island pin$' $C/island.conf) $(grep -c 'island tuck$' $C/island.conf)" "0 0 1"
ok "taken keys: the user is told which line has them" "$(grep -c -- '- .*is already yours' $R/install.out)" 2
ok "taken keys: the user's own binds are untouched" "$(grep -c '^bind=SUPER,space,togglefloating$' $C/config.conf)" 1
ok "a waybar exec_once won't start it again" "$(grep -c '^exec_once=waybar' $C/config.conf)" 0
ok "a clipboard pipe through fuzzel is the clipboard, not the launcher" "$(grep -c '^bind=SUPER+SHIFT,v,spawn,.*/island open clipboard$' $C/config.conf) $(grep -c '^bind=SUPER,d,spawn,.*/island open apps$' $C/config.conf)" "1 1"
ISLAND_BUILD=0 $H/../../install.sh --undo > /dev/null; sleep 2
ok "undo after taken keys: config byte for byte" "$(cmp $C/config.conf $R/conflict.original && echo same)" same
ok "undo starts the exec_once waybar again" "$(layers)" "['waybar']"

# TOML, symlinked like a dotfile manager would.
fresh
mkdir -p $C/real
cat > $C/real/config.toml <<E
[global]
tag_num = 9
exec_once = ["$WB", "dunst"]

[bind.default]
"SUPER+d" = "spawn,rofi -show drun"
"SUPER+space" = "togglefloating"
"SUPER+q" = "spawn,kitty"
E
ln -s real/config.toml $C/config.toml
cp $C/real/config.toml $R/toml.original
nohup setsid $WB >/dev/null 2>&1 &
sleep 1.5
ISLAND_BUILD=0 $H/../../install.sh > $R/install.out 2>&1
sleep .5
ok "toml: island comes up" "$(layers)" "['island']"
ok "toml: island.conf is sourced from [global]" "$(sed -n 2,3p $C/config.toml | tr -d '\n')" "# >>> islandsource_optional = [\"$C/island.conf\"]"
ok "toml: binds swapped in place" "$(grep -c '^"SUPER+d" = "spawn,.*/island open apps"$' $C/config.toml)" 1
ok "toml: waybar and dunst leave the exec_once array" "$(grep '^exec_once' $C/config.toml)" "exec_once = []"
ok "toml: the config is still a symlink" "$([ -L $C/config.toml ] && echo link)" link
ok "toml: mango accepts it" "$(env -u DBUS_SESSION_BUS_ADDRESS mango -c $C/config.toml -p >/dev/null 2>&1 && echo fine)" fine
ISLAND_BUILD=0 $H/../../install.sh --undo > /dev/null; sleep 2
ok "toml undo: byte for byte, still a link" "$(cmp $C/real/config.toml $R/toml.original && [ -L $C/config.toml ] && echo same)" same
ok "toml undo: waybar back" "$(layers)" "['waybar']"

# --manual, answered from stdin: keep the launcher swap, add a run key, move the
# search key off the taken SUPER+space, and drop the tuck and reset keys.
fresh
printf 'tag_num=9\nbind=SUPER,d,spawn,rofi -show drun\nbind=SUPER,space,togglefloating\n' > $C/config.conf
printf 'none\nnone\nnone\ny\nSUPER,r\n\nnone\nnone\n\ny\nSUPER+ALT,space\n' | ISLAND_BUILD=0 $H/../../install.sh --manual > $R/install.out 2>&1
sleep .5
ok "manual: island comes up" "$(layers)" "['island']"
ok "manual: answers land in island.conf" "$(grep -cE 'bind=SUPER,r,spawn,.*island open run$|bind=SUPER\+ALT,space,spawn,.*island open$|bind=SUPER,i,spawn,.*island pin$' $C/island.conf) $(grep -cE 'island (tuck|reset)$' $C/island.conf)" "3 0"
ok "manual: launcher swapped" "$(grep -c '^bind=SUPER,d,spawn,.*/island open apps$' $C/config.conf)" 1
ok "manual: the final message names the new search key" "$(grep -c 'SUPER+ALT+space opens search' $R/install.out)" 1
ISLAND_BUILD=0 $H/../../install.sh --undo > /dev/null
kill_all
