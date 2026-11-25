#!/bin/bash
# Stages what the Thurea Windows 11 ISO installs, from a Thurea release (release/build.sh):
#   out/thurea-stage/Setup-Thurea.ps1                  run by Windows Setup (specialize pass)
#   out/thurea-stage/helium_*_x64-mini-installer.exe   Helium, made the default browser
#   out/thurea-stage/Thurea/...                        thurea-windows.zip, unpacked (+ the zip itself,
#                                                      offered by the tray's QR page and kept for repairs)
# The ISO therefore contains exactly the release that devices update from.
set -euo pipefail
A=$(cd "$(dirname "$0")" && pwd)
R=$(cd "$A/.." && pwd)
DIST=${THUREA_DIST:-$R/dist}
ZIP=$DIST/thurea-windows.zip
[ -f "$ZIP" ] || { echo "no $ZIP: run release/build.sh first"; exit 1; }
openssl dgst -sha256 -verify "$R/release/release-key.pub.pem" -signature "$DIST/thurea-manifest.json.sig" "$DIST/thurea-manifest.json" >/dev/null
python3 - "$DIST" <<'PY'
import hashlib, json, sys
d = sys.argv[1]; m = json.load(open(f"{d}/thurea-manifest.json"))
f = m["files"]["windows"]
assert hashlib.sha256(open(f"{d}/{f['name']}", "rb").read()).hexdigest() == f["sha256"], "zip doesn't match the signed manifest"
PY
python3 "$R/release/check-public.py" --assets "$ZIP"
HELIUM=$(ls "$A"/downloads/helium_*_x64-mini-installer.exe | sort -V | tail -1)
OUT=$A/out/thurea-stage
rm -rf "$OUT"; mkdir -p "$OUT"
cp "$A/iso/Setup-Thurea.ps1" "$HELIUM" "$OUT/"
sed -i 's/\r\?$/\r/' "$OUT/Setup-Thurea.ps1"
( cd "$OUT" && python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall('.')" "$ZIP" )
mkdir -p "$OUT/Thurea/windows"; cp "$ZIP" "$OUT/Thurea/windows/thurea-windows.zip"
echo "Staged Thurea $(cat "$OUT/Thurea/VERSION") (signed release) in $OUT"
du -sh "$OUT"/*
