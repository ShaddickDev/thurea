#!/bin/bash
# Lays out Thurea for Linux under DESTDIR: the same files for every distribution, plus what each
# package manager needs to keep Thurea installed and re-applied.
#
#   stage.sh DESTDIR DATA arch|deb|rpm
#
# DATA: the shared release data (agent, tray, extension, block lists, policies, release key) that
# release/build.sh prepares. Used by the PKGBUILD (Arch), and by build.sh for .deb and .rpm.
set -euo pipefail
D=${1:?DESTDIR} DATA=${2:?DATA} FLAVOR=${3:?arch, deb or rpm}
L=$(cd "$(dirname "$0")" && pwd)
F=$L/files

inst() { install -Dm"$1" "$2" "$D/$3"; }

inst 755 "$F/thurea-apply"       usr/lib/thurea/thurea-apply
inst 755 "$F/thurea-update"      usr/lib/thurea/thurea-update
inst 755 "$F/thurea-omarchy"     usr/lib/thurea/thurea-omarchy
inst 755 "$F/thurea-family-host" usr/lib/thurea/thurea-family-host
inst 644 "$F/thurea-tray.desktop" etc/xdg/autostart/thurea-tray.desktop
install -dm755 "$D/usr/bin"
ln -sf /usr/lib/thurea/thurea-update  "$D/usr/bin/thurea-update"
ln -sf /usr/lib/thurea/thurea-omarchy "$D/usr/bin/thurea-omarchy"
for u in thurea.service thurea.timer thurea.path thurea-extensions.service thurea-update.service thurea-update.timer thurea-mesh.service; do
  inst 644 "$F/$u" "usr/lib/systemd/system/$u"
done
for f in resolved.conf networkmanager-dns.conf apt-deny-browsers dnf-deny-browsers.conf thurea.nft README; do
  inst 644 "$F/$f" "usr/share/thurea/$f"
done
# Omarchy first: Thurea's bar plugin (linked into every Omarchy user's plugins by thurea-omarchy).
install -dm755 "$D/usr/share/thurea"
cp -r "$L/omarchy-plugin/thurea" "$D/usr/share/thurea/omarchy-plugin"
find "$D/usr/share/thurea/omarchy-plugin" -type d -exec chmod 755 {} + -o -type f -exec chmod 644 {} +

# The Rust agent and tray, the Android app the phone setup serves, the shared data.
inst 755 "$DATA/thurea"      usr/bin/thurea
inst 755 "$DATA/thurea-tray" usr/bin/thurea-tray
inst 644 "$DATA/Thurea.apk"  usr/share/thurea/Thurea.apk
inst 644 "$DATA/CREDITS.md"  usr/share/thurea/CREDITS.md
inst 644 "$DATA/CREDITS.md"  usr/share/licenses/thurea/CREDITS.md
for f in thurea.crx updates.xml hosts-block chromium-policy.json firefox-policies.json extension-id release-key.pub.pem VERSION; do
  inst 644 "$DATA/$f" "usr/share/thurea/$f"
done
inst 644 "$DATA/thurea.crx"  usr/share/thurea/web/thurea.crx
inst 644 "$DATA/updates.xml" usr/share/thurea/web/updates.xml
# The Firefox-family build (named by version: Firefox reinstalls when the address changes).
inst 644 "$DATA/thurea.xpi"  "usr/share/thurea/web/$(cat "$DATA/firefox-xpi-name")"
inst 644 "$DATA/firefox-id"  usr/share/thurea/firefox-id
inst 755 "$F/thurea-video-host" usr/lib/thurea/thurea-video-host
# The extension's helpers: family (com.thurea.family) and video downloads (com.thurea.ytdlp),
# for every Chromium-family browser (allowed_origins) and every Firefox-family one
# (allowed_extensions; Firefox reads /usr/lib/mozilla, and /usr/lib64/mozilla on Fedora; some forks
# their own). Not on Arch: /usr/lib64 is a link to /usr/lib there.
EXT=$(cat "$DATA/extension-id") GECKO=$(cat "$DATA/firefox-id")
host() { # host NAME DESCRIPTION PATH chromium|firefox
  if [ "$4" = chromium ]; then allow="\"allowed_origins\":[\"chrome-extension://$EXT/\"]"; else allow="\"allowed_extensions\":[\"$GECKO\"]"; fi
  printf '{"name":"%s","description":"%s","path":"%s","type":"stdio",%s}\n' "$1" "$2" "$3" "$allow"
}
for d in etc/chromium/native-messaging-hosts etc/opt/chrome/native-messaging-hosts etc/opt/edge/native-messaging-hosts \
         etc/brave/native-messaging-hosts etc/opt/brave.com/brave/native-messaging-hosts etc/opt/vivaldi/native-messaging-hosts \
         etc/vivaldi/native-messaging-hosts etc/opt/chromium/native-messaging-hosts etc/chromium-browser/native-messaging-hosts \
         etc/thorium/native-messaging-hosts etc/opt/thorium/native-messaging-hosts \
         usr/lib/mozilla/native-messaging-hosts usr/lib/librewolf/native-messaging-hosts \
         usr/lib/waterfox/native-messaging-hosts usr/lib/zen-browser/native-messaging-hosts usr/lib/floorp/native-messaging-hosts \
         $([ "$FLAVOR" = rpm ] && echo usr/lib64/mozilla/native-messaging-hosts); do
  kind=chromium; case $d in usr/*) kind=firefox ;; esac
  install -dm755 "$D/$d"
  host com.thurea.family "Thurea family helper" /usr/lib/thurea/thurea-family-host $kind > "$D/$d/com.thurea.family.json"
  host com.thurea.ytdlp "Thurea video downloads" /usr/lib/thurea/thurea-video-host $kind > "$D/$d/com.thurea.ytdlp.json"
  chmod 644 "$D/$d"/com.thurea.*.json
done

case $FLAVOR in
  arch)
    # pacman: refuse removal, refuse bypass browsers, re-apply after every transaction.
    inst 755 "$F/deny-remove"  usr/lib/thurea/deny-remove
    inst 755 "$F/deny-install" usr/lib/thurea/deny-install
    for h in "$F"/hooks/*.hook; do inst 644 "$h" "usr/share/libalpm/hooks/$(basename "$h")"; done
    ;;
  deb)
    # apt: re-apply after every dpkg run. (Removal: Protected: yes + the prerm script;
    # bypass browsers: an apt pin that thurea-apply keeps in place.)
    install -dm755 "$D/etc/apt/apt.conf.d"
    printf '%s\n' '// Thurea: put the family protection back after every package change.' \
      'DPkg::Post-Invoke { "if [ -x /usr/lib/thurea/thurea-apply ]; then /usr/lib/thurea/thurea-apply >/dev/null 2>&1 || true; fi"; };' \
      > "$D/etc/apt/apt.conf.d/90thurea"
    ;;
  rpm)
    # dnf refuses to remove protected packages (and the %preun script refuses rpm -e).
    install -dm755 "$D/etc/dnf/protected.d"
    echo thurea > "$D/etc/dnf/protected.d/thurea.conf"
    ;;
  *) echo "unknown flavor $FLAVOR" >&2; exit 2 ;;
esac
