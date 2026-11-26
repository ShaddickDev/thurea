#!/bin/bash
# The release job of .github/workflows/release.yml, run as the "builder" user.
#
#   release/ci/run.sh release   keys from the "release" environment's secrets → release/build.sh
#                               → release/publish.sh (GitHub release vVERSION, marked latest)
#   release/ci/run.sh test      the same build with throwaway keys made here; nothing is published
#
# The version is release/VERSION of the commit being built. A tag push must match it, and a
# version that already has a GitHub release is refused, so a run can never replace a release.
set -euo pipefail
MODE=${1:?usage: run.sh release|test}
R=$(cd "$(dirname "$0")/../.." && pwd)
export PATH=$HOME/.cargo/bin:$PATH
K=$HOME/.config/thurea
umask 077; mkdir -p "$K"
VER=$(tr -d '[:space:]' < "$R/release/VERSION")
REPO=${GITHUB_REPOSITORY:-ShaddickDev/thurea}

if [ "$MODE" = test ]; then
  echo "== Test build of $VER with throwaway keys (nothing is published)"
  unset AMO_JWT_ISSUER AMO_JWT_SECRET THUREA_SIGN_CMD
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:3072 -out "$K/release-key.pem" 2>/dev/null
  openssl pkey -in "$K/release-key.pem" -pubout -out "$R/release/release-key.pub.pem"
  openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out "$K/extension-key.pem" 2>/dev/null
  openssl rand -hex 24 > "$K/android-keystore-pass"
  keytool -genkeypair -noprompt -keystore "$K/android-release.jks" -storetype PKCS12 \
    -storepass:file "$K/android-keystore-pass" -alias thurea -keyalg RSA -keysize 2048 -validity 2 -dname CN=thurea-ci-test
  umask 022  # the keys above stay private; the packages need normal permissions
  "$R/release/build.sh" "$VER"
  D=$R/dist
  python3 -I "$R/release/check-public.py" --assets "$D"/thurea-*.{crx,xpi,zst,deb,rpm,zip,apk}
  echo "== Test build OK: $(ls "$D" | tr '\n' ' ')"
  exit 0
fi

[ "$MODE" = release ] || { echo "usage: run.sh release|test"; exit 1; }
case "${GITHUB_REF:-}" in
  refs/heads/master) ;;
  refs/tags/v*) [ "${GITHUB_REF#refs/tags/}" = "v$VER" ] || { echo "tag ${GITHUB_REF#refs/tags/} doesn't match release/VERSION ($VER)"; exit 1; } ;;
  *) echo "releases are made only from master or a vX.Y.Z tag (this is ${GITHUB_REF:-unknown})"; exit 1 ;;
esac
if gh release view "v$VER" --repo "$REPO" >/dev/null 2>&1; then
  echo "v$VER is already released: raise release/VERSION in a new commit, then run again."; exit 1
fi

need() { [ -n "${!1:-}" ] || { echo "the secret $1 isn't set in the \"release\" environment"; exit 1; }; }
need THUREA_RELEASE_KEY; need THUREA_EXTENSION_KEY; need THUREA_ANDROID_KEYSTORE; need THUREA_ANDROID_KEYSTORE_PASS
printf '%s\n' "$THUREA_RELEASE_KEY" > "$K/release-key.pem"
printf '%s\n' "$THUREA_EXTENSION_KEY" > "$K/extension-key.pem"
printf '%s' "$THUREA_ANDROID_KEYSTORE" | tr -d '[:space:]' | base64 -d > "$K/android-release.jks"
printf '%s\n' "$THUREA_ANDROID_KEYSTORE_PASS" | head -1 > "$K/android-keystore-pass"
if [ -n "${THUREA_WINDOWS_PFX:-}" ]; then
  need THUREA_WINDOWS_PFX_PASS
  printf '%s' "$THUREA_WINDOWS_PFX" | tr -d '[:space:]' | base64 -d > "$K/windows-signing.pfx"
  printf '%s\n' "$THUREA_WINDOWS_PFX_PASS" | head -1 > "$K/windows-signing-pass"
  export THUREA_SIGN_CMD="$R/release/ci/sign-windows.sh"
fi
unset THUREA_RELEASE_KEY THUREA_EXTENSION_KEY THUREA_ANDROID_KEYSTORE THUREA_ANDROID_KEYSTORE_PASS THUREA_WINDOWS_PFX THUREA_WINDOWS_PFX_PASS
[ "${THUREA_REQUIRE_SIGNED:-}" = 1 ] || unset THUREA_REQUIRE_SIGNED

echo "== Releasing $(git -C "$R" log -1 --format='%h %s') as Thurea $VER"
umask 022
"$R/release/build.sh" "$VER"
THUREA_REPO=$REPO THUREA_TARGET=${GITHUB_SHA:-} "$R/release/publish.sh"
echo "== Published Thurea $VER: https://github.com/$REPO/releases/tag/v$VER"
