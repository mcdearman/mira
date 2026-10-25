//! RetroDunk on an Android phone.
//!
//! The game is a Meadow script (examples/retro_dunk/Dunk.mw), built ahead of time into a
//! library that the package carries, `libDunk.so`. This is only where Android begins it: the
//! engine, and the game loaded where it is, as a plugin that is part of the app. What is
//! logged, and what a panic says, goes to the phone's log: `adb logcat -s mira`.
#![cfg(target_os = "android")]

use mira::prelude::*;

#[no_mangle]
fn android_main(activity: mira::window::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("mira"));
    std::panic::set_hook(Box::new(|panic| log::error!("{panic}")));
    mira::window::started_in(activity);
    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    // The package's own libraries are found by their name alone.
    if let Err(err) = app.load_bundled_plugin("libDunk.so") {
        log::error!("the game could not be loaded: {err:#}");
        return;
    }
    log::info!("RetroDunk is loaded");
    if let Err(err) = app.run() {
        log::error!("the game stopped: {err:#}");
    }
}