#!/usr/bin/env bash
# Runs every test that uses Bend (HVM) and fails if Bend is missing instead of skipping.
#   bend/test.sh            # Bend-backed tests only
#   bend/test.sh --all      # the whole workspace, with Bend required
# Install Bend first: cargo install hvm bend-lang (needs a C compiler for run-c).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
command -v bend >/dev/null || { echo "bend not found: cargo install hvm bend-lang" >&2; exit 1; }
export THUREA_REQUIRE_BEND=1
if [ "${1:-}" = "--all" ]; then
  cargo test --workspace
else
  cargo test -p thurea-core --lib bend::
  cargo test -p thurea-core --test bend_props -- --nocapture
fi
