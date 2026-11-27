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
[ -f "$D/thurea-windows.msix" ] && ASSETS+=(thurea-windows.msix)
# Not in the signed manifest (nothing updates from it): trusts a self-signed signing certificate.
EXTRA=(); [ -f "$D/Trust-Thurea.cmd" ] && EXTRA+=("$D/Trust-Thurea.cmd")
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
# Release notes: the fixed intro, then this version's section of CHANGELOG.md if it has one.
NOTES="Thurea $VER for Windows, Linux (Arch / Omarchy, Debian / Ubuntu, Fedora) and Android. Installed copies update themselves; every file is checked against the signed thurea-manifest.json.

**Single adult** (just your own devices): install the file for your system below and you are done.

**Family** (parents and children): the same files on every device, then follow [Family setup](https://github.com/ShaddickDev/thurea/blob/master/docs/install.md#family)."
CHANGES=$(awk -v v="$VER" '/^## /{f = ($2 == v); next} f' "$R/CHANGELOG.md" 2>/dev/null || true)
[ -n "$CHANGES" ] && NOTES="$NOTES"$'\n\n'"$CHANGES"
# THUREA_TARGET (a commit) tags that commit when the tag doesn't exist yet (the GitHub workflow).
TARGET=(); [ -n "${THUREA_TARGET:-}" ] && TARGET=(--target "$THUREA_TARGET")
gh release create "v$VER" --repo "$REPO" --title "Thurea $VER" --latest "${TARGET[@]}" \
  --notes "$NOTES" \
  "${ASSETS[@]}" \
  "$D"/thurea-manifest.json "$D"/thurea-manifest.json.sig "${EXTRA[@]}"
