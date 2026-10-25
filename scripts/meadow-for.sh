#!/bin/sh
# Builds a Meadow script, with mira's bindings, into a library for another system: a
# phone's, a .so for Android or a static .a for iOS, as the engine builds one for the
# machine it runs on (src/plugin/scripts.rs) and with the same package around it.
#
#     scripts/meadow-for.sh <Script.mw> <target, as aarch64-linux-android> <out folder>
#
# MIRA_MEADOWBOOT says where MeadowBoot is (else `meadowboot` on the path): from a Meadow
# release that carries the phones' runtimes. Android needs the NDK (ANDROID_NDK_HOME or
# ANDROID_NDK_LATEST_HOME), iOS Xcode. Prints where the library was put.
set -e
script=$1
target=$2
out=$3
here=$(cd "$(dirname "$0")/.." && pwd)
stem=$(basename "$script" .mw)
# A package's name is a module's, so it begins with a capital.
title=$(printf '%s' "$stem" | cut -c1 | tr '[:lower:]' '[:upper:]')$(printf '%s' "$stem" | cut -c2-)
work=$(mktemp -d)
mkdir -p "$work/Mira/src" "$work/$title/src" "$out"
cp "$here/bindings/meadow/Meadow.toml" "$work/Mira/"
cp "$here/bindings/meadow/src/Lib.mw" "$work/Mira/src/"
printf '[package]\nname = "%s"\nversion = "0.1.0"\n\n[dependencies]\nMira = { path = "../Mira" }\n' "$title" >"$work/$title/Meadow.toml"
cp "$script" "$work/$title/src/Main.mw"
"${MIRA_MEADOWBOOT:-meadowboot}" build --release --target "$target" "$work/$title" >&2
built=$(find "$work/$title/target" \( -name "lib$title.so" -o -name "lib$title.a" \) | head -n 1)
if [ -z "$built" ]; then
    echo "MeadowBoot built no library of $title for $target" >&2
    exit 1
fi
cp "$built" "$out/"
echo "$out/$(basename "$built")"