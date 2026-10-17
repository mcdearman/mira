#!/bin/sh
# Makes an iPhone app of a program built for iOS: a folder `Tail Lights.app` with the
# program and what iOS reads of it (ios/Info.plist). For the simulator the app is signed
# with no one's key, which is all the simulator asks; for a phone it is left unsigned, to be
# signed with a developer's certificate later.
#
#     ios/package.sh <program> <iPhoneOS | iPhoneSimulator> <out folder>
set -e
program=$1
platform=$2
out=$3
here=$(dirname "$0")
app="$out/Tail Lights.app"
rm -rf "$app"
mkdir -p "$app"
cp "$program" "$app/TailLights"
sed "s/PLATFORM/$platform/" "$here/Info.plist" >"$app/Info.plist"
plutil -lint "$app/Info.plist"
printf 'APPL????' >"$app/PkgInfo"
if [ "$platform" = iPhoneSimulator ]; then
    codesign --force --sign - --timestamp=none "$app"
fi
ls -l "$app"
