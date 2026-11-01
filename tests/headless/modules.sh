#!/bin/bash
# Everything a modder touches: layouts, script modules, glyphs, live reload.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
C=$R/cfg; mkdir -p $C/island; export XDG_CONFIG_HOME=$C
cat > $C/island/config.toml <<E
pill = ["custom:hello", "clock"]
panel = ["custom:hello", "stats"]
[[custom]]
name = "hello"
exec = "echo hello world"
on_click = "touch $R/clicked"
[commands]
wallpaper_pick = "touch $R/wall"
E
$H/isl; sleep .3; $V move 1500 900; sleep .8; grim $H/run/p1.png
$V move 960 22; sleep 1.2; grim $H/run/p2.png
ok "custom panel layout: two modules make a short panel" "$(px $H/run/p2.png 690 215 opaque)" no
bright=$(magick $H/run/p2.png -crop 160x24+700+20 -format '%[fx:maxima>0.6?1:0]' info:)
ok "script output draws in the panel" "$bright" 1
$V click 730 32; sleep .4
ok "script module on_click runs" "$([ -f $R/clicked ] && echo ran)" ran
$V move 1500 900; sleep .9

# Unknown module names are skipped, broken config falls back, reload needs no restart
cat > $C/island/config.toml <<E
pill = ["nonsense", "clock"]
icon_style = "color"
E
$BIN reload; sleep .6
kill -0 $(cat $R/island.pid) && echo "ok   unknown module is skipped"
$V move 960 22; sleep 1.2; grim $H/run/p3.png; $V move 1500 900; sleep .8
ok "reload swapped back to the tall default panel" "$(px $H/run/p3.png 690 215 opaque)" yes
echo 'pill = [' > $C/island/config.toml
$BIN reload; sleep .5
kill -0 $(cat $R/island.pid) && echo "ok   broken config keeps it running"
grep -q 'ignoring' $R/island.log && echo "ok   broken config is reported" || echo "FAIL broken config is reported"
rm -f $C/island/config.toml; unset XDG_CONFIG_HOME
