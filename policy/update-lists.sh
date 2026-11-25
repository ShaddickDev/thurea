#!/bin/bash
# Rebuilds Thurea's adult-site list from openly licensed sources only, so every entry can be
# credited (see CREDITS.md):
#
#   StevenBlack/hosts, alternates/porn-only   MIT        (itself built from Sinfonietta and
#                                                         Clefspeare13's lists, both MIT)
#   blocklistproject/Lists, porn              Unlicense  (public domain), only for entries already
#                                                         on Thurea's list, so the list stays small
#
# Writes extension/filters/porn-domains.txt (also the Linux hosts block and Windows/Firefox
# filters, via release/build.sh) and extension/porn-rules.json (the extension's static rules).
#
#   policy/update-lists.sh            fetch the sources and rebuild
set -euo pipefail
R=$(cd "$(dirname "$0")/.." && pwd)
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
UA="User-Agent: thurea-list-builder"
curl -fsSL -H "$UA" -o "$T/sb" https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/porn-only/hosts
curl -fsSL -H "$UA" -o "$T/bp" https://raw.githubusercontent.com/blocklistproject/Lists/master/alt-version/porn-nl.txt
python3 -I - "$T" "$R" <<'PY'
import json, re, sys
t, r = sys.argv[1], sys.argv[2]
ok = re.compile(r"^(?=.{4,253}$)([a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,63}$")
# Never on the list, whatever a source says (the extension's own filters handle these sites).
never = re.compile(r"(^|\.)(google\.[a-z.]+|youtube\.com|youtu\.be|googlevideo\.com|reddit\.com|redd\.it|twitter\.com|x\.com|twimg\.com|twitch\.tv|duckduckgo\.com|bing\.com|github\.com|githubusercontent\.com)$")
def clean(names):
    out = set()
    for n in names:
        n = n.strip().lower().rstrip(".")
        if n.startswith("www.") and n.count(".") >= 2:
            n = n[4:]
        if ok.match(n) and not never.search(n):
            out.add(n)
    return out
sb = clean(l.split()[1] for l in open(f"{t}/sb") if l.startswith("0.0.0.0 ") and len(l.split()) >= 2)
bp = clean(l.split()[-1] for l in open(f"{t}/bp") if l.strip() and not l.startswith("#"))
cur_path = f"{r}/extension/filters/porn-domains.txt"
cur = clean(open(cur_path)) if __import__("os").path.exists(cur_path) else set()
final = sorted(sb | (cur & bp))
assert len(final) > 50000, f"only {len(final)} domains: a source probably failed"
open(cur_path, "w").write("\n".join(final) + "\n")
sub = ["sub_frame", "media", "image", "xmlhttprequest", "script", "stylesheet", "font", "object", "ping", "websocket", "other"]
rules = [
  {"id": 1, "priority": 20, "action": {"type": "redirect", "redirect": {"extensionPath": "/porn-block.html"}},
   "condition": {"requestDomains": final, "resourceTypes": ["main_frame"]}},
  {"id": 2, "priority": 20, "action": {"type": "block"}, "condition": {"requestDomains": final, "resourceTypes": sub}},
]
json.dump(rules, open(f"{r}/extension/porn-rules.json", "w"), separators=(",", ":"))
# The Rust core's built-in list (first policy version) must stay a subset of the full list.
core = [l.strip() for l in open(f"{r}/policy/blocklist.txt") if l.strip()]
missing = [d for d in core if d not in set(final)]
print(f"{len(final)} domains ({len(sb)} from StevenBlack/hosts, {len(final) - len(sb)} more from blocklistproject)")
if missing:
    print(f"note: {len(missing)} built-in core entries aren't in the sources: {', '.join(missing[:10])}")
PY
