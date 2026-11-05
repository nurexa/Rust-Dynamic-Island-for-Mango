#!/bin/bash
# Claude Code: hooks in settings.json, the pill, permission cards and done notes.
# The hook is fed events by hand, the way Claude Code would run it.
H=$(dirname "$(readlink -f "$0")"); source $H/run/env; V=$H/vp; R=$XDG_RUNTIME_DIR
ok(){ if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2' want '$3'"; fi; }
# The pill's width: its background is darker and less blue than the desktop around it.
pillw(){ magick "$1" -crop 1920x1+0+35 -fx "b<0.055" -format "%[fx:round(mean*w)]" info:; }
wider(){ [ $(( $(pillw $2) - $(pillw $1) )) -gt 40 ] && echo wider || echo "same ($(pillw $1) -> $(pillw $2))"; }
light(){ magick "$1" -crop 40x16+$(($2-20))+$(($3-8)) -format "%[fx:mean>0.12?1:0]" info: | sed 's/1/light/;s/0/dark/'; }
export DBUS_SESSION_BUS_ADDRESS=unix:path=$R/bus PATH=$H/shim:$PATH CLAUDE_CONFIG_DIR=$R/claude
BIN=${ISLAND_BIN:-$(readlink -f $H/../../target/release/island)}
rm -rf $R/claude; mkdir -p $R/claude
printf '{"theme":"dark","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}' > $R/claude/settings.json
cp $R/claude/settings.json $R/claude.orig
hooks(){ python3 -c "import json;d=json.load(open('$R/claude/settings.json'));print(sum('claude-hook' in h['command'] for g in d.get('hooks',{}).values() for e in g for h in e['hooks']), d['theme'])"; }
$BIN claude-setup >/dev/null; $BIN claude-setup >/dev/null
ok "setup adds seven hooks once and keeps the rest" "$(hooks)" "7 dark"
$BIN claude-setup --undo >/dev/null
ok "setup --undo leaves the file as it was" "$(python3 -c "import json;print(json.load(open('$R/claude/settings.json'))==json.load(open('$R/claude.orig')))")" True

ev(){ printf '{"hook_event_name":"%s","session_id":"s1","cwd":"/home/u/island"%s}' "$1" "$2"; }
ASK=',"tool_name":"Bash","tool_input":{"command":"cargo test"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"cargo test:*"}],"behavior":"allow","destination":"localSettings"}]'
# Pin is saved across restarts and other tests leave it either way: set it, don't toggle it.
pinned(){ grep -qx pinned $R/island/state 2>/dev/null; }
pin(){ pinned || $BIN pin; }
unpin(){ pinned && $BIN pin; true; }
unpin; sleep 1; grim -g "560,0 800x60" $H/run/cl0.png
ev UserPromptSubmit | $BIN claude-hook; sleep .8; grim -g "560,0 800x60" $H/run/cl1.png
ok "a prompt shows Claude working on the pill" "$(wider $H/run/cl0.png $H/run/cl1.png)" wider
answer(){ ev PermissionRequest "$ASK" | $BIN claude-hook > $R/cl.out & sleep 1.6; grim -g "560,0 800x120" $H/run/cl2.png; $V click $1 83; sleep .8; }
answer 794
ok "a permission prompt becomes a card" "$(light $H/run/cl2.png 250 31)" light
ok "Allow answers allow" "$(python3 -c "import json;print(json.load(open('$R/cl.out'))['hookSpecificOutput']['decision']['behavior'])")" allow
answer 877
ok "Always allow carries Claude's suggested rule" "$(python3 -c "import json;print(json.load(open('$R/cl.out'))['hookSpecificOutput']['decision']['updatedPermissions'][0]['rules'][0]['ruleContent'])")" "cargo test:*"
answer 960
ok "Deny answers deny" "$(python3 -c "import json;print(json.load(open('$R/cl.out'))['hookSpecificOutput']['decision']['behavior'])")" deny
answer 1145
ok "closing the card leaves it to the terminal" "$(wc -c < $R/cl.out)" 0
ev PermissionRequest "$ASK" | $BIN claude-hook > $R/cl.out & sleep 1
ev PostToolUse ',"tool_name":"Bash"' | $BIN claude-hook; sleep .6; grim -g "560,0 800x120" $H/run/cl3.png
ok "answered in the terminal: the card goes" "$(light $H/run/cl3.png 250 31)" dark
# From a terminal you're looking at, the terminal asks and the hook returns at once.
kitty sh -c "printf '{\"hook_event_name\":\"PermissionRequest\",\"session_id\":\"s2\",\"cwd\":\"/x\",\"tool_name\":\"Bash\"}' | $BIN claude-hook > $R/cl5.out; echo done >> $R/cl5.out" & sleep 2.5
ok "a focused terminal gets no card" "$(cat $R/cl5.out)" done
ev Stop ',"last_assistant_message":"All tests pass."' | $BIN claude-hook; sleep .8; grim -g "560,0 800x120" $H/run/cl4.png
ok "a finished session you weren't watching leaves a note" "$(light $H/run/cl4.png 250 31)" light
ev SessionEnd | $BIN claude-hook
kill -0 $(cat $R/island.pid) && echo "ok   alive after every hook"
# Leave a plain island behind, with no notes up, for whatever runs next.
$H/isl
