#!/bin/bash
# Downloads the third-party pieces android/build.sh puts into Thurea, each checked against a
# pinned SHA-256, into ~/.cache/thurea-android (override with THUREA_DEPS). Prints that folder.
#
# - ARSCLib (REAndroid, Apache-2.0): merges Twitch's split APKs (APKMirror bundles) into one APK
#   before patching.
# - aapt2 from ReVanced Manager's APK (AOSP, Apache-2.0, built for Android by ReVanced): the
#   patcher runs it to rebuild resources. Android only runs programs from an app's own native
#   library folder, so Thurea has to carry it; the patcher itself is loaded on the phone from
#   Manager's ReVanced-signed APK (Patcher.java). Manager is never installed.
set -euo pipefail
D=${THUREA_DEPS:-$HOME/.cache/thurea-android}; mkdir -p "$D"

ARSCLIB_URL=https://repo1.maven.org/maven2/io/github/reandroid/ARSCLib/1.4.0/ARSCLib-1.4.0.jar
ARSCLIB_SHA=0cc95b160ba4c731a5cadb455f32b79f4e64b4160bc5ec3e5183ea3676b385ce
MANAGER_URL=https://github.com/ReVanced/revanced-manager/releases/download/v2.6.0/revanced-manager-2.6.0.apk
MANAGER_SHA=cba4c461553ea74eb25a7b2e1842419c7f4f40adc619b10d518c114204b7e846

fetch() {   # url sha256 file
  if [ -f "$3" ] && echo "$2  $3" | sha256sum -c --quiet 2>/dev/null; then return; fi
  curl -fsSL --retry 5 --retry-all-errors --retry-delay 10 -o "$3.part" "$1"
  echo "$2  $3.part" | sha256sum -c --quiet || { rm -f "$3.part"; echo "SHA-256 mismatch: $1" >&2; exit 1; }
  mv "$3.part" "$3"
}

fetch "$ARSCLIB_URL" "$ARSCLIB_SHA" "$D/ARSCLib-1.4.0.jar"
# Without its stand-ins for Android's own XmlResourceParser / AttributeSet (Android has the real ones).
if [ ! -f "$D/arsclib.jar" ]; then
  rm -rf "$D/arsclib" && mkdir "$D/arsclib" && (cd "$D/arsclib" && unzip -q ../ARSCLib-1.4.0.jar && rm -rf android META-INF)
  (cd "$D/arsclib" && zip -qr ../arsclib.jar .) && rm -rf "$D/arsclib"
fi

fetch "$MANAGER_URL" "$MANAGER_SHA" "$D/revanced-manager-2.6.0.apk"
for abi in arm64-v8a armeabi-v7a x86_64; do
  if [ ! -f "$D/aapt2/$abi/libaapt2.so" ]; then
    mkdir -p "$D/aapt2/$abi"
    unzip -p "$D/revanced-manager-2.6.0.apk" "lib/$abi/libaapt2.so" > "$D/aapt2/$abi/libaapt2.so"
  fi
done
echo "$D"
