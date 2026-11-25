#!/bin/bash
# Thurea's sealed test lab. Everything runs inside one unprivileged network namespace that
# has only a loopback interface: VMs and the Android emulator can reach each other through
# it, and nothing can reach your LAN or the internet (checked before anything starts).
#
#   lab/lab.sh up        boot Laptop + Desktop VMs (+ the Android emulator with --phone,
#                        or a freshly wiped, never-set-up one with --phone-new)
#   lab/lab.sh phone-pair   join the emulator to the household over adb (Thurea installed by adb)
#   lab/lab.sh phone-qr     set up the wiped emulator from `thurea setup-phone`'s QR code
#   lab/lab.sh ns CMD    run a command inside the sealed namespace
#   lab/lab.sh kill NAME power a VM off (to test that the others notice)
#   lab/lab.sh logs NAME
#   lab/lab.sh down      stop everything and remove the namespace
set -euo pipefail
L=$(cd "$(dirname "$0")" && pwd); O=$L/out; mkdir -p "$O"
PIDF=$O/ns.pid
ADB=~/Android/Sdk/platform-tools/adb; EMU=~/Android/Sdk/emulator/emulator

ns_pid() { [ -f "$PIDF" ] && kill -0 "$(cat "$PIDF")" 2>/dev/null && cat "$PIDF"; }
inns() { nsenter -t "$(ns_pid)" -U -n --preserve-credentials -- "$@"; }

sealed_check() {
  local routes links
  routes=$(inns ip route show; inns ip -6 route show | grep -v '^::1 ' || true)
  links=$(inns ip -o link show | grep -v ': lo:' || true)
  if [ -n "$routes" ] || [ -n "$links" ]; then echo "lab: namespace is NOT sealed — refusing to continue"; echo "$routes$links"; exit 1; fi
  if inns bash -c 'exec 3<>/dev/tcp/1.1.1.1/443' 2>/dev/null; then echo "lab: outside reachable — refusing"; exit 1; fi
  echo "lab: sealed (only loopback; 1.1.1.1 unreachable)"
}

ns_up() {
  if [ -z "$(ns_pid || true)" ]; then
    unshare -rn sleep infinity < /dev/null > /dev/null 2>&1 & echo $! > "$PIDF"; sleep 0.3
    inns ip link set lo up
  fi
  sealed_check
}

vm() { # name hostport peers [extra kernel args]
  local name=$1 port=$2 peers=$3 extra=${4:-}
  inns qemu-system-x86_64 -enable-kvm -cpu host -m 384 -smp 2 -display none -no-reboot -daemonize \
    -pidfile "$O/$name.pid" -kernel "$O/vmlinuz" -initrd "$O/initramfs.img" \
    -append "console=ttyS0 quiet panic=-1 thurea.name=$name thurea.key=$(cat "$O/key") thurea.peers=$peers $extra" \
    -netdev "user,id=n0,hostfwd=udp:127.0.0.1:$port-:47823" -device virtio-net-pci,netdev=n0 \
    -serial "file:$O/$name.log" < /dev/null > "$O/$name.qemu.log" 2>&1   # a daemon must not hold the caller's output open
  echo "lab: $name VM booted (mesh port $port in the namespace)"
}

phone_ready() {
  inns "$ADB" wait-for-device
  until [ "$(inns "$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 2; done
  # The emulator's simulated Wi-Fi sometimes doesn't connect in an offline namespace:
  # wait for its route to the lab gateway, toggling Wi-Fi if it doesn't come.
  for attempt in 1 2 3; do
    for _ in $(seq 20); do inns "$ADB" shell ip route get 10.0.2.2 2>&1 | grep -q 'dev wlan0' && return; sleep 2; done
    echo "lab: phone Wi-Fi not connected — toggling (attempt $attempt)"
    inns "$ADB" shell svc wifi disable; sleep 3; inns "$ADB" shell svc wifi enable
  done
}

tap_text() {  # taps the on-screen button with this exact text, if there is one
  local b
  inns "$ADB" shell uiautomator dump /data/local/tmp/ui.xml >/dev/null 2>&1 || return 1
  b=$(inns "$ADB" shell cat /data/local/tmp/ui.xml | grep -o "text=\"$1\"[^>]*bounds=\"\[[0-9]*,[0-9]*\]\[[0-9]*,[0-9]*\]\"" | head -1 | grep -o '\[[0-9,]*\]\[[0-9,]*\]' | tr '[],' '   ')
  [ -n "$b" ] || return 1
  set -- $b
  inns "$ADB" shell input tap $(( ($1 + $3) / 2 )) $(( ($2 + $4) / 2 ))
}

case "${1:-}" in
  up)
    [ -f "$O/initramfs.img" ] || "$L/build-image.sh"
    ns_up
    [ -f "$O/key" ] || od -An -tx1 -N32 /dev/urandom | tr -d ' \n' > "$O/key"
    vm Laptop 47901 10.0.2.2:47902,10.0.2.2:47903 "thurea.block_at=60:lab-test-site.example"
    vm Desktop 47902 10.0.2.2:47901,10.0.2.2:47903
    if [ "${2:-}" = --phone ] || [ "${2:-}" = --phone-new ]; then
      inns "$ADB" start-server >/dev/null 2>&1
      wipe=; [ "$2" = --phone-new ] && wipe=-wipe-data
      inns "$EMU" -avd thurea-test -read-only $wipe -no-snapshot-load -no-snapshot-save -no-window -no-audio -no-boot-anim -feature -Wifi \
        < /dev/null > "$O/phone-emulator.log" 2>&1 &
      echo "lab: Android emulator starting (read-only: nothing is saved to the AVD${wipe:+; wiped like a new phone})"
    fi
    ;;
  phone-pair)  # after the emulator has booted
    phone_ready
    inns "$ADB" emu redir add udp:47903:47823 >/dev/null
    inns "$ADB" install -r "$L/../android/Thurea.apk" | tail -1
    inns "$ADB" shell "am start -S -n com.thurea.guard/.Remote --es mesh_key '$(cat "$O/key")' --es mesh_name 'Phone' --es mesh_peers '10.0.2.2:47901,10.0.2.2:47902'" | tail -1
    ;;
  phone-qr)  # after `up --phone-new` has booted
    phone_ready
    inns "$ADB" emu redir add udp:47903:47823 >/dev/null
    inns "$ADB" root >/dev/null; inns "$ADB" wait-for-device
    phone_ready
    # A wiped emulator skips its setup screens; make it a phone still on its welcome screen
    # (Android's device-policy service caches "set up", so forget that and restart the framework).
    # The lab has no internet, so also skip Android's "is there internet?" probe.
    inns "$ADB" shell 'rm -f /data/system/device_policies.xml; settings put global device_provisioned 0; settings put secure user_setup_complete 0; settings put global captive_portal_mode 0; sync; sleep 3; stop; start'
    sleep 5; phone_ready
    for _ in $(seq 60); do inns "$ADB" shell dumpsys connectivity 2>/dev/null | grep -q VALIDATED && break
      [ $((_ % 15)) = 0 ] && inns "$ADB" shell 'svc wifi disable; sleep 3; svc wifi enable'; sleep 2; done
    inns "$ADB" shell dumpsys connectivity 2>/dev/null | grep -q VALIDATED || { echo "lab: phone Wi-Fi never came up"; exit 1; }
    kill "$(cat "$O/setup-phone.pid" 2>/dev/null)" 2>/dev/null || true; sleep 1
    # The household computer's view: the lab key, reached at the Laptop VM's port.
    H=$O/host; mkdir -p "$H/cfg" "$H/state"; chmod 700 "$H/cfg"
    printf '{"id":"labhost","name":"Lab computer","key":"%s","peers":["10.0.2.2:47902"],"port":47901,"lan_broadcast":false}\n' "$(cat "$O/key")" > "$H/cfg/config.json"
    rm -f "$O/qr.pgm" "$O/qr.json"
    [ -f "$O/scan.dex" ] || "$L/scan/build.sh"
    THUREA_CONFIG_DIR=$H/cfg THUREA_STATE_DIR=$H/state inns "$L/../target/debug/thurea" setup-phone --address 10.0.2.2 --name "QR phone" \
      --apk "$L/../android/Thurea.apk" --save "$O/qr.pgm" < /dev/null > "$O/setup-phone.log" 2>&1 &
    echo $! > "$O/setup-phone.pid"
    until [ -s "$O/qr.pgm" ]; do sleep 0.5; done
    zbarimg -q --raw "$O/qr.pgm" 2>/dev/null > "$O/qr.json"
    echo "lab: decoded the QR code from the picture ($(wc -c < "$O/qr.json") bytes)"
    inns "$ADB" push "$O/scan.dex" "$O/qr.json" /data/local/tmp/ >/dev/null
    inns "$ADB" shell CLASSPATH=/data/local/tmp/scan.dex app_process / Scan /data/local/tmp/qr.json
    # Android's own setup screens: tap through them as the person holding the phone would.
    for i in $(seq 90); do
      [ -n "$(inns "$ADB" shell dpm list-owners | grep DeviceOwner)" ] && break
      tap_text "Next" || tap_text "Accept &amp; continue" || true
      sleep 2
    done
    inns "$ADB" shell dpm list-owners | grep -q DeviceOwner && echo "lab: Thurea is the phone's device owner" || { echo "lab: provisioning did not finish"; exit 1; }
    inns "$ADB" shell CLASSPATH=/data/local/tmp/scan.dex app_process / Scan --finalize >/dev/null
    for _ in $(seq 60); do grep -aq "QR phone (android) joined" "$O/Laptop.log" && break; sleep 2; done
    grep -aq "QR phone (android) joined" "$O/Laptop.log" && echo "lab: the Laptop sees \"QR phone\" in the household" || { echo "lab: the phone never checked in with the Laptop"; exit 1; }
    ;;
  ns) shift; ns_pid >/dev/null || { echo "lab is down"; exit 1; }; inns "$@" ;;
  kill) kill "$(cat "$O/$2.pid")" && echo "lab: $2 powered off" ;;
  logs) cat "$O/$2.log" ;;
  check) sealed_check ;;
  down)
    for p in "$O"/*.pid; do [ "$p" = "$PIDF" ] || kill "$(cat "$p")" 2>/dev/null || true; done
    if [ -n "$(ns_pid || true)" ]; then inns "$ADB" emu kill >/dev/null 2>&1 || true; inns "$ADB" kill-server >/dev/null 2>&1 || true; kill "$(cat "$PIDF")"; fi
    rm -f "$O"/*.pid; echo "lab: down" ;;
  *) sed -n '2,12p' "$0"; exit 1 ;;
esac
