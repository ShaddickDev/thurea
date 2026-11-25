#!/usr/bin/python3 -I
"""Assign Thurea's keyboard shortcuts in Chromium's preferences.

Chromium only assigns an extension's suggested shortcuts when it is first
installed, so shortcuts added to Thurea later (panic, copy URL, download
video) were never bound. Run this while Chromium is CLOSED: it waits for
Chromium to exit, then binds Alt+Shift+X / L / D / R to Thurea (replacing the
stale bindings of the removed Omarchy extensions). A backup is kept.
"""
import json, os, shutil, subprocess, time

PREFS = os.path.expanduser("~/.config/chromium/Default/Preferences")
THUREA = "hcjcijjkmdgjpomggbhldhjmdkmnnnhl"
KEYS = {"Alt+Shift+X": "panic", "Alt+Shift+L": "copy-url", "Alt+Shift+D": "download-video", "Alt+Shift+R": "random-video"}

def chromium_running():
    return subprocess.run(["pgrep", "-x", "chromium"], capture_output=True).returncode == 0

if chromium_running():
    print("Waiting for Chromium to close…")
    while chromium_running():
        time.sleep(1)
    time.sleep(1)

shutil.copy2(PREFS, PREFS + ".before-thurea-shortcuts")
with open(PREFS) as f:
    prefs = json.load(f)
ext = prefs.setdefault("extensions", {})
bindings = ext.setdefault("commands", {})
for key, cmd in KEYS.items():
    bindings["linux:" + key] = {"command_name": cmd, "extension": THUREA, "global": False}
thurea_cmds = ext.setdefault("settings", {}).setdefault(THUREA, {}).setdefault("commands", {})
for key, cmd in KEYS.items():
    thurea_cmds[cmd] = {"suggested_key": key, "was_assigned": True}
with open(PREFS + ".tmp", "w") as f:
    json.dump(prefs, f, separators=(",", ":"))
os.replace(PREFS + ".tmp", PREFS)
print("Thurea shortcuts set: " + ", ".join(f"{k} = {c}" for k, c in KEYS.items()))
print("Backup: " + PREFS + ".before-thurea-shortcuts")
