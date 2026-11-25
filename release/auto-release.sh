#!/bin/bash
# Release from git: builds the given commit in a clean checkout, signs it with Thurea's release
# key and publishes it, so every installed Thurea (Windows, Linux, Android, the extension)
# updates itself within the hour. Run by the pre-push hook (release/hooks/pre-push) when the
# main branch is pushed; it can also be run by hand.
#
#   release/auto-release.sh [COMMIT]       (default: HEAD)
#   release/auto-release.sh --dry-run ...  build and sign, but don't publish or tag
#
# The version is the last release's, plus one in the last place (1.0.0 → 1.0.1); put a
# different one in release/VERSION in the commit to jump (e.g. 1.1.0). A commit that's already
# released, or that only changes documentation, is skipped.
#
# Built from the commit itself, never from uncommitted files. The keys stay on this computer
# (~/.config/thurea, the Android keystore): nothing secret goes to GitHub.
set -euo pipefail
R=$(cd "$(dirname "$0")/.." && pwd)
DRY=0; [ "${1:-}" = --dry-run ] && { DRY=1; shift; }
SHA=$(git -C "$R" rev-parse "${1:-HEAD}^{commit}")
LOGDIR=${XDG_STATE_HOME:-$HOME/.local/state}/thurea-release; mkdir -p "$LOGDIR"
say() { echo "[$(date +%T)] $*"; }
notify() { command -v notify-send >/dev/null && notify-send -a Thurea "Thurea release" "$1" || true; }

# Already released?
if git -C "$R" tag --points-at "$SHA" | grep -q '^v[0-9]'; then say "$(git -C "$R" tag --points-at "$SHA" | head -1) is already this commit: nothing to do."; exit 0; fi

# Anything that ships changed since the last release?
LAST=$(git -C "$R" tag --list 'v[0-9]*' --sort=-v:refname | head -1 || true)
if [ -n "$LAST" ] && git -C "$R" diff --quiet "$LAST" "$SHA" -- . ':(exclude)*.md' ':(exclude)lab' ':(exclude)windows/test' ':(exclude)windows/tools'; then
  say "Only documentation/tests changed since $LAST: no release."; exit 0
fi

python3 "$R/release/check-public.py" --commits "$SHA"

# Version: the commit's release/VERSION if it's newer than every release so far, else the newest
# release + 0.0.1. "Released so far" = git tags, GitHub's latest release, and local dist-X.Y.Z builds.
ver_gt() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$1" ] && [ "$1" != "$2" ]; }
IN_COMMIT=$(git -C "$R" show "$SHA:release/VERSION" | tr -d '[:space:]')
PUBLISHED=$(gh release view --repo "${THUREA_REPO:-ShaddickDev/thurea}" --json tagName -q .tagName 2>/dev/null || true)
BASEV=$( { echo "${LAST#v}"; echo "${PUBLISHED#v}"; ls -d "$R"/dist-[0-9]* 2>/dev/null | sed 's/.*dist-//'; } | { grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' || true; } | sort -V | tail -1 || true)
if [ -z "$BASEV" ] || ver_gt "$IN_COMMIT" "$BASEV"; then VER=$IN_COMMIT
else IFS=. read -r a b c <<< "$BASEV"; VER="$a.$b.$((c + 1))"; fi
LOG=$LOGDIR/v$VER.log
say "Releasing $(git -C "$R" log -1 --format='%h %s' "$SHA") as Thurea $VER (log: $LOG)"

# A clean checkout of exactly that commit, sharing the Rust build cache.
WT=$(mktemp -d "${TMPDIR:-/tmp}/thurea-release.XXXXXX")
cleanup() { git -C "$R" worktree remove --force "$WT" >/dev/null 2>&1 || rm -rf "$WT"; }
trap cleanup EXIT
git -C "$R" worktree add --detach "$WT" "$SHA" >/dev/null 2>&1
mkdir -p "$R/target"; ln -s "$R/target" "$WT/target"

if ! "$WT/release/build.sh" "$VER" > "$LOG" 2>&1; then
  tail -15 "$LOG"; notify "Thurea $VER failed to build (see $LOG)"; say "Build failed: nothing published."; exit 1
fi
mkdir -p "$R/dist-$VER"; cp "$WT"/dist/* "$R/dist-$VER/"
say "Built and signed: $R/dist-$VER"
if [ $DRY = 1 ]; then say "Dry run: not published."; exit 0; fi

if ! "$WT/release/publish.sh" >> "$LOG" 2>&1; then
  tail -5 "$LOG"; notify "Thurea $VER built but couldn't be published (gh auth login?)"; say "Publishing failed (is gh signed in? gh auth login)."; exit 1
fi
git -C "$R" tag -a "v$VER" "$SHA" -m "Thurea $VER"
git -C "$R" push --quiet "${THUREA_REMOTE:-origin}" "v$VER" 2>/dev/null || true
notify "Thurea $VER is published. Every device updates within the hour."
say "Published Thurea $VER. Installed copies pick it up within the hour."
