#!/bin/sh
# Puts a package on the phone that adb has (an emulator, in the workflow), starts it, leaves
# it a while, and keeps what it logged and what was on the screen. It never fails and never
# waits long: an emulator draws on the processor, and can be too slow to answer.
#
#     android/try-on-emulator.sh <package.apk> <its name, as dev.mira.taillights>
apk=$1
name=$2
adb install "$apk" || exit 0
adb logcat -c
# Everything logged from now on, kept as it comes, so that a phone that stops answering
# has still said what it said.
adb logcat >tried-log.txt 2>&1 &
logging=$!
adb shell am start -n "$name/android.app.NativeActivity"
sleep 45
timeout 20 adb shell pidof "$name" >tried-alive.txt 2>&1 || echo "not running, or the phone did not answer" >tried-alive.txt
timeout 40 adb exec-out screencap -p >tried-screen.png 2>/dev/null || echo "no picture: the phone did not answer in time"
kill "$logging" 2>/dev/null
echo "--- still running?"
cat tried-alive.txt
echo "--- what it said"
grep -E " mira *:|FATAL|dlopen failed|Abort message" tried-log.txt | tail -n 120
exit 0