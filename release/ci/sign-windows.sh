#!/bin/bash
# THUREA_SIGN_CMD for release/ci/run.sh: Authenticode-signs one file (.exe or .ps1) in place with
# osslsigncode, using the code-signing certificate in ~/.config/thurea/windows-signing.pfx.
set -euo pipefail
f=${1:?file to sign}; K=$HOME/.config/thurea
osslsigncode sign -pkcs12 "$K/windows-signing.pfx" -readpass "$K/windows-signing-pass" -h sha256 \
  -n Thurea -i https://github.com/ShaddickDev/thurea -ts http://timestamp.digicert.com \
  -in "$f" -out "$f.signed"
mv "$f.signed" "$f"
