//! Plays a game that is a folder of scripts: nothing but the game, in a window.
//!
//! ```text
//! cargo run --bin mira-play -- plugins/chase
//! ```
//!
//! Every script in the folder is built and loaded, and built again when it is saved, so the
//! game can be written while it is played. The engine app (`cargo run -- <folder>`) is the
//! same game with the tools around it.

use mira::prelude::*;

fn main() -> anyhow::Result<()> {
    let Some(folder) = std::env::args().nth(1) else {
        anyhow::bail!("say which game to play: a folder of scripts, as `mira-play plugins/chase`");
    };
    let mut game = App::new();
    game.insert_resource(WindowSettings { title: folder.clone(), ..Default::default() }).add_plugins(DefaultPlugins).add_plugins(PhysicsPlugin);
    if std::path::Path::new(&folder).is_dir() {
        game.load_scripts(&folder)?;
    } else {
        game.load_script(&folder)?;
    }
    if let Some((script, said)) = game.script_problems().next() {
        anyhow::bail!("{} does not build:\n{said}", script.display());
    }
    game.run()
}