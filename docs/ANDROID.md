# Android

The engine is built for Android (`aarch64-linux-android`) on every change, so nothing gets
written that a phone cannot have, and Tail Lights is packaged for one. What is here is what
it takes to build, the places where Android is not like a desktop, and what has been seen
on real phones.

## On real phones

| Phone | What was seen (October 2026) |
|---|---|
| Pixel 8 | Tail Lights plays. |
| Galaxy S26 | Tail Lights plays, with what is seen decided on the processor. With it decided on the graphics card, as on a desktop, only the background was drawn. |

So on a phone what is seen is decided on the processor (`MIRA_CULL=gpu` asks for the
card's way). The S26's graphics say they can draw indirectly from what a compute pass
listed and then draw nothing; why has not been looked into, nor which of the S26's two
kinds of graphics that phone had. The first package of all closed as it opened on both:
it lacked the C++ library its sound needs, which the package now carries.

## Starting a game

On Android a game is a library the system loads into an activity, and it begins at
`android_main`, which is handed that activity. The engine must be told of it before the app
is run: the window and the app's files come from it.

```rust
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(activity: mira::window::AndroidApp) {
    mira::window::started_in(activity);
    game().run().unwrap();
}
```

The game's crate is built as a `cdylib`, with the Android NDK's compilers for what is
written in C among the engine's dependencies (CI sets `CC_aarch64_linux_android`,
`CXX_aarch64_linux_android` and `AR_aarch64_linux_android` to the NDK's own; `cargo ndk`
and `cargo apk` do the same and make the package).

## A package to put on a phone

`android/tail_lights` is Tail Lights as an app: a small crate that takes the example's code
in and begins it from `android_main`. With the Android SDK and NDK and `cargo install
cargo-apk`:

```sh
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo apk build -p tail_lights_android
# target/debug/apk/tail-lights.apk; `cargo apk run` puts it on a phone that is plugged in
```

It is signed with the key for trying things, and built fast all the same, since a phone
crawls through the engine unoptimized. Without the tools, the "Android package" workflow
builds it on a runner and keeps the package (Actions, the run, `tail-lights-apk`). What the
game logs, and what a panic says, is in the phone's log: `adb logcat -s mira`.

## A game written as a script

On a desktop the engine builds a game's Meadow scripts itself while it runs. A phone cannot:
the script is built ahead of time, on a desktop or a runner, into a library the package
carries, and the app loads it where it is.

```sh
# MeadowBoot from Meadow's release (v0.4.0 on): its archive carries the phones' runtimes.
ANDROID_NDK_HOME=... MIRA_MEADOWBOOT=.../MeadowBoot \
    sh scripts/meadow-for.sh examples/retro_dunk/Dunk.mw aarch64-linux-android android/retro_dunk/libs/arm64-v8a
CARGO_PROFILE_DEV_OPT_LEVEL=3 cargo apk build -p retro_dunk_android
```

`scripts/meadow-for.sh` wraps the script in the package the engine would (mira's bindings
beside it) and builds it for the target. `android/retro_dunk` is the app: the engine, with
`app.load_bundled_plugin("libDunk.so")`, and cargo-apk's `runtime_libs` carries the
library for each processor. The Android workflow builds RetroDunk this way, and keeps the
package (`retro-dunk-apk`).

## What is not as on a desktop

| | |
|---|---|
| Input | Fingers: see [INPUT.md](INPUT.md). Bind swipes and taps beside keys and the game is the same game. |
| Saved games | `SaveGames::for_game` keeps them among the app's own files, which the system keeps apart for each app. |
| Sound | Goes out through the system's own (AAudio, by way of Oboe), as it does through ALSA or WASAPI elsewhere. |
| Ray tracing | Used where the phone's graphics have it and gone without where they have not, as on any machine. |
| What is seen | Decided on the processor, not the graphics card: see above. |

## Put away and brought back

When an app is put away Android takes its window, and gives it another when it is brought
back. The engine lets go of the surface it draws on as the window goes, does not run the
game while it is away, and takes up a new surface when it is back; the device and all that
is on it stay as they were. This can be tried at a desk, on a running game:

```text
mira-debug window to=away
mira-debug window to=back
```

Away, frames are drawn off screen (a screenshot still works); back, they are shown again.
Tried so on Windows, with Vulkan. Sound is held where it is while the app is away, and goes on when it is back.

## What there is not yet

- Why deciding what is seen on the graphics card draws nothing on a Galaxy S26.
- More phones tried than two, and anything but Tail Lights tried on one.
- Assets: a desktop game reads `res/` beside the program; in a package they are inside it.
- The editor and the interface crate, which need a clipboard a phone does not give.

iOS is in [IOS.md](IOS.md).