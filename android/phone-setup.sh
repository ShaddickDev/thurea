#!/bin/bash
# Guided setup: installs Thurea and makes it the device owner, with your accounts signed out
# only for the moment Android requires. Never touches passwords: you remove and re-add the
# accounts on the phone; this script opens the right screens, waits, and does the rest.
# Runs without keyboard input (progress is printed); stops safely at any problem.
set -uo pipefail
A=$(cd "$(dirname "$0")" && pwd)
LOG=${THUREA_SETUP_LOG:-/dev/null}
say() { echo "[$(date +%H:%M:%S)] $*" | tee -a "$LOG"; }
sh() { adb shell "$@" </dev/null 2>/dev/null | tr -d '\r'; }   # </dev/null: adb must never eat the script's input
# Play Protect's "Verify apps over USB" can hold up (or block) installing Thurea over USB; it's
# switched off for this run only and put back as it was when the script ends.
usb_verify_off() {
  [ -n "${VERIFY_WAS:-}" ] && return
  VERIFY_WAS=$(sh settings get global verifier_verify_adb_installs); [ -n "$VERIFY_WAS" ] || VERIFY_WAS=null
  sh settings put global verifier_verify_adb_installs 0 >/dev/null
  trap 'usb_verify_restore' EXIT
}
usb_verify_restore() {
  [ -n "${VERIFY_WAS:-}" ] || return
  if [ "$VERIFY_WAS" = null ]; then sh settings delete global verifier_verify_adb_installs >/dev/null; else sh settings put global verifier_verify_adb_installs "$VERIFY_WAS" >/dev/null; fi
}
grant() {  # lets Thurea keep its YouTube ad blocker (an accessibility service) switched on
  sh dumpsys deviceidle whitelist +com.thurea.guard >/dev/null   # never put to sleep (Samsung "Sleeping apps" / Doze)
  sh appops set com.thurea.guard ACCESS_RESTRICTED_SETTINGS allow >/dev/null
  sh pm grant com.thurea.guard android.permission.WRITE_SECURE_SETTINGS && say "✓ YouTube ad blocker permission granted." || say "! Couldn't grant the YouTube ad blocker permission."
}
pin_apps() {  # ad-free YouTube + YouTube Music, pinned by Thurea (both at once)
  local a
  for a in youtube music; do
    [ -f "$A/pin/out/$a.properties" ] && continue
    say "Building the ad-free $a on this computer (it opens the download page if the genuine app is needed)…"
    "$A/pin/thurea-pin.sh" build "$a" </dev/null || say "! $a couldn't be built this time — run phone-setup.sh again later."
  done
  [ -f "$A/pin/out/youtube.properties" ] || [ -f "$A/pin/out/music.properties" ] || { say "! Nothing to pin yet."; return; }
  say "Pinning ad-free YouTube and YouTube Music…"
  "$A/pin/thurea-pin.sh" push </dev/null || say "! Pinning didn't finish — run phone-setup.sh again; Thurea keeps what's already pinned."
}
# Signing certificates (SHA-256) the projects publish in their READMEs — anything else is refused.
signer() { ~/Android/Sdk/build-tools/35.0.0/apksigner verify --print-certs "$1" 2>/dev/null | sed -n 's/^Signer #1 certificate SHA-256 digest: //p' | head -1; }
app_stores() {  # Droid-ify and Obtainium: the only apps allowed to install other apps
  local repo pkg cert f url
  usb_verify_off
  for pair in "Droid-ify/client com.looker.droidify ed8859c55af311162658b94af982b9f091dcd27628d4de3486d1217ebf3c9935" \
              "ImranR98/Obtainium dev.imranr.obtainium b353601f6a1d5fd6603ae2f50be80cf301367b86b6ab8b1f66243da96cd57362"; do
    read -r repo pkg cert <<<"$pair"
    if ! sh pm list packages | grep -qx "package:$pkg"; then
      url=$(curl -fsSL --max-time 20 "https://api.github.com/repos/$repo/releases/latest" | grep -oE '"browser_download_url": *"[^"]*/app-release\.apk"' | sed 's/.*"\(https[^"]*\)"/\1/')
      f="$A/pin/tools/$(basename "$repo")-latest.apk"
      if [ -n "$url" ] && curl -fsSL -o "$f" "$url" && [ "$(signer "$f")" != "$cert" ]; then
        say "! $(basename "$repo") download isn't signed by its developer's published key — not installed."; rm -f "$f"; continue; fi
      if [ -n "$url" ] && [ -s "$f" ] && adb install "$f" </dev/null >/dev/null; then say "✓ Installed $(basename "$repo") ($pkg)."
      else say "! Couldn't install $pkg — run phone-setup.sh again later."; continue; fi
    fi
    sh appops set "$pkg" REQUEST_INSTALL_PACKAGES allow && say "✓ $pkg may install apps."
  done
}
helper() { sh am start -f 0x10008000 -n com.thurea.guard/.Remote --es account_mode "$@" >/dev/null; }   # Thurea's account helper
accounts() { sh dumpsys account | grep -E '^\s+Account \{name=' | sed -E 's/^\s+Account \{name=([^,]*), type=([^}]*)\}.*/\2 \1/' | sort -u; }
signed_in() { accounts | grep -qxF "$1 $2"; }    # exact "type name" match

# ── 1. Checks (nothing is changed if any fail) ──
[ -s "$A/Thurea.apk" ] || { say "STOP: $A/Thurea.apk is missing — run build.sh first."; exit 1; }
install_thurea() {
  usb_verify_off
  local o; o=$(adb install -r "$A/Thurea.apk" </dev/null 2>&1) && return 0
  say "Installing Thurea failed: $(echo "$o" | tr -d '\r' | tail -1)"
  say "On a Samsung, check Settings → Security and privacy → Auto Blocker is OFF (it blocks installs over USB)."
  return 1
}
n=$(adb devices | grep -c "device$")
[ "$n" = 1 ] || { say "STOP: connect exactly one phone with USB debugging on, and tap Allow on the phone. Found: $n"; exit 1; }
say "Phone: $(sh getprop ro.product.manufacturer) $(sh getprop ro.product.model), Android $(sh getprop ro.build.version.release)"
[ "$(sh getprop ro.build.version.sdk)" -ge 29 ] || { say "STOP: Android 10 or newer is needed."; exit 1; }
if [ "${1:-}" != check ] && sh dpm list-owners | grep -q "Device Owner"; then
  if sh dpm list-owners | grep -q com.thurea.guard; then say "Thurea is already the device owner — updating it."; install_thurea || exit 1; grant; sh am start -n com.thurea.guard/.MainActivity >/dev/null; pin_apps; app_stores; say "Done."; exit 0; fi
  say "STOP: another app already manages this phone:"; sh dpm list-owners; exit 1
fi
users=$(sh pm list users | grep -c "UserInfo{")
if [ "$users" -gt 1 ] && [ "${1:-}" != check ]; then
  say "STOP: the phone has $users users/profiles (Secure Folder, Dual Messenger, a work profile or a guest)."
  sh pm list users | grep "UserInfo{" | sed 's/^/        /'
  say "Remove the extra ones (Settings → Security and privacy → Secure Folder → Uninstall, or Advanced features → Dual Messenger), then run this again."
  exit 1
fi

if sh dumpsys connectivity | grep -q 'VPN CONNECTED\|type: VPN\[.*CONNECTED'; then
  say "Note: a VPN is connected. Thurea pauses any VPN that doesn't use family DNS (1.1.1.3) for 90 seconds —"
  say "      disconnect it for setup, or set ProtonVPN's Custom DNS to 1.1.1.3 and 1.0.0.3 first."
fi

# Apps Thurea will hide as browsers (anything that opens every web address, minus the kept ones).
web_apps() {
  for s in http https; do sh cmd package query-activities --brief -a android.intent.action.VIEW -c android.intent.category.BROWSABLE -d "$s://example.com/"; done \
    | grep -oE '^ *[a-zA-Z0-9_.]+/' | tr -d ' /' | sort -u \
    | grep -vxE 'com\.android\.chrome|com\.google\.android\.(googlequicksearchbox|youtube|webview|gms)|tv\.twitch\.android\.app|ch\.protonvpn\.android|com\.android\.vending|com\.thurea\.guard|app\.revanced\.android\.gms|app\.morphe\.android\..*|com\.looker\.droidify|dev\.imranr\.obtainium(\.fdroid)?'
}

# ── Dry run: "phone-setup.sh check" shows what would happen and changes nothing ──
if [ "${1:-}" = check ]; then
  say "Users/profiles: $users (must be 1)"
  say "Accounts that would be signed out for a moment:"; accounts | sed 's/^/        /'
  say "Apps Thurea would hide (browsers):"; web_apps | sed 's/^/        /'
  say "VPN apps installed:"; sh pm list packages | sed 's/package://' | grep -iE 'vpn|wireguard|tunnel|proton' | sed 's/^/        /'
  say "Check only — nothing was changed."
  exit 0
fi

# ── 2. Install Thurea while you're still signed in ──
install_thurea || { say "STOP: nothing was changed."; exit 1; }
say "Thurea installed."
grant

# ── 3. Remember which accounts to put back (kept on this computer only) ──
before=$(accounts)
REMIND=$A/.accounts-to-readd
if [ -z "$before" ] && [ -s "$REMIND" ] && grep -q . "$REMIND"; then
  # An earlier run was interrupted after signing out: keep its list so those accounts still come back.
  say "Using the account list saved by the last (interrupted) run: $(awk '{print $2}' "$REMIND" | paste -sd, -)"
else
  echo "$before" > "$REMIND"; chmod 600 "$REMIND"
fi
if [ -n "$before" ]; then
  say "Accounts on the phone (Thurea signs these out for a moment — Android needs that once):"
  echo "$before" | while read -r type name; do say "   • $name   ($type)"; done
  helper remove
  say "Thurea is tapping Remove account for each one. If the phone asks for your PIN, pattern or"
  say "Samsung password, enter it yourself — Thurea never types those. Waiting (up to 20 minutes)…"
  for i in $(seq 1 400); do
    left=$(accounts)
    [ -z "$left" ] && break
    [ $((i % 10)) = 0 ] && say "   still signed in: $(echo "$left" | awk '{print $2}' | paste -sd, -)"
    sleep 3
  done
  [ -z "$(accounts)" ] || { say "STOP: accounts are still on the phone after 20 minutes. Nothing was changed; sign back in if you removed any."; exit 1; }
fi

# ── 4. Device owner, right away ──
say "No accounts left — making Thurea the device owner…"
owner=1
if out=$(adb shell dpm set-device-owner com.thurea.guard/.AdminReceiver </dev/null 2>&1); then
  say "✓ Thurea is the device owner."
else
  owner=0
  say "Device owner failed: $(echo "$out" | tr -d '\r' | tail -1)"
  say "Nothing is locked. Signing your accounts back in now; then run phone-setup.sh again."
fi

# ── 5. Back in: Thurea opens Google's sign-in and fills in the email; you do the password/YubiKey ──
sh am start -n com.thurea.guard/.MainActivity >/dev/null; sleep 2
if [ -s "$REMIND" ]; then
  apps_back=()
  while read -r type name <&3; do
    [ -n "$name" ] || continue
    signed_in "$type" "$name" && continue
    case $type in
      com.google)
        helper add --es email "$name"
        say "Signing back into $name: Thurea fills in the address and picks \"Use your passkey\" — touch or tap your YubiKey (and enter its PIN if asked)."
        for i in $(seq 1 200); do signed_in "$type" "$name" && break; sleep 3; done
        signed_in "$type" "$name" && say "   ✓ $name is back." || say "   ! $name isn't back yet — add it later in Settings → Accounts." ;;
      com.osp.app.signin)   # Samsung account
        sh am start -a android.settings.ADD_ACCOUNT_SETTINGS >/dev/null
        say "Add account is open: tap Samsung account and sign back in as $name with your Samsung password"
        say "(no YubiKey — Samsung may text you a 2-step code). Thurea doesn't touch this one. Waiting up to 10 minutes…"
        for i in $(seq 1 200); do signed_in "$type" "$name" && break; sleep 3; done
        signed_in "$type" "$name" && say "   ✓ Samsung account is back." || say "   ! Samsung account isn't back yet — Settings → Accounts and backup → Manage accounts." ;;
      *) apps_back+=("$type") ;;   # app accounts (WhatsApp, Signal, Telegram…) come back on their own
    esac
  done 3< "$REMIND"
  [ ${#apps_back[@]} -gt 0 ] && say "Open these apps and sign back in if they ask (WhatsApp and Meet reconnect by themselves): $(printf '%s\n' "${apps_back[@]}" | sort -u | paste -sd, -)"
fi
helper off

# ── 6. Ad-free YouTube and YouTube Music, pinned (Thurea keeps a backup on the phone for repairs) ──
[ "$owner" = 1 ] || { say "STOP: Thurea isn't in charge of the phone yet (see above). Fix that, then run phone-setup.sh again."; exit 1; }
: > "$REMIND"
pin_apps; app_stores
say "Done. Thurea is in charge; your accounts are back. Pinned YouTube signs in separately inside MicroG-RE (email + YubiKey)."
