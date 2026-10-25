#!/bin/sh
# Makes an iPhone app of a program built for iOS: a folder `<Name>.app` with the program and
# what iOS reads of it (ios/Info.plist). For the simulator the app is signed with no one's
# key, which is all the simulator asks; for a phone it is left unsigned, to be signed with a
# developer's certificate later.
#
#     ios/package.sh <program> <iPhoneOS | iPhoneSimulator> <out folder> [name] [identifier]
#
# The name is the app's as a person sees it, Tail Lights unless given; the identifier
# iOS's for it, dev.mira.taillights unless given.
set -e
program=$1
platform=$2
out=$3
name=${4:-Tail Lights}
identifier=${5:-dev.mira.taillights}
here=$(dirname "$0")
# The program inside is named as the app, without its spaces.
executable=$(printf '%s' "$name" | tr -d ' ')
app="$out/$name.app"
rm -rf "$app"
mkdir -p "$app"
cp "$program" "$app/$executable"
sed -e "s/PLATFORM/$platform/" -e "s/NAME/$name/" -e "s/PROGRAM/$executable/" -e "s/IDENTIFIER/$identifier/" "$here/Info.plist" >"$app/Info.plist"
plutil -lint "$app/Info.plist"
printf 'APPL????' >"$app/PkgInfo"
if [ "$platform" = iPhoneSimulator ]; then
    codesign --force --sign - --timestamp=none "$app"
fi
ls -l "$app"