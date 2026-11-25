#!/bin/bash
# Build/test VM helper (QEMU/KVM). The VM has no internet: either no network at all, or with
# NET=isolated a virtual adapter on QEMU's restricted user network (nothing leaves this machine).
#   vm.sh start [cd.iso]      boot the VM (headless; VNC on 127.0.0.1:5907)
#   vm.sh stop                ACPI shutdown and wait
#   vm.sh mount | umount      the transfer disk (XFER) on this computer; only while the VM is off
#   vm.sh shot FILE.png       screenshot
#   vm.sh run 'COMMAND'       type COMMAND into Win+R (UK keyboard layout in the guest)
set -euo pipefail
VM=${VM_DIR:-$HOME/VMs/w11b}
CD=${2:-$HOME/Downloads/Win11.iso}
mon() { python3 - "$1" <<'E'
import socket, sys, time
s = socket.socket(socket.AF_UNIX); s.connect(__import__('os').path.expanduser('~/VMs/w11b/mon.sock'))
time.sleep(.2); s.recv(65536); s.send((sys.argv[1] + '\n').encode()); time.sleep(.3); s.close()
E
}
running() { [ -f "$VM/qemu.pid" ] && kill -0 "$(cat "$VM/qemu.pid")" 2>/dev/null; }
case "${1:-}" in
start)
    running && { echo "already running"; exit 0; }
    NETOPT="-nic none"; [ "${NET:-}" = isolated ] && NETOPT="-nic user,restrict=on,model=e1000e"; [ "${NET:-}" = isolated-fwd ] && NETOPT="-nic user,restrict=on,model=e1000e,guestfwd=tcp:10.0.2.100:80-tcp:127.0.0.1:8099"
    rm -f "$VM/mon.sock"
    setsid -f qemu-system-x86_64 -name w11b -machine q35,accel=kvm -cpu host -smp 8 -m 10G -rtc base=localtime \
      -drive file="$VM/win.qcow2",if=none,id=hd,cache=unsafe,discard=unmap -device ide-hd,drive=hd,bus=ide.0,bootindex=1 \
      -drive file="$CD",if=none,id=cd,media=cdrom,readonly=on -device ide-cd,drive=cd,bus=ide.1,bootindex=2 \
      -device qemu-xhci,id=xhci -drive file="$VM/xfer.img",if=none,id=xfer,format=raw -device usb-storage,bus=xhci.0,drive=xfer,removable=on \
      $NETOPT -display none -vnc 127.0.0.1:7 -monitor unix:"$VM/mon.sock",server,nowait -daemonize -pidfile "$VM/qemu.pid"
    echo "started";;
stop)
    running || { echo "not running"; exit 0; }
    mon system_powerdown
    for _ in $(seq 1 60); do running || { echo "stopped"; exit 0; }; sleep 3; done
    echo "still running"; exit 1;;
mount)
    running && { echo "VM is running: stop it first"; exit 1; }
    L=$(udisksctl loop-setup --no-user-interaction ${RO:+-r} -f "$VM/xfer.img" | grep -o '/dev/loop[0-9]*'); sleep 3
    M=$(lsblk -no MOUNTPOINT "${L}p1"); [ -n "$M" ] || { udisksctl mount --no-user-interaction -b "${L}p1" >/dev/null; M=$(lsblk -no MOUNTPOINT "${L}p1"); }
    echo "$M";;
umount)
    L=$(losetup -j "$VM/xfer.img" | cut -d: -f1); [ -n "$L" ] || exit 0
    sync; udisksctl unmount --no-user-interaction -b "${L}p1" >/dev/null 2>&1 || true; udisksctl loop-delete --no-user-interaction -b "$L";;
shot)
    mon "screendump $VM/s.ppm"; sleep 1; magick "$VM/s.ppm" "${2:?file}";;
run)
    python3 "$VM/typ.py" run "${2:?command}";;
*) sed -n 2,9p "$0"; exit 1;;
esac
