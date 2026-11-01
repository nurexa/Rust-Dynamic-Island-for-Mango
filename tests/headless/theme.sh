#!/bin/bash
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
px(){ python3 $H/px.py "$@"; }
T=$R/home/.config/theme/generated; mkdir -p $T
tok(){ cat <<E
[colors]
accent          = "$1"
on_accent       = "$2"
surface         = "$3"
surface_variant = "#41484d"
on_surface      = "$4"
muted           = "#8a9297"
glow            = "#8fcff3aa"
error           = "#ffb4ab"
E
}
tok "#8fcff3" "#003549" "#0f1417" "#dfe3e7" > $T/colors.toml
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}; HOME=$R/home ISLAND_BIN=$BIN $H/isl
$V move 1500 900; sleep .6; grim $H/run/i1.png
echo "     dark  pill bg: $(px $H/i1.png 960 8)"
# in-place write, as matugen does
tok "#ffb59d" "#561f0f" "#1a110f" "#f1dfda" > $T/colors.toml; sleep .6; grim $H/run/i2.png
echo "     warm  pill bg: $(px $H/i2.png 960 8)"
ok "in-place write repaints" "$([ "$(px $H/i1.png 960 8)" != "$(px $H/i2.png 960 8)" ] && echo changed)" changed
# rename into place
tok "#0a6780" "#ffffff" "#f6fafe" "#171c1f" > $T/new; mv $T/new $T/colors.toml; sleep .6; grim $H/run/i3.png
echo "     light pill bg: $(px $H/i3.png 960 8)"
ok "atomic rename repaints, light palette stays light" "$(px $H/i3.png 960 8 opaque)" no
$V move 960 22; sleep 1.2; grim $H/run/i4.png; magick $H/run/i4.png -crop 640x270+640+0 $H/run/i4s.png; $V move 1500 900; sleep .6
# garbage must not wipe the theme
echo "not a palette" > $T/colors.toml; sleep .5; grim $H/run/i5.png
echo "     after garbage: $(px $H/i5.png 960 8)"
kill -0 $(cat $R/island.pid) && echo "ok   still alive"
tail -2 $R/island.log
