#!/bin/bash
# Builds out/scan.dex (the lab's stand-in QR scanner).
set -euo pipefail
L=$(cd "$(dirname "$0")/.." && pwd); SDK=~/Android/Sdk; BT=$SDK/build-tools/35.0.0; JAR=$SDK/platforms/android-35/android.jar
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
javac -source 17 -target 17 -Xlint:-options -classpath "$JAR" -d "$T" "$L/scan/Scan.java"
"$BT/d8" --min-api 34 --lib "$JAR" --output "$T" "$T"/*.class
mv "$T/classes.dex" "$L/out/scan.dex"
