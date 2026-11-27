#!/bin/bash
# Builds a complete Thurea release: every platform from this repo, all with one version number.
#
#   release/build.sh [VERSION]       (default: release/VERSION)
#
# dist/ then holds the release (the asset names never change, so devices can always fetch
# releases/latest/download/<name>):
#   thurea-extension.crx       the Thurea browser extension (also inside every desktop package)
#   thurea-extension-firefox.xpi  the same for Firefox-family browsers (signed with AMO_JWT_*)
#   thurea-linux.pkg.tar.zst   Thurea for Linux (Arch / Omarchy pacman package)
#   thurea-windows.zip         Thurea for Windows (Thurea\Install Thurea.cmd)
#   thurea-android.apk         Thurea for Android
#   thurea-manifest.json       version + SHA-256 of each file
#   thurea-manifest.json.sig   RSA-SHA256 signature of the manifest with the release key
# Every installed Thurea accepts an update only if the manifest's signature checks out against the
# public key it shipped with (release/release-key.pub.pem) and the file matches its checksum.
#
# Keys (never in the repo): ~/.config/thurea/release-key.pem (release signing),
# ~/.config/thurea/extension-key.pem (extension ID), the Android keystore (android/build.sh).
set -euo pipefail
R=$(cd "$(dirname "$0")/.." && pwd)
VER=${1:-$(cat "$R/release/VERSION")}
[[ $VER =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be like 1.0.0"; exit 1; }
KEYDIR=${THUREA_KEYS:-$HOME/.config/thurea}
RELKEY=$KEYDIR/release-key.pem
EXTKEY=$KEYDIR/extension-key.pem
for k in "$RELKEY" "$EXTKEY"; do [ -f "$k" ] || { echo "missing key: $k"; exit 1; }; done
openssl pkey -in "$RELKEY" -pubout | cmp -s - "$R/release/release-key.pub.pem" || { echo "release key doesn't match release/release-key.pub.pem"; exit 1; }
B=$R/build/release; D=$R/dist
rm -rf "$B" "$D"; mkdir -p "$B/common" "$B/ext" "$D"
C=$B/common
echo "== Thurea $VER"
echo "$VER" > "$R/release/VERSION"
echo "$VER" > "$C/VERSION"
# One version everywhere: the Rust crates (`thurea version`) and the Omarchy plugin too.
sed -i -E "0,/^version = \"[0-9.]+\"/s//version = \"$VER\"/" "$R/Cargo.toml"
# The Omarchy plugin's manifest, and the copy at the repository root that lets
# `omarchy plugin add https://github.com/ShaddickDev/thurea.git` install it.
python3 - "$R/linux/omarchy-plugin/omarea/manifest.json" "$R/manifest.json" "$VER" <<'PY'
import json, sys
for p in sys.argv[1:3]:
    m = json.load(open(p)); m["version"] = sys.argv[3]
    json.dump(m, open(p, "w"), indent=2); open(p, "a").write("\n")
PY
cp "$R/CREDITS.md" "$C/CREDITS.md"
cp "$R/LICENSE" "$C/LICENSE"
cp "$R/extension/icons/thurea.svg" "$C/omarea.svg"

# ---- Extension ----
( cd "$R/extension" && tar --exclude='host' --exclude='firefox' --exclude='README.md' --exclude='bump-sw.sh' -cf - . ) | tar -xf - -C "$B/ext"
cp "$R/CREDITS.md" "$B/ext/CREDITS.md"
cp "$R/LICENSE" "$B/ext/LICENSE"
python3 - "$B/ext/manifest.json" "$VER" <<'PY'
import json, sys
p, v = sys.argv[1], sys.argv[2]
m = json.load(open(p)); m["version"] = v; m.pop("update_url", None)
m["update_url"] = "http://127.0.0.1:8137/updates.xml"
json.dump(m, open(p, "w"), indent=2, ensure_ascii=False)
PY
CHROMIUM=$(command -v chromium || echo /usr/lib/chromium/chromium)
"$CHROMIUM" --no-sandbox --headless=new --user-data-dir="$B/packprofile" --pack-extension="$B/ext" --pack-extension-key="$EXTKEY" >/dev/null 2>&1 || true
[ -s "$B/ext.crx" ] || { echo "packing the extension failed"; exit 1; }
cp "$B/ext.crx" "$C/thurea.crx"
# Extension ID from the key (Chromium: first 128 bits of SHA-256 of the public key, as a-p).
ID=$(openssl pkey -in "$EXTKEY" -pubout -outform DER 2>/dev/null | sha256sum | head -c 32 | tr '0-9a-f' 'a-p')
echo "$ID" > "$C/extension-id"
cat > "$C/updates.xml" <<XML
<?xml version='1.0' encoding='UTF-8'?>
<gupdate xmlns='http://www.google.com/update2/response' protocol='2.0'>
  <app appid='$ID'>
    <updatecheck codebase='http://127.0.0.1:8137/thurea.crx' version='$VER' />
  </app>
</gupdate>
XML
echo "   extension $ID"

# The Firefox-family build (Firefox, LibreWolf, Waterfox, Zen, Floorp…): same code, Firefox's
# manifest and background (release/firefox-ext.py). Firefox and its release-based forks only
# install add-ons signed by Mozilla, so with AMO API keys (AMO_JWT_ISSUER / AMO_JWT_SECRET, from
# https://addons.mozilla.org/developers/addon/api/key/) it is signed as an unlisted (self-hosted)
# add-on; without them it is built unsigned, which only LibreWolf, Firefox ESR / Developer
# Edition / Nightly and other builds that allow unsigned add-ons accept.
GECKO=$(python3 -I "$R/release/firefox-ext.py" "$B/ext" "$B/ext-firefox" "$VER")
echo "$GECKO" > "$C/firefox-id"
XPI_NAME="thurea-firefox-$VER.xpi"; echo "$XPI_NAME" > "$C/firefox-xpi-name"
python3 -I - "$B/ext-firefox" "$B/ext-firefox.xpi" <<'PY'
import os, sys, zipfile
src, out = sys.argv[1:3]
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for root, _, files in sorted(os.walk(src)):
        for f in sorted(files):
            p = os.path.join(root, f)
            z.write(p, os.path.relpath(p, src))
PY
FIREFOX_SIGNED=false
if [ -n "${AMO_JWT_ISSUER:-}" ] && [ -n "${AMO_JWT_SECRET:-}" ]; then
  rm -rf "$B/amo"; mkdir -p "$B/amo"
  npx --yes web-ext@8 sign --channel=unlisted --source-dir="$B/ext-firefox" --artifacts-dir="$B/amo" \
      --api-key="$AMO_JWT_ISSUER" --api-secret="$AMO_JWT_SECRET" > "$B/amo.log" 2>&1 || { tail -20 "$B/amo.log"; echo "signing the Firefox add-on failed"; exit 1; }
  cp "$(ls "$B"/amo/*.xpi | head -1)" "$C/thurea.xpi"; FIREFOX_SIGNED=true
  echo "   firefox $GECKO (signed by Mozilla)"
elif [ "${THUREA_REQUIRE_SIGNED:-0}" = 1 ]; then
  echo "THUREA_REQUIRE_SIGNED=1 but AMO_JWT_ISSUER / AMO_JWT_SECRET aren't set: Firefox won't install an unsigned add-on"; exit 1
else
  cp "$B/ext-firefox.xpi" "$C/thurea.xpi"
  echo "   firefox $GECKO (WARNING: unsigned; Firefox and Zen won't install it, set AMO_JWT_ISSUER / AMO_JWT_SECRET)"
fi

# ---- Shared block lists and policies ----
{ echo "# >>> thurea (Thurea blocker - managed by the system, do not edit)"
  awk '{print "0.0.0.0 " $1; if (split($1, p, ".") == 2) print "0.0.0.0 www." $1}' "$R/extension/filters/porn-domains.txt"
  for d in dns.google dns64.dns.google cloudflare-dns.com mozilla.cloudflare-dns.com chrome.cloudflare-dns.com \
           one.one.one.one 1dot1dot1dot1.cloudflare-dns.com security.cloudflare-dns.com dns.quad9.net dns9.quad9.net \
           dns10.quad9.net dns11.quad9.net dns12.quad9.net doh.opendns.com dns.opendns.com dns.adguard-dns.com \
           unfiltered.adguard-dns.com dns.adguard.com dns-unfiltered.adguard.com dns.nextdns.io doh.mullvad.net \
           dns.mullvad.net base.dns.mullvad.net adblock.dns.mullvad.net all.dns.mullvad.net dns.controld.com \
           freedns.controld.com doh.dns.sb dns.alidns.com doh.pub dns.twnic.tw common.dot.dns.yandex.net \
           doh.libredns.gr dns.switch.ch doh.ffmuc.net dns.digitale-gesellschaft.ch protective.joindns4.eu unfiltered.joindns4.eu; do
    echo "0.0.0.0 $d"; done
  echo "# <<< thurea"; } > "$C/hosts-block"
python3 - "$ID" "$C" "$R/extension/filters/porn-domains.txt" "$R/release/release-key.pub.pem" "$GECKO" "$XPI_NAME" <<'PY'
import base64, json, subprocess, sys
ID, out, lst, pub, GECKO, XPI = sys.argv[1:7]
pol = {
  "ExtensionInstallForcelist": [f"{ID};http://127.0.0.1:8137/updates.xml"],
  "ExtensionSettings": {
    ID: {"installation_mode": "force_installed", "update_url": "http://127.0.0.1:8137/updates.xml", "override_update_url": True},
    "cackkknkjnchcemedibohdnlejfgdben": {"installation_mode": "removed"},
    "jinjaccalgkegednnccohejagnlnfdag": {"installation_mode": "removed"}
  },
  "3rdparty": {"extensions": {ID: {"pornBlock": True}}},
  "ForceGoogleSafeSearch": True,
  "DnsOverHttpsMode": "secure",
  "DnsOverHttpsTemplates": "https://family.cloudflare-dns.com/dns-query",
  "IncognitoModeAvailability": 1,
  "BrowserGuestModeEnabled": False
}
json.dump(pol, open(f"{out}/chromium-policy.json", "w"), indent=2)
known = [l.strip() for l in open(lst) if l.strip()]
top = [d for d in known if d.count(".") == 1][:3000]
json.dump({"policies": {
  # The Thurea extension, force-installed from this computer's own update server. Its address
  # carries the version: Firefox installs again from install_url when the address changes.
  "ExtensionSettings": {GECKO: {"installation_mode": "force_installed", "install_url": f"http://127.0.0.1:8137/{XPI}"}},
  "3rdparty": {"Extensions": {GECKO: {"pornBlock": True}}},
  # Builds that allow unsigned add-ons (ESR, Developer Edition, Nightly, LibreWolf…) take an
  # unsigned release too; Firefox itself ignores this and needs the Mozilla-signed one.
  "Preferences": {"xpinstall.signatures.required": {"Value": False, "Status": "locked"}},
  "DNSOverHTTPS": {"Enabled": True, "ProviderURL": "https://family.cloudflare-dns.com/dns-query", "Locked": True, "Fallback": False},
  "DisablePrivateBrowsing": True,
  "WebsiteFilter": {"Block": [f"*://*.{d}/*" for d in top] + [f"*://{d}/*" for d in top]}
}}, open(f"{out}/firefox-policies.json", "w"))
# Release public key: PEM (Linux, openssl), modulus/exponent (Windows), SPKI (Android).
import shutil; shutil.copy(pub, f"{out}/release-key.pub.pem")
txt = subprocess.run(["openssl", "rsa", "-pubin", "-in", pub, "-noout", "-text"], capture_output=True, text=True, check=True).stdout
mod = "".join(l.strip() for l in txt.split("Modulus:")[1].split("Exponent:")[0].splitlines()).replace(":", "")
n = bytes.fromhex(mod).lstrip(b"\0")
e = int(txt.split("Exponent:")[1].split()[0])
json.dump({"n": base64.b64encode(n).decode(), "e": base64.b64encode(e.to_bytes((e.bit_length() + 7) // 8, "big")).decode()}, open(f"{out}/release-key.json", "w"))
der = subprocess.run(["openssl", "pkey", "-pubin", "-in", pub, "-outform", "DER"], capture_output=True, check=True).stdout
open(f"{out}/release-key.spki.b64", "w").write(base64.b64encode(der).decode())
PY
cp "$C/thurea.crx" "$D/thurea-extension.crx"
cp "$C/thurea.xpi" "$D/thurea-extension-firefox.xpi"

# ---- Android ----
CODE=$(awk -F. '{print $1*10000 + $2*100 + $3}' <<< "$VER")
sed -i -E "s/android:versionCode=\"[0-9]+\"/android:versionCode=\"$CODE\"/; s/android:versionName=\"[^\"]+\"/android:versionName=\"$VER\"/" "$R/android/AndroidManifest.xml"
cat > "$R/android/src/com/thurea/guard/ReleaseKey.java" <<JAVA
package com.thurea.guard;

/** Thurea's release public key (written by release/build.sh from release/release-key.pub.pem). */
final class ReleaseKey {
    static final String SPKI = "$(cat "$C/release-key.spki.b64")";
}
JAVA
echo "== Android ($VER, code $CODE)"
"$R/android/build.sh" > "$B/android.log" 2>&1 || { tail -20 "$B/android.log"; echo "Android build failed"; exit 1; }
cp "$R/android/Thurea.apk" "$D/thurea-android.apk"

# ---- Linux ----
echo "== Linux"
# Thurea's Rust agent (phone setup by QR, verse of the day, status) and the non-Omarchy tray.
( cd "$R" && cargo build --release -p thurea-agent -p thurea-tray ) > "$B/rust.log" 2>&1 || { tail -20 "$B/rust.log"; echo "Rust build failed"; exit 1; }
( cd "$R" && cargo test --release -p thurea-core -p thurea-agent -p thurea-tray ) >> "$B/rust.log" 2>&1 || { tail -30 "$B/rust.log"; echo "Rust tests failed"; exit 1; }
cp "$R/target/release/thurea" "$R/target/release/thurea-tray" "$C/"
cp "$D/thurea-android.apk" "$C/Thurea.apk"
rm -rf "$B/linux"; cp -r "$R/linux" "$B/linux"
sed -i "s/^pkgver=.*/pkgver=$VER/" "$B/linux/PKGBUILD" "$R/linux/PKGBUILD"
( cd "$B/linux" && THUREA_DATA=$C PKGDEST=$B/linux makepkg -f --nodeps >/dev/null 2>"$B/linux.log" ) || { cat "$B/linux.log"; exit 1; }
cp "$B"/linux/thurea-"$VER"-*.pkg.tar.zst "$D/thurea-linux.pkg.tar.zst"

# ---- Debian / Ubuntu (.deb) and Fedora (.rpm): the same files (linux/stage.sh) ----
need() { command -v "$1" >/dev/null || { echo "$1 is needed for the $2 package (Arch: sudo pacman -S $3)"; exit 1; }; }
need dpkg-deb .deb dpkg
need rpmbuild .rpm rpm-tools
echo "== Debian / Ubuntu"
DR=$B/deb-root; rm -rf "$DR"; "$R/linux/stage.sh" "$DR" "$C" deb
mkdir -p "$DR/DEBIAN"
cat > "$DR/DEBIAN/control" <<CTL
Package: thurea
Version: $VER
Architecture: amd64
Maintainer: Thurea <https://github.com/ShaddickDev/thurea>
Depends: systemd, e2fsprogs, python3, nftables, openssl, curl
Recommends: network-manager, adb, ffmpeg
Protected: yes
Section: admin
Priority: optional
Homepage: https://github.com/ShaddickDev/thurea
Description: Family protection: harmful-site and ad blocker, managed by the system
 Thurea: hosts block, family DNS, browser policies with the Thurea extension
 force-installed, DNS lockdown, household mesh with parents' controls,
 self-updating from signed GitHub releases.
CTL
install -m755 "$R/linux/debian/postinst" "$R/linux/debian/prerm" "$DR/DEBIAN/"
dpkg-deb --root-owner-group -Zxz --build "$DR" "$D/thurea-linux.deb" >/dev/null
echo "== Fedora"
RR=$B/rpm-root; rm -rf "$RR" "$B/rpmbuild"; "$R/linux/stage.sh" "$RR" "$C" rpm
mkdir -p "$B/rpmbuild"
# Every staged file and the directories Thurea owns (config files under /etc kept on upgrade).
( cd "$RR" && find . \( -type f -o -type l \) | sed 's|^\.||' | sort | while read -r f; do
    case $f in /etc/*) echo "%config(noreplace) $f" ;; *) echo "$f" ;; esac
  done
  echo "%dir /usr/lib/thurea"; echo "%dir /usr/share/thurea" ) > "$B/rpmbuild/files.list"
# _rpmformat 4: rpm 6 builds v6 packages by default, which libarchive (the private-feature check)
# can't read and older rpm releases may not install. Older rpm ignores the macro.
rpmbuild -bb --quiet --define "_rpmformat 4" --define "_topdir $B/rpmbuild" --define "stage $RR" --define "ver $VER" "$R/linux/rpm/thurea.spec" >/dev/null
cp "$B"/rpmbuild/RPMS/x86_64/thurea-"$VER"-1.x86_64.rpm "$D/thurea-linux.rpm"

# ---- Windows ----
echo "== Windows"
W=$B/windows/Thurea; mkdir -p "$W/data" "$W/web" "$W/android"
cp -r "$R/windows/thurea/." "$W/"
cp "$C/extension-id" "$C/firefox-id" "$C/chromium-policy.json" "$C/firefox-policies.json" "$C/release-key.json" "$W/data/"
cp "$R/policy/verses.json" "$W/data/verses.json"
# The Rust agent for Windows: the tray runs it for "Keep my phone" (setup without a reset).
( cd "$R" && PATH="$HOME/.cargo/bin:$PATH" cargo build --release -p thurea-agent --target x86_64-pc-windows-gnu ) >> "$B/rust.log" 2>&1 || { tail -20 "$B/rust.log"; echo "Windows agent build failed"; exit 1; }
cp "$R/target/x86_64-pc-windows-gnu/release/thurea.exe" "$W/thurea.exe"
# The tray app, built here so the release can ship it signed. Without mcs (mono), Thurea.ps1
# compiles it on each PC at install, and that copy is unsigned (Smart App Control may block it).
if command -v mcs >/dev/null; then
  mcs -nologo -target:winexe -optimize+ -out:"$W/ThureaTray.exe" -r:System.Windows.Forms.dll -r:System.Drawing.dll \
      -r:System.Web.Extensions.dll -r:System.Core.dll "$W"/tray/*.cs >> "$B/tray.log" 2>&1 || { cat "$B/tray.log"; echo "tray app build failed"; exit 1; }
else
  echo "   (no mcs: ThureaTray.exe will be compiled on each PC, unsigned)"
fi
cp "$R/CREDITS.md" "$W/CREDITS.md"
cp "$R/LICENSE" "$W/LICENSE"
cp "$C/thurea.crx" "$C/updates.xml" "$W/web/"
cp "$C/thurea.xpi" "$W/web/$XPI_NAME"
cp "$D/thurea-android.apk" "$W/android/Thurea.apk"
echo "$VER" > "$W/VERSION"
find "$W" \( -name "*.ps1" -o -name "*.cs" -o -name "*.cmd" -o -name "*.md" \) -print0 | xargs -0 sed -i "s/\r\?$/\r/"
# Authenticode: thurea.exe, ThureaTray.exe and Thurea.ps1 (after the CRLF pass: a signed script
# must not change afterwards). THUREA_SIGN_CMD signs one file in place, e.g. with jsign and a
# hardware token or Azure Trusted Signing (see docs/windows.md, "Code signing"). Unsigned releases
# still work, but Smart App Control, SmartScreen and Defender treat them with suspicion; set
# THUREA_REQUIRE_SIGNED=1 for a public release so an unsigned build fails instead.
WIN_SIGNED=("$W/thurea.exe" "$W/Thurea.ps1"); [ -f "$W/ThureaTray.exe" ] && WIN_SIGNED+=("$W/ThureaTray.exe")
if [ -n "${THUREA_SIGN_CMD:-}" ]; then
  for f in "${WIN_SIGNED[@]}"; do
    bash -c "$THUREA_SIGN_CMD \"\$1\"" sign "$f" >> "$B/sign.log" 2>&1 || { cat "$B/sign.log"; echo "signing $f failed"; exit 1; }
  done
  echo "   signed: ${WIN_SIGNED[*]##*/}"
elif [ "${THUREA_REQUIRE_SIGNED:-0}" = 1 ]; then
  echo "THUREA_REQUIRE_SIGNED=1 but THUREA_SIGN_CMD isn't set: refusing to build an unsigned Windows release"; exit 1
else
  echo "   WARNING: Windows files are NOT code-signed (set THUREA_SIGN_CMD)"
fi
( cd "$B/windows" && python3 - <<'PY'
import os, zipfile
with zipfile.ZipFile("../../../dist/thurea-windows.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for root, _, files in os.walk("Thurea"):
        for f in sorted(files):
            z.write(os.path.join(root, f))
PY
)

# ---- Manifest, signed ----
python3 - "$D" "$VER" <<'PY'
import hashlib, json, os, sys, time
d, v = sys.argv[1], sys.argv[2]
files = {}
for key, name in [("extension", "thurea-extension.crx"), ("extension-firefox", "thurea-extension-firefox.xpi"),
                  ("linux", "thurea-linux.pkg.tar.zst"),
                  ("linux-deb", "thurea-linux.deb"), ("linux-rpm", "thurea-linux.rpm"),
                  ("windows", "thurea-windows.zip"), ("android", "thurea-android.apk")]:
    data = open(os.path.join(d, name), "rb").read()
    files[key] = {"name": name, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
json.dump({"name": "Thurea", "version": v, "released": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "files": files},
          open(os.path.join(d, "thurea-manifest.json"), "w"), indent=2)
PY
openssl dgst -sha256 -sign "$RELKEY" -out "$D/thurea-manifest.json.sig" "$D/thurea-manifest.json"
openssl dgst -sha256 -verify "$R/release/release-key.pub.pem" -signature "$D/thurea-manifest.json.sig" "$D/thurea-manifest.json" >/dev/null
echo "== dist/ (Thurea $VER, manifest signed)"
ls -l "$D"
