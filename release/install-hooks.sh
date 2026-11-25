#!/bin/bash
# Turns on "push to release" for this clone: git uses release/hooks (tracked in the repo).
#   release/install-hooks.sh          on
#   release/install-hooks.sh --off    off
set -euo pipefail
R=$(cd "$(dirname "$0")/.." && pwd)
if [ "${1:-}" = --off ]; then git -C "$R" config --unset core.hooksPath || true; echo "Push-to-release is off."; exit 0; fi
chmod +x "$R/release/hooks/"* "$R/release/auto-release.sh"
git -C "$R" config core.hooksPath release/hooks
echo "Publication checks are on. Pushing master/main builds, signs and publishes a Thurea release."
gh auth status >/dev/null 2>&1 || echo "Note: gh isn't signed in (gh auth login), so publishing will fail until it is."
git -C "$R" remote get-url origin >/dev/null 2>&1 || echo "Note: no 'origin' remote yet (git remote add origin <url>)."
