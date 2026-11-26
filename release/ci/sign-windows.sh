#!/bin/bash
# THUREA_SIGN_CMD for release/ci/run.sh: Authenticode-signs one file (.exe or .ps1) in place with
# jsign, using the code-signing certificate in ~/.config/thurea/windows-signing.pfx.
set -euo pipefail
f=${1:?file to sign}; K=$HOME/.config/thurea
jsign --storetype PKCS12 --keystore "$K/windows-signing.pfx" --storepass "$(head -1 "$K/windows-signing-pass")" \
  --name Thurea --url https://github.com/ShaddickDev/thurea \
  --tsaurl http://timestamp.digicert.com --tsmode RFC3161 "$f"
