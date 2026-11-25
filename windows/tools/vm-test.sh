#!/bin/bash
# Runs test/test-vm.ps1 in the already-started VM and waits for the VM to power off.
# Signs in first if Windows is at the sign-in screen (password of the throwaway build VM).
set -uo pipefail
VM=$HOME/VMs/w11b
T=$(cd "$(dirname "$0")/.." && pwd)
send() { python3 - "$@" <<'E'
import socket, sys, time
def cmd(c):
    s = socket.socket(socket.AF_UNIX); s.connect(__import__('os').path.expanduser('~/VMs/w11b/mon.sock')); time.sleep(.2); s.recv(65536)
    s.send((c + '\n').encode()); time.sleep(.25); s.close()
for a in sys.argv[1:]:
    if a.startswith('key:'): cmd('sendkey ' + a[4:])
    else:
        for ch in a: cmd('sendkey ' + {'-': 'minus'}.get(ch, ch))
E
}
sleep "${BOOT_WAIT:-75}"
send key:ctrl; sleep 2; send 'build-vm-only' key:ret; sleep 30
python3 "$VM/typ.py" run "${RUN_CMD:-powershell -ep bypass -f D:\test\test-vm.ps1}"
until ! kill -0 "$(cat "$VM/qemu.pid")" 2>/dev/null; do sleep 10; done
echo "VM powered off at $(date +%T)"
M=$("$T/tools/vm.sh" mount)
tr -d '\r' < "$M/test/results.txt" | grep -v '^\*\*\*\|^$' | grep -v "^Start time\|^Username\|^RunAs\|^Configuration\|^Machine\|^Host Application\|^Process ID\|^PS\|^BuildVersion\|^CLRVersion\|^WSMan\|^PSRemoting\|^Serialization\|^Windows PowerShell transcript" > "$T/test/out/results.txt"
"$T/tools/vm.sh" umount
echo "results: $T/test/out/results.txt"
