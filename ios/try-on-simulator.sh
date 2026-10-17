#!/bin/sh
# Puts an app on an iPhone simulator, starts it, leaves it a while, and keeps what it said
# and what was on the screen. It never fails: whether the game lived is in what it keeps.
#
#     ios/try-on-simulator.sh <Tail Lights.app>
app=$1
name=dev.mira.taillights
# An iPhone the runner has, the newest listed.
device=$(xcrun simctl list devices available | grep -E '^ +iPhone' | tail -n 1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/')
echo "simulator: $(xcrun simctl list devices available | grep "$device")"
xcrun simctl boot "$device" || true
xcrun simctl bootstatus "$device" -b || true
xcrun simctl install "$device" "$app" || { echo "not installed" >tried-alive.txt; exit 0; }
# What the game prints, kept as it comes, so that a game that stops has still said it.
xcrun simctl launch --console-pty --terminate-running-process "$device" "$name" >tried-log.txt 2>&1 &
sleep 45
if xcrun simctl spawn "$device" launchctl list | grep -q "$name"; then
    echo "running" >tried-alive.txt
else
    echo "not running" >tried-alive.txt
fi
xcrun simctl io "$device" screenshot tried-screen.png || echo "no picture"
xcrun simctl terminate "$device" "$name" || true
echo "--- still running?"
cat tried-alive.txt
echo "--- what it said"
tail -n 120 tried-log.txt
exit 0
