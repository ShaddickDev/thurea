#!/bin/bash
# Sets up the release build tools in the Arch Linux container of .github/workflows/release.yml
# (runs as root): packages, a "builder" user (makepkg refuses root), Rust with the Windows and
# Android targets, and the Android SDK where android/build.sh expects it (~/Android/Sdk).
set -euo pipefail
pacman -Syu --noconfirm --needed git sudo rustup mingw-w64-gcc chromium dpkg rpm-tools mono github-cli \
  jdk17-openjdk python openssl unzip zip nodejs npm osslsigncode
archlinux-java set java-17-openjdk

id builder >/dev/null 2>&1 || useradd -m builder
chown -R builder: "$PWD"
as() { sudo -H -u builder bash -c "$1"; }

as 'rustup default stable >/dev/null && rustup target add x86_64-pc-windows-gnu aarch64-linux-android armv7-linux-androideabi x86_64-linux-android'

# Android command-line tools, then the platform, build tools and NDK android/build.sh uses.
as 'set -e
SDK=$HOME/Android/Sdk; mkdir -p "$SDK/cmdline-tools"
curl -fsSLo /tmp/clt.zip https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip
unzip -q /tmp/clt.zip -d "$SDK/cmdline-tools" && mv "$SDK/cmdline-tools/cmdline-tools" "$SDK/cmdline-tools/latest"
export JAVA_HOME=/usr/lib/jvm/java-17-openjdk
M="$SDK/cmdline-tools/latest/bin/sdkmanager --sdk_root=$SDK"
(yes || true) | $M --licenses >/dev/null
$M "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018" >/dev/null
echo "Android SDK ready"'
