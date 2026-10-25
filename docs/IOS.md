# iOS

Tail Lights is built for the iPhone and tried on the iPhone simulator on every change to it
(the "iOS app" workflow, on a macOS runner). It has not been on a real iPhone yet: that
needs an Apple developer account, below.

## How a game starts

On iOS a game is an ordinary Rust program: winit starts the app (`UIApplicationMain`) when
the game's app is run, and the window it makes is the screen. So the example is built as it
is, for `aarch64-apple-ios` (a phone) or `aarch64-apple-ios-sim` (the simulator on an
Apple-silicon Mac), with Xcode's SDKs:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
IPHONEOS_DEPLOYMENT_TARGET=16.0 cargo build --example tail_lights --target aarch64-apple-ios-sim
sh ios/package.sh target/aarch64-apple-ios-sim/debug/examples/tail_lights iPhoneSimulator out
sh ios/try-on-simulator.sh "out/Tail Lights.app"
```

`ios/package.sh` makes the app: a folder `Tail Lights.app` with the program and what iOS
reads of it ([`ios/Info.plist`](../ios/Info.plist): its name and identifier,
`dev.mira.taillights`, landscape only, the whole screen, Metal required, iOS 16 or later).
For the simulator it is signed with no one's key, which is all the simulator asks.
`ios/try-on-simulator.sh` puts it on an iPhone simulator, runs it for 45 seconds, and keeps
what the game printed (`tried-log.txt`), whether it was still running, and a picture of
the screen. The picture is in the phone's upright orientation, so a landscape game lies on
its side in it.

## A game written as a script

iOS lets an app open no library of its own, so a Meadow script is built ahead of time into
a static library and linked into the program: `scripts/meadow-for.sh` with the target
`aarch64-apple-ios` (or `aarch64-apple-ios-sim`) and MeadowBoot from Meadow's Apple-silicon
release archive, which carries the iPhone's runtimes. `ios/retro_dunk` is the app: its
build script links `libDunk.a` from the folder `RETRO_DUNK_LIB` names, and it starts the
game with `app.load_linked_plugin("Dunk", LinkedPlugin { ... })`, from the entry points
linked in. The iOS workflow builds RetroDunk this way and tries it on the simulator.

## What is not as on a desktop

| | |
|---|---|
| The window | The screen. A size in `WindowSettings` is for a desktop: iOS would make a window that size, bigger than the screen, and show its corner. |
| Graphics | Metal. The device is asked for no more than it has: the simulator's falls short of WebGPU's defaults (15 values between shader stages, not 16). |
| What is seen | Decided on the processor, as on Android. |
| Saved games | In the app's own `Library/Application Support`. |
| Sound | Core Audio, through cpal. |
| Game controllers | None yet, as on Android. |

## Onto an iPhone

Apple lets an app onto a phone only when it is signed with certificates Apple issues. The
ways there:

| Way | Needs | For |
|---|---|---|
| TestFlight | The Apple Developer Program ($99 a year) | Anyone invited, on any iPhone, from a link; builds last 90 days; testers outside the team need Apple's beta review, about a day. |
| Ad hoc | The same account, and each phone's identifier registered (100 a year) | A few known phones, no review. |
| Xcode and a free Apple ID | A Mac and a cable | Your own phone, for 7 days at a time. |
| The App Store | The same account, Apple's full review | Everyone. |

For TestFlight the workflow would sign the phone's build with a distribution certificate
and upload it with an App Store Connect API key, both kept as repository secrets.

## What there is not yet

- A run on a real iPhone, and signing for one.
- An icon, and a launch screen other than plain white.
- Touch tried on iOS: it is winit's, as on Android, where it works.
- A game written as a script played on a real iPhone (RetroDunk runs on the simulator).
