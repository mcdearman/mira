//! Tail Lights on an Android phone.
//!
//! The game is the example's, every line of it: this is only where Android begins it. A
//! phone has no terminal, so what is logged, and what a panic says, goes to the system's
//! log, read with `adb logcat -s mira`.
#![cfg(target_os = "android")]

#[path = "../../examples/tail_lights/main.rs"]
mod game;

#[no_mangle]
fn android_main(activity: mira::window::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("mira"));
    std::panic::set_hook(Box::new(|panic| log::error!("{panic}")));
    mira::window::started_in(activity);
    if let Err(err) = game::main() {
        log::error!("the game stopped: {err:#}");
    }
}