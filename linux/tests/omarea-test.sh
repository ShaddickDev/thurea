#!/bin/bash
# shellcheck disable=SC2034  # the checks are strings, run by eval
# Tests for Omarea (linux/files/omarea): install, uninstall and the notifications, against
# throwaway home folders shaped like Omarchy 4 (omarchy-shell) and Omarchy 3 (Waybar), with
# stand-ins for thurea, omarchy-shell and the notification senders. Needs bash, jq and node.
#
#   linux/tests/omarea-test.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
OMAREA=$HERE/../files/omarea
PLUGIN=$HERE/../omarchy-plugin/omarea
ROOT=$HERE/../..
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
PASS=0 FAIL=0
pass() { PASS=$((PASS + 1)); }
fail() { FAIL=$((FAIL + 1)); printf '  ✗ %s\n' "$*"; }
check() { if eval "$2"; then pass; else fail "$1"; fi; }

# ── Stand-ins ──
BIN=$T/bin; mkdir -p "$BIN"
cat > "$BIN/thurea" <<'EOF'
#!/bin/bash
case $1 in
  desktop-status) cat "$FAKE/status.json" ;;
  family) cat "$FAKE/family.json" 2>/dev/null || echo '{}' ;;
  control-center) echo "control-center $*" >> "$FAKE/calls" ;;
  *) exit 1 ;;
esac
EOF
cat > "$BIN/omarchy-shell" <<'EOF'
#!/bin/bash
[ "$1" = -q ] && shift
echo "$*" >> "$FAKE/shell-calls"
[ -e "$FAKE/shell-up" ] || exit 1
case "$1 $2" in
  "shell ping") echo ok ;;
  "shell enablePlugin") echo ok ;;
  "thurea.omarea "*) [ -e "$FAKE/widget-up" ] && echo ok || { echo "No target named thurea.omarea" >&2; exit 1; } ;;
  *) echo ok ;;
esac
EOF
cat > "$BIN/omarchy-notification-send" <<'EOF'
#!/bin/bash
printf '%s\n' "$*" >> "$FAKE/notes"
EOF
printf '#!/bin/bash\nexit 0\n' > "$BIN/hyprctl"
chmod +x "$BIN"/*
NODE_DIR=$(dirname "$(command -v node || echo /usr/bin/node)")
export PATH="$BIN:$NODE_DIR:/usr/bin:/bin"
export OMAREA_PLUGIN_SRC=$PLUGIN

newhome() {   # newhome NAME: a fresh HOME and FAKE
  export HOME=$T/$1 FAKE=$T/$1-fake XDG_CONFIG_HOME="" XDG_STATE_HOME="" XDG_RUNTIME_DIR=$T/$1-run
  unset XDG_CONFIG_HOME XDG_STATE_HOME
  mkdir -p "$HOME" "$FAKE" "$XDG_RUNTIME_DIR"
  echo '{"state":"ok","line":"This computer is protected","version":"1.0.0"}' > "$FAKE/status.json"
}
om() { bash "$OMAREA" "$@"; }

# Omarchy's own menu parser (shell/plugins/menu/MenuModel.js stripJsonc), then JSON.parse.
menu_keys() {
  node -e '
    const raw = require("fs").readFileSync(process.argv[1], "utf8");
    const s = raw.replace(/^\s*\/\/[^\n]*(\n|$)/gm, "").replace(/,(\s*[}\]])/g, "$1");
    console.log(Object.keys(JSON.parse(s)).join(" "));' "$1"
}
# Waybar reads JSONC: whole-line // comments and trailing commas.
jsonc_ok() {
  node -e '
    const raw = require("fs").readFileSync(process.argv[1], "utf8");
    JSON.parse(raw.replace(/^\s*\/\/[^\n]*(\n|$)/gm, "").replace(/,(\s*[}\]])/g, "$1"));' "$1" 2>/dev/null
}

echo "== plugin manifest (the checks omarchy plugin validate makes)"
for dir in "$PLUGIN" "$ROOT"; do
  out=$(python3 -I - "$dir" <<'PY'
import json, os, re, sys
d = sys.argv[1]; m = json.load(open(os.path.join(d, "manifest.json")))
err = []
if m.get("schemaVersion") != 1: err.append("schemaVersion")
for f in ("id", "name", "version", "kinds", "entryPoints"):
    if f not in m: err.append("missing " + f)
i = m.get("id", "")
if not re.match(r"^[A-Za-z0-9][A-Za-z0-9._-]*$", i) or ".." in i or i.startswith("omarchy."): err.append("id " + i)
for k, ep in {"bar": "bar", "bar-widget": "barWidget", "menu": "menu", "overlay": "overlay", "panel": "panel", "service": "service"}.items():
    if k in m.get("kinds", []) and ep not in m.get("entryPoints", {}): err.append("kind %s needs %s" % (k, ep))
for ep in m.get("entryPoints", {}).values():
    if ep.startswith("/") or ".." in ep or not os.path.isfile(os.path.join(d, ep)): err.append("entry point " + ep)
if m.get("barWidget", {}).get("defaultSection", "right") not in ("left", "centerBegin", "center", "centerEnd", "tray", "right"): err.append("section")
for root, dirs, files in os.walk(d):
    dirs[:] = [x for x in dirs if x not in (".git", "target", "node_modules")]
    for x in dirs + files:
        if os.path.islink(os.path.join(root, x)): err.append("symlink " + os.path.join(root, x))
print("; ".join(err))
PY
)
  check "manifest in $dir: $out" '[ -z "$out" ]'
done
check "plugin and root manifest agree on the id" '[ "$(jq -r .id "$PLUGIN/manifest.json")" = "$(jq -r .id "$ROOT/manifest.json")" ]'
check "plugin id is thurea.omarea" '[ "$(jq -r .id "$PLUGIN/manifest.json")" = thurea.omarea ]'
check "Panel.qml answers on the thurea.omarea IPC target" 'grep -q "ipcTarget: \"thurea.omarea\"" "$PLUGIN/Panel.qml"'
check "en.json is valid JSON" 'jq -e . "$PLUGIN/i18n/en.json" >/dev/null'

echo "== which Omarchy"
newhome detect
check "not Omarchy: 0" '[ "$(OMARCHY_PATH= bash -c ". /dev/null; HOME=$HOME bash $OMAREA install" | grep -c "This isn" )" = 1 ]'
mkdir -p "$HOME/.config/omarchy" "$HOME/.local/share/omarchy"
check "Omarchy 3: no shell" 'OMARCHY_PATH=$HOME/.local/share/omarchy om install | grep -q "for Omarchy 3"'
mkdir -p "$HOME/.local/share/omarchy/shell"; : > "$HOME/.local/share/omarchy/shell/shell.qml"
check "Omarchy 4: omarchy-shell" 'OMARCHY_PATH=$HOME/.local/share/omarchy om install | grep -q "for Omarchy 4"'

echo "== Omarchy 4: install"
export OMAREA_OMARCHY=4
newhome o4
C=$HOME/.config
mkdir -p "$C/omarchy/extensions" "$C/hypr"
cat > "$C/omarchy/shell.json" <<'EOF'
{
  "version": 1,
  "bar": { "layout": { "left": [{ "id": "omarchy.menu" }], "right": [{ "id": "omarchy.network" }, { "id": "omarchy.audio" }] } }
}
EOF
cat > "$C/omarchy/extensions/omarchy-menu.jsonc" <<'EOF'
{
  // Extend the Quickshell Omarchy menu with JSONC.
  // "personal": {"icon":"","label":"Personal"},
  "personal": {"icon":"x","label":"Personal"},
  "personal.notes": {"icon":"y","label":"Notes","action":"omarchy-launch-editor ~/notes"}
}
EOF
cat > "$C/hypr/bindings.lua" <<'EOF'
-- Keep only your personal keybinding overrides here.
-- o.bind("SUPER + SHIFT + R", "SSH", "xdg-terminal-exec -e ssh your-server")
o.bind("SUPER + SHIFT + R", "SSH", "xdg-terminal-exec -e ssh box")
EOF
cp -a "$C" "$T/o4-before"
om install > "$T/out" 2>&1
check "install says it's done" 'grep -q "Done." "$T/out"'
check "plugin linked to the package's copy" '[ "$(readlink "$C/omarchy/plugins/thurea.omarea")" = "$PLUGIN" ]'
check "in the bar, first on the right (shell not running: shell.json)" '[ "$(jq -r ".bar.layout.right[0].id" "$C/omarchy/shell.json")" = thurea.omarea ]'
check "the rest of the bar kept" '[ "$(jq -c "[.bar.layout.right[].id]" "$C/omarchy/shell.json")" = "[\"thurea.omarea\",\"omarchy.network\",\"omarchy.audio\"]" ]'
keys=$(menu_keys "$C/omarchy/extensions/omarchy-menu.jsonc")
check "menu still parses, user's entries kept ($keys)" '[[ " $keys " == *" personal "* && " $keys " == *" personal.notes "* ]]'
check "Omarea submenu in the menu" '[[ " $keys " == *" omarea "* && " $keys " == *" omarea.family "* && " $keys " == *" omarea.phone "* ]]'
# Every menu action is a real omarea command.
bad_actions=$(node -e '
  const raw = require("fs").readFileSync(process.argv[1], "utf8");
  const m = JSON.parse(raw.replace(/^\s*\/\/[^\n]*(\n|$)/gm, "").replace(/,(\s*[}\]])/g, "$1"));
  for (const k in m) if (k.startsWith("omarea") && m[k].action) console.log(m[k].action);' "$C/omarchy/extensions/omarchy-menu.jsonc" |
  while read -r _ sub _; do grep -qE "^  ($sub|$sub\|[a-z|-]+|[a-z|-]+\|$sub)\)" "$OMAREA" || echo "$sub"; done)
check "menu actions are omarea commands (${bad_actions:-})" '[ -z "$bad_actions" ]'
check "keybindings added once" '[ "$(grep -c "o.bind(\"SUPER + CTRL + U\", \"Omarea\"" "$C/hypr/bindings.lua")" = 1 ]'
check "user's own binding kept" 'grep -q "ssh box" "$C/hypr/bindings.lua"'
cp -a "$C" "$T/o4-once"
om install >/dev/null 2>&1
check "install twice changes nothing" 'diff -r "$T/o4-once" "$C" >/dev/null'

echo "== Omarchy 4: with the shell running"
newhome o4up
C=$HOME/.config; mkdir -p "$C/omarchy"; touch "$FAKE/shell-up"
om install >/dev/null 2>&1
check "asks the running shell to put it in the bar" 'grep -q "shell enablePlugin thurea.omarea {\"section\":\"right\",\"index\":0}" "$FAKE/shell-calls"'
check "no widget yet: open says so in a notification" '! om toggle && grep -q "isn.t in the bar" "$FAKE/notes"'
touch "$FAKE/widget-up"
check "open goes to the widget" 'om toggle && grep -q "thurea.omarea toggle" "$FAKE/shell-calls"'
check "phone goes to the widget" 'om phone && grep -q "thurea.omarea setupPhone" "$FAKE/shell-calls"'
check "family opens the control centre" 'om family && grep -q "control-center control-center" "$FAKE/calls"'
check "downloads opens its page" 'om downloads && grep -q "control-center --page downloads" "$FAKE/calls"'

echo "== Omarchy 4: from the old Thurea bar plugin"
newhome o4old
C=$HOME/.config; mkdir -p "$C/omarchy/plugins"
ln -s /usr/share/thurea/omarchy-plugin "$C/omarchy/plugins/thurea.omarchy"
cat > "$C/omarchy/shell.json" <<'EOF'
{"version":1,"bar":{"layout":{"right":[{"id":"omarchy.network"},{"id":"omarchy.audio"},{"id":"thurea.omarchy","x":1}]}}}
EOF
om install --auto >/dev/null 2>&1
check "old plugin link gone" '[ ! -e "$C/omarchy/plugins/thurea.omarchy" ] && [ ! -L "$C/omarchy/plugins/thurea.omarchy" ]'
check "Omarea takes the old widget's place" '[ "$(jq -c ".bar.layout.right[2]" "$C/omarchy/shell.json")" = "{\"id\":\"thurea.omarea\",\"x\":1}" ]'
check "and isn't added twice" '[ "$(jq "[.. | objects | select(.id? == \"thurea.omarea\")] | length" "$C/omarchy/shell.json")" = 1 ]'

echo "== Omarchy 4: added with omarchy plugin add (a git checkout)"
newhome o4git
C=$HOME/.config; mkdir -p "$C/omarchy/plugins/thurea.omarea/.git"
printf '[remote "origin"]\n\turl = https://github.com/ShaddickDev/thurea.git\n' > "$C/omarchy/plugins/thurea.omarea/.git/config"
om install --auto >/dev/null 2>&1
check "Thurea's checkout swapped for the package's self-updating copy" '[ "$(readlink "$C/omarchy/plugins/thurea.omarea")" = "$PLUGIN" ]'
check "and kept outside Omarchy's plugins folder" '[ -f "$HOME/.local/state/omarea/git-checkout/.git/config" ]'
newhome o4own
C=$HOME/.config; mkdir -p "$C/omarchy/plugins/thurea.omarea/.git"
printf '[remote "origin"]\n\turl = https://example.org/me/fork.git\n' > "$C/omarchy/plugins/thurea.omarea/.git/config"
om install --auto >/dev/null 2>&1
check "someone else's checkout is left alone" '[ -d "$C/omarchy/plugins/thurea.omarea" ] && [ ! -L "$C/omarchy/plugins/thurea.omarea" ]'

newhome o4gone
C=$HOME/.config; mkdir -p "$C/omarchy" "$C/thurea"; touch "$C/thurea/.omarchy-bar"
echo '{"version":1,"bar":{"layout":{"right":[{"id":"omarchy.network"}]}}}' > "$C/omarchy/shell.json"
om install --auto >/dev/null 2>&1
check "took the old plugin out of the bar: Omarea stays out too" '[ "$(jq -c "[.bar.layout.right[].id]" "$C/omarchy/shell.json")" = "[\"omarchy.network\"]" ]'

echo "== Omarchy 4: keys already taken"
newhome o4keys
C=$HOME/.config; mkdir -p "$C/omarchy" "$C/hypr"
echo 'o.bind("SUPER + CTRL + U", "Mine", "my-thing")' > "$C/hypr/bindings.lua"
om install > "$T/out" 2>&1
check "says the keys are yours" 'grep -q "already yours" "$T/out"'
check "leaves your binding alone" '[ "$(cat "$C/hypr/bindings.lua")" = "o.bind(\"SUPER + CTRL + U\", \"Mine\", \"my-thing\")" ]'

echo "== Omarchy 4: uninstall"
export HOME=$T/o4 FAKE=$T/o4-fake
C=$HOME/.config
om uninstall > "$T/out" 2>&1
check "says protection is unchanged" 'grep -q "protection is unchanged" "$T/out"'
check "menu back to exactly what it was" 'cmp -s "$T/o4-before/omarchy/extensions/omarchy-menu.jsonc" "$C/omarchy/extensions/omarchy-menu.jsonc"'
check "bindings back to exactly what they were" 'cmp -s "$T/o4-before/hypr/bindings.lua" "$C/hypr/bindings.lua"'
check "out of the bar" '[ "$(jq -c "[.bar.layout.right[].id]" "$C/omarchy/shell.json")" = "[\"omarchy.network\",\"omarchy.audio\"]" ]'
check "plugin link gone" '[ ! -e "$C/omarchy/plugins/thurea.omarea" ]'
om install --auto >/dev/null 2>&1
check "Thurea's updates respect it (install --auto does nothing)" '[ ! -e "$C/omarchy/plugins/thurea.omarea" ]'
om install >/dev/null 2>&1
check "omarea install brings it back" '[ -L "$C/omarchy/plugins/thurea.omarea" ] && [ "$(jq -r ".bar.layout.right[0].id" "$C/omarchy/shell.json")" = thurea.omarea ]'

echo "== Omarchy 3: Waybar"
export OMAREA_OMARCHY=3
newhome o3
C=$HOME/.config; mkdir -p "$C/omarchy" "$C/waybar" "$C/hypr"
cat > "$C/waybar/config.jsonc" <<'EOF'
{
  "reload_style_on_change": true,
  "layer": "top",
  "modules-left": ["custom/omarchy", "hyprland/workspaces"],
  "modules-right": [
    "group/tray-expander",
    "bluetooth",
    "battery"
  ],
  "battery": {
    "format": "{capacity}% {icon}"
  }
}
EOF
printf '@import "../omarchy/current/theme/waybar.css";\n\n* {\n  font-size: 12px;\n}\n' > "$C/waybar/style.css"
printf '# Application bindings\nbindd = SUPER, RETURN, Terminal, exec, uwsm-app -- xdg-terminal-exec\n' > "$C/hypr/bindings.conf"
cp -a "$C" "$T/o3-before"
om install > "$T/out" 2>&1
check "Waybar module added" 'grep -q "Waybar module" "$T/out"'
check "config still parses" 'jsonc_ok "$C/waybar/config.jsonc"'
check "first in modules-right" '[ "$(node -e "const r=require(\"fs\").readFileSync(process.argv[1],\"utf8\");const j=JSON.parse(r.replace(/^\s*\/\/[^\n]*(\n|\$)/gm,\"\").replace(/,(\s*[}\]])/g,\"\$1\"));console.log(j[\"modules-right\"][0], j[\"custom/omarea\"].exec)" "$C/waybar/config.jsonc")" = "custom/omarea omarea status --waybar" ]'
check "style for the module" 'grep -q "#custom-omarea.attention" "$C/waybar/style.css"'
check "Hyprland binding (bindd)" 'grep -q "^bindd = SUPER CTRL, U, Omarea, exec, omarea toggle" "$C/hypr/bindings.conf"'
check "no Omarchy 4 menu file on Omarchy 3" '[ ! -e "$C/omarchy/extensions/omarchy-menu.jsonc" ]'
cp -a "$C" "$T/o3-once"; om install >/dev/null 2>&1
check "install twice changes nothing" 'diff -r "$T/o3-once" "$C" >/dev/null'
# One-line modules-right too.
sed -i -e ':a;N;$!ba;s/"modules-right": \[\n *"group\/tray-expander",\n *"bluetooth",\n *"battery"\n *\]/"modules-right": ["bluetooth", "battery"]/' "$T/o3-before/waybar/config.jsonc"
om uninstall >/dev/null 2>&1
check "uninstall: Waybar config back to what it was" 'cmp -s "$T/o3-once/../o3-before/waybar/style.css" "$C/waybar/style.css"'
check "uninstall: bindings back to what they were" 'cmp -s "$T/o3-before/hypr/bindings.conf" "$C/hypr/bindings.conf"'
check "uninstall: no custom/omarea left" '! grep -q omarea "$C/waybar/config.jsonc" && jsonc_ok "$C/waybar/config.jsonc"'
cp "$T/o3-before/waybar/config.jsonc" "$C/waybar/config.jsonc"; rm -f "$C/thurea/.omarea-off"
om install >/dev/null 2>&1
check "one-line modules-right: added and parses" 'grep -q "\"modules-right\": \[\"custom/omarea\", \"bluetooth\"" "$C/waybar/config.jsonc" && jsonc_ok "$C/waybar/config.jsonc"'
om uninstall >/dev/null 2>&1
check "one-line modules-right: removed exactly" 'cmp -s "$T/o3-before/waybar/config.jsonc" "$C/waybar/config.jsonc"'

echo "== Waybar status"
newhome bar
w=$(om status --waybar)
check "Waybar JSON: protected" '[ "$(jq -r ".class[0] + \" \" + .alt" <<<"$w")" = "protected ok" ]'
echo '{"state":"degraded","line":"Not fully protected"}' > "$FAKE/status.json"
echo '{"parent":true,"inbox":[{"id":"a","child":"Mia","label":"Instagram"}]}' > "$FAKE/family.json"
w=$(om status --waybar)
check "Waybar JSON: attention, waiting" '[ "$(jq -c .class <<<"$w")" = "[\"attention\",\"waiting\"]" ] && jq -r .tooltip <<<"$w" | grep -q "1 family request waiting"'

echo "== notifications"
newhome notes
notes() { cat "$FAKE/notes" 2>/dev/null | wc -l; }
echo '{"parent":true,"day":"2026-11-26","inbox":[{"id":"old","child":"Sam","label":"TikTok"}],"children":{}}' > "$FAKE/family.json"
om check
check "first look: nothing old is announced" '[ "$(notes)" = 0 ]'
echo '{"parent":true,"day":"2026-11-26","inbox":[{"id":"old","child":"Sam","label":"TikTok"},{"id":"n1","child":"Mia","label":"Instagram"}],"children":{}}' > "$FAKE/family.json"
om check
check "a new request: one notification" '[ "$(notes)" = 1 ] && grep -q "Mia is asking For Instagram" "$FAKE/notes" && grep -q -- "--action Open Family omarea family" "$FAKE/notes"'
om check
check "the same request: not again" '[ "$(notes)" = 1 ]'
echo '{"parent":true,"day":"2026-11-26","inbox":[{"id":"n2","child":"Mia","label":"A"},{"id":"n3","child":"Sam","label":"B"}],"children":{}}' > "$FAKE/family.json"
om check
check "two new: one notification for both" '[ "$(notes)" = 2 ] && tail -n1 "$FAKE/notes" | grep -q "2 family requests"'
echo '{"state":"degraded","line":"Not fully protected: Thurea is putting it back"}' > "$FAKE/status.json"
om check
check "protection off for one look: no alarm yet (Thurea is putting it back)" '[ "$(notes)" = 2 ]'
om check
check "still off: one critical notification" '[ "$(notes)" = 3 ] && tail -n1 "$FAKE/notes" | grep -q -- "-u critical Protection needs attention"'
om check
check "and only one" '[ "$(notes)" = 3 ]'
echo '{"state":"ok","line":"This computer is protected"}' > "$FAKE/status.json"
om check
check "protected again: says so" '[ "$(notes)" = 4 ] && tail -n1 "$FAKE/notes" | grep -q "Protected again"'
echo '{"parent":true,"day":"2026-11-26","inbox":[],"children":{"Mia":{"today":{"used":52,"allowed":60}},"Sam":{"today":{"used":10,"allowed":60}}}}' > "$FAKE/family.json"
om check
check "8 minutes left: once" '[ "$(notes)" = 5 ] && tail -n1 "$FAKE/notes" | grep -q "Mia: 8 min left"'
om check
check "not again the same day" '[ "$(notes)" = 5 ]'
echo '{"parent":false,"inbox":[{"id":"x","child":"Mia","label":"Z"}]}' > "$FAKE/family.json"
om check
check "not a parent: no family notifications" '[ "$(notes)" = 5 ]'
om notifications off >/dev/null
echo '{"state":"unknown","line":"x"}' > "$FAKE/status.json"
check "notifications off: status says so" '! om notifications status'
om notifications on >/dev/null
check "notifications on again" 'om notifications status'

echo "== doctor"
export OMAREA_OMARCHY=4
newhome doc
mkdir -p "$HOME/.config/omarchy"
check "doctor fails before install" '! om doctor >/dev/null 2>&1'
om install >/dev/null 2>&1
out=$(om doctor 2>&1)
check "doctor after install: all good ($out)" '! grep -q "✗" <<<"$(sed "s/\x1b\[[0-9;]*m//g" <<<"$out")"'

echo
echo "$PASS passed, $FAIL failed"
[ "$FAIL" = 0 ]
