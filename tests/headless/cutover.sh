#!/bin/bash
H=$(dirname "$(readlink -f "$0")")
# Rehearse install.sh on a copy of the mango config, against a real Waybar running
# in the test compositor. Everything it kills is matched on this compositor's socket.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sorted(l['name'] for l in json.load(sys.stdin)['layers'] if l['layer']=='top'))"; }
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
printf '#!/bin/sh\n# Test copy of the real bar script.\nuntil waybar; do sleep 1; done\n' > $C/scripts/bar; chmod +x $C/scripts/bar
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
ok "config block added once" "$(grep -c 'layer_name:island' $C/config.conf)" 1
ok "binds keep their keys and run the island" "$(grep -c '^bind=SUPER,d,spawn,.*/island open apps$' $C/config.conf)" 1
ok "shell-pipeline binds are replaced too" "$(grep -c '^bind=SUPER+SHIFT,c,spawn,.*/island open clipboard$' $C/config.conf)" 1
ok "screenshots move over" "$(grep -c '^bind=SHIFT,Print,spawn,.*/island shot region$' $C/config.conf)" 1
ok "wallpaper picker moves over, the rest of wall stays" "$(grep -c '/island open wallpaper$' $C/config.conf) $(grep -c '^bind=SUPER+SHIFT,w,spawn,.*wall next$' $C/config.conf)" "1 1"
ok "unrelated binds are left alone" "$(grep -c '^bind=SUPER,q,spawn,kitty$' $C/config.conf)" 1
ok "dunst won't autostart" "$(grep -c '^exec_once=dunst' $C/config.conf)" 0
ok "dunst can't be activated over D-Bus" "$(grep -c 'mask dunst' $R/shim.log)" 1
ok "hypridle untouched" "$(grep -c '^exec_once=hypridle' $C/config.conf)" 1
$H/../../install.sh --no-build >/dev/null; sleep 1
ok "second run changes nothing" "$(grep -c 'layer_name:island' $C/config.conf) $(layers)" "1 ['island']"
$H/../../install.sh --undo
sleep 2.5
ok "undo brings waybar back" "$(layers)" "['waybar']"
ok "undo removes the config block" "$(grep -c 'layer_name:island' $C/config.conf)" 0
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
ok "stock config: autostart added" "$(grep -c '^exec_once=.*scripts/bar' $C/config.conf)" 1
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3

# Bar only: nothing else changes hands
rm -rf $R/backup $C/scripts; cp $R/config.original $C/config.conf; : > $R/shim.log
$H/../../install.sh --no-build > /dev/null
$H/../../install.sh --undo > /dev/null; sleep .3
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x waybar --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3
: > $R/shim.log; ISLAND_BUILD=0 $H/../../install.sh --bar-only > /dev/null
ok "--bar-only leaves rofi, dunst and binds alone" "$(grep -c 'rofi -show drun$' $C/config.conf) $(grep -c '^exec_once=dunst' $C/config.conf) $(grep -c mask $R/shim.log)" "1 1 0"
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE
