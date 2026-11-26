#!/bin/bash
# Publishes dist/ (from release/build.sh) as GitHub release vVERSION of ShaddickDev/thurea and marks
# it latest, which is what every installed Thurea checks. Refuses if the manifest isn't signed
# with the release key or a file doesn't match it.
set -euo pipefail
R=$(cd "$(dirname "$0")/.." && pwd)
D=$R/dist
REPO=${THUREA_REPO:-ShaddickDev/thurea}
openssl dgst -sha256 -verify "$R/release/release-key.pub.pem" -signature "$D/thurea-manifest.json.sig" "$D/thurea-manifest.json" >/dev/null
ASSETS=(thurea-extension.crx thurea-extension-firefox.xpi thurea-linux.pkg.tar.zst thurea-linux.deb thurea-linux.rpm thurea-windows.zip thurea-android.apk)
VER=$(python3 - "$D" "${ASSETS[@]}" <<'PY'
import hashlib, json, sys
d = sys.argv[1]; m = json.load(open(f"{d}/thurea-manifest.json"))
files = m["files"].values()
if sorted(f["name"] for f in files) != sorted(sys.argv[2:]):
    sys.exit("The signed manifest does not list the expected release assets")
for f in files:
    with open(f"{d}/{f['name']}", "rb") as asset:
        if hashlib.file_digest(asset, "sha256").hexdigest() != f["sha256"]:
            sys.exit(f"Asset checksum mismatch: {f['name']}")
print(m["version"])
PY
)
for i in "${!ASSETS[@]}"; do ASSETS[i]="$D/${ASSETS[i]}"; done
python3 "$R/release/check-public.py" --assets "${ASSETS[@]}"
echo "Publishing Thurea $VER to github.com/$REPO"
gh release create "v$VER" --repo "$REPO" --title "Thurea $VER" --latest \
  --notes "Thurea $VER for Windows, Linux (Arch / Omarchy, Debian / Ubuntu, Fedora) and Android. Installed copies update themselves; every file is checked against the signed thurea-manifest.json." \
  "${ASSETS[@]}" \
  "$D"/thurea-manifest.json "$D"/thurea-manifest.json.sig
