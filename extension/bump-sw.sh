#!/bin/bash
# Chromium caches the service worker of extensions loaded with --load-extension
# and keeps running the old code until the worker's file name changes. Run this
# after editing background-*.js or any file it importScripts().
set -euo pipefail
cd "$(dirname "$0")"
cur=$(ls background-*.js | sed -E 's/background-([0-9]+)\.js/\1/' | sort -n | tail -1)
next=$((cur + 1))
mv "background-$cur.js" "background-$next.js"
python3 -I - "$next" <<'PY'
import json,sys
m=json.load(open("manifest.json"))
m["background"]["service_worker"]=f"background-{sys.argv[1]}.js"
json.dump(m,open("manifest.json","w"),indent=2); open("manifest.json","a").write("\n")
PY
echo "service worker is now background-$next.js"
