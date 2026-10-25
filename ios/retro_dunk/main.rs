//! RetroDunk on an iPhone.
//!
//! The game is a Meadow script (examples/retro_dunk/Dunk.mw), built ahead of time into a
//! static library and linked into this program, since iOS lets an app open no library of
//! its own. This is only where iOS begins it: the engine, and the game started from the
//! entry points linked in.
#[cfg(target_os = "ios")]
mod app {
    use mira::{
        plugin::{LinkedPlugin, MiraApi, MiraApp},
        prelude::*,
    };

    extern "C" {
        fn mira_plugin_abi_version() -> u32;
        fn mira_plugin_load(api: *const MiraApi, app: *mut MiraApp) -> i32;
    }

    pub fn main() -> anyhow::Result<()> {
        env_logger::init();
        let mut app = App::new();
        app.add_plugins(DefaultPlugins);
        app.load_linked_plugin("Dunk", LinkedPlugin { abi_version: mira_plugin_abi_version, load: mira_plugin_load, flags: None })?;
        log::info!("RetroDunk is loaded");
        app.run()
    }
}

#[cfg(target_os = "ios")]
fn main() -> anyhow::Result<()> {
    app::main()
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("RetroDunk's iOS app is built for iOS; elsewhere, play examples/retro_dunk with mira-play");
}