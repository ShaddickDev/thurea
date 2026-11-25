#!/bin/bash
# The household scenario, start to finish, inside the sealed lab (see lab.sh):
#  1. Laptop + Desktop VMs and the Android phone join one household
#  2. every device enforces the family policy
#  3. the Laptop blocks a site at 60 s → the Desktop and the phone enforce it too
#  4. the Desktop is powered off → the Laptop and the phone both raise the alarm
set -uo pipefail
L=$(cd "$(dirname "$0")" && pwd); O=$L/out
ADB="$L/lab.sh ns $HOME/Android/Sdk/platform-tools/adb"
pass=0; fail=0
ok() { echo "PASS  $1"; pass=$((pass+1)); }
bad() { echo "FAIL  $1"; fail=$((fail+1)); }
last() { awk "/=== $1 /{b=\"\"} {b=b\"\n\"\$0} END{print b}" "$O/$1.log"; }
wait_for() { # seconds description command...
  local t=$1 d=$2; shift 2
  for _ in $(seq $((t / 3))); do "$@" >/dev/null 2>&1 && { ok "$d"; return 0; }; sleep 3; done
  bad "$d"; return 1
}
phone_notes() { $ADB shell dumpsys notification --noredact 2>/dev/null | grep -oE 'android.text=String \(.*\)' ; }

"$L/lab.sh" down >/dev/null 2>&1
rm -f "$O/key" "$O"/*.log
"$L/lab.sh" up --phone || exit 1
for _ in $(seq 60); do grep -q 'Boot completed' "$O/phone-emulator.log" 2>/dev/null && break; sleep 3; done
"$L/lab.sh" phone-pair >/dev/null

wait_for 150 "Laptop sees Desktop and Phone" bash -c "last(){ awk \"/=== \$1 /{b=\\\"\\\"} {b=b\\\"\\n\\\"\\\$0} END{print b}\" $O/\$1.log; }; last Laptop | grep -q '✓ Desktop' && last Laptop | grep -q '✓ Phone'"
wait_for 60 "Laptop enforces everything" bash -c "awk '/=== Laptop /{b=\"\"} {b=b\"\n\"\$0} END{print b}' $O/Laptop.log | grep -q 'Thurea is run by' && ! awk '/=== Laptop /{b=\"\"} {b=b\"\n\"\$0} END{print b}' $O/Laptop.log | grep -qE '^[!?] '"
wait_for 120 "Desktop adopted the Laptop's block (policy v2, hosts file)" bash -c "grep -q 'Family policy updated to version 2' $O/Desktop.log && tail -5 $O/Desktop.log | grep -q 'lab-test-site: [1-9]'"
wait_for 150 "Phone adopted policy v2" bash -c "$ADB shell dumpsys notification --noredact | grep -q 'Family policy updated to version 2'"
wait_for 90 "Phone sees all 3 devices protected" bash -c "$ADB shell dumpsys notification --noredact | grep -q '3 of 3 devices protected'"

"$L/lab.sh" kill Desktop >/dev/null; echo "....  Desktop powered off at $(date +%T)"
wait_for 200 "Laptop warns the Desktop went quiet" grep -q 'Desktop has not checked in' "$O/Laptop.log"
wait_for 60 "Phone warns the Desktop went quiet" bash -c "$ADB shell dumpsys notification --noredact | grep -q 'Desktop has not checked in'"

"$L/lab.sh" check
echo; echo "$pass passed, $fail failed"
[ $fail -eq 0 ]
