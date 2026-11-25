#!/bin/bash
# Makes the setup QR code for a new (or factory-reset) phone, for Thurea hosted on an HTTPS server.
# Easier, on a household computer: `sudo thurea setup-phone` — it serves Thurea itself and the
# phone joins the household too.
#
#   ./make-qr.sh https://your.host/some-long-random-path/Thurea.apk
#
# 1. Upload Thurea.apk to that address yourself (any HTTPS host). The phone checks Thurea's
#    signing key before installing, so a swapped or tampered file is refused.
# 2. On the new phone's first "Welcome" screen, tap the same spot 6 times → it opens a QR
#    scanner (it may ask for Wi-Fi first) → scan out/thurea-setup-qr.png.
# 3. The phone downloads Thurea, makes it the device owner and Thurea's protection is on
#    before you reach the home screen. Then finish in Thurea → Finish setup.
#
# The QR holds only the download address and Thurea's key fingerprint: no Wi-Fi password,
# no accounts, nothing about you. There is no server: Thurea runs only on the phone.
set -euo pipefail
A=$(cd "$(dirname "$0")" && pwd)
URL=${1:-}
[[ $URL =~ ^https:// ]] || { echo "Usage: $0 https://…/Thurea.apk  (an HTTPS address you will upload Thurea.apk to)"; exit 1; }
APK=$A/Thurea.apk
[ -s "$APK" ] || { echo "Thurea.apk missing — run build.sh first."; exit 1; }
BT=~/Android/Sdk/build-tools/35.0.0
HEX=$("$BT/apksigner" verify --print-certs "$APK" | sed -n 's/^Signer #1 certificate SHA-256 digest: //p')
[ -n "$HEX" ] || { echo "Couldn't read Thurea.apk's signature."; exit 1; }
SUM=$(python3 -c 'import base64,sys;print(base64.urlsafe_b64encode(bytes.fromhex(sys.argv[1])).decode().rstrip("="))' "$HEX")
mkdir -p "$A/out"
JSON=$(python3 - "$URL" "$SUM" <<'PY'
import json, sys
print(json.dumps({
  "android.app.extra.PROVISIONING_DEVICE_ADMIN_COMPONENT_NAME": "com.thurea.guard/.AdminReceiver",
  "android.app.extra.PROVISIONING_DEVICE_ADMIN_PACKAGE_DOWNLOAD_LOCATION": sys.argv[1],
  "android.app.extra.PROVISIONING_DEVICE_ADMIN_SIGNATURE_CHECKSUM": sys.argv[2],
  "android.app.extra.PROVISIONING_LEAVE_ALL_SYSTEM_APPS_ENABLED": True,
  "android.app.extra.PROVISIONING_SKIP_EDUCATION_SCREENS": True,
}, separators=(",", ":")))
PY
)
echo "$JSON" > "$A/out/thurea-setup-qr.json"
qrencode -l M -s 10 -m 4 -o "$A/out/thurea-setup-qr.png" "$JSON"
qrencode -l M -t ansiutf8 "$JSON"
echo
echo "QR code: $A/out/thurea-setup-qr.png"
echo "Upload:  $APK  →  $URL"
echo "Check after uploading:  curl -sI '$URL' | head -1   (must be 200)"
echo "Every new Thurea.apk (same key) works with this same QR — just upload it to the same address."
