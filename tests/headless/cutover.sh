#!/bin/bash
H=$(dirname "$(readlink -f "$0")")
# Rehearse install.sh on a copy of the mango config, against a real Waybar running
# in the test compositor. Everything it kills is matched on this compositor's socket.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
layers(){ mmsg get all-layers | python3 -c "import json,sys;print(sorted(l['name'] for l in json.load(sys.stdin)['layers'] if l['layer']=='top'))"; }
winy(){ mmsg get all-clients | python3 -c "import json,sys;print([c['y'] for c in json.load(sys.stdin)['clients'] if c['appid']=='kitty'][0])"; }
C=$R/cfg; rm -rf $C; mkdir -p $C/scripts; cp ~/.config/mango/config.conf ~/.config/mango/colors.conf $C/; sed -i "s#^source_optional=.*#source_optional=$C/colors.conf#" $C/config.conf
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
$H/../../install.sh --no-build >/dev/null; sleep 1
ok "second run changes nothing" "$(grep -c 'layer_name:island' $C/config.conf) $(layers)" "1 ['island']"
$H/../../install.sh --undo
sleep 2.5
ok "undo brings waybar back" "$(layers)" "['waybar']"
ok "undo removes the config block" "$(grep -c 'layer_name:island' $C/config.conf)" 0
ok "undo restores the bar script" "$(grep -c waybar $C/scripts/bar)" 1
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x waybar --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE

# A stock config: no bar script, nothing to back up, so the block has to start the island.
pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; sleep .3
rm -rf $R/backup $C/scripts; printf 'tag_num=9\n' > $C/config.conf
$H/../../install.sh --no-build
sleep 1
ok "stock config: island comes up" "$(layers)" "['island']"
ok "stock config: autostart added" "$(grep -c '^exec_once=.*scripts/bar' $C/config.conf)" 1
pkill -f "$C/scripts/bar" --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE; pkill -x island --env MANGO_INSTANCE_SIGNATURE=$MANGO_INSTANCE_SIGNATURE
