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
# The extension's family helper, for every Chromium-family browser.
host=$(printf '{"name":"com.thurea.family","description":"Thurea family helper","path":"/usr/lib/thurea/thurea-family-host","type":"stdio","allowed_origins":["chrome-extension://%s/"]}' "$(cat "$DATA/extension-id")")
for d in etc/chromium/native-messaging-hosts etc/opt/chrome/native-messaging-hosts etc/opt/edge/native-messaging-hosts etc/brave/native-messaging-hosts; do
  install -dm755 "$D/$d"
  printf '%s\n' "$host" > "$D/$d/com.thurea.family.json"
  chmod 644 "$D/$d/com.thurea.family.json"
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
