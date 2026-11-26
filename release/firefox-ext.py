#!/usr/bin/env python3
"""Turns the Chromium build of the Thurea extension into the Firefox-family build.

    release/firefox-ext.py SRC DEST VERSION

SRC is a copy of extension/ (as release/build.sh stages it). DEST gets the Firefox version:
  - background: Firefox runs MV3 backgrounds as event pages, not service workers, and event pages
    have no importScripts(). The service worker and every file it imports become one script, in
    the same order, so everything runs exactly as in Chromium.
  - manifest: background.scripts instead of service_worker; no Chromium-only key/update_url;
    browser_specific_settings from extension/firefox/manifest.json; userScripts is an optional
    permission in Firefox (asked for on the Scripts page).
Prints the add-on ID.
"""
import json
import os
import re
import shutil
import sys

src, dest, version = sys.argv[1:4]
if os.path.exists(dest):
    shutil.rmtree(dest)
shutil.copytree(src, dest, ignore=shutil.ignore_patterns("firefox", "*.crx", "*.pem"))

m = json.load(open(os.path.join(src, "manifest.json")))
overlay = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "extension", "firefox", "manifest.json")))
overlay.pop("//", None)

sw = m["background"]["service_worker"]
code = open(os.path.join(src, sw), encoding="utf-8").read()
imports = re.compile(r"^importScripts\(([^)]*)\);\s*$", re.M)
found = imports.findall(code)
if len(found) != 1:
    sys.exit(f"{sw}: expected exactly one importScripts(...) line, found {len(found)}")
files = re.findall(r'"([^"]+\.js)"', found[0])
inlined = "\n".join(f"// ---- {f} ----\n" + open(os.path.join(src, f), encoding="utf-8").read().rstrip() + "\n" for f in files)
code = imports.sub(lambda _: inlined + f"// ---- {sw} (continued) ----", code)
open(os.path.join(dest, "background.js"), "w", encoding="utf-8").write(code)
for f in [sw, *files]:
    # Still used by pages? (userscripts-lib.js is also loaded by scripts.html.) Keep those.
    used = any(f in open(os.path.join(dest, p), encoding="utf-8", errors="ignore").read()
               for p in os.listdir(dest) if p.endswith(".html"))
    if not used:
        os.remove(os.path.join(dest, f))

m["version"] = version
m["background"] = {"scripts": ["background.js"]}
for k in ("key", "update_url", "minimum_chrome_version"):
    m.pop(k, None)
perms = m.get("permissions", [])
optional = [p for p in perms if p == "userScripts"]
m["permissions"] = [p for p in perms if p not in optional]
if optional:
    m["optional_permissions"] = sorted(set(m.get("optional_permissions", []) + optional))
m.update(overlay)
json.dump(m, open(os.path.join(dest, "manifest.json"), "w", encoding="utf-8"), indent=2, ensure_ascii=False)
print(m["browser_specific_settings"]["gecko"]["id"])
