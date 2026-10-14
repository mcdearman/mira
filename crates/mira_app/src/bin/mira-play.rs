//! Plays a game that is a folder of scripts: nothing but the game, in a window.
//!
//! ```text
//! cargo run --bin mira-play -- plugins/chase
//! cargo run --bin mira-play -- examples/sacred_sites --headless        # no window at all
//! cargo run --bin mira-play -- examples/sacred_sites --rules my.rules  # its rules kept as text
//! ```
//!
//! Every script in the folder is built and loaded, and built again when it is saved, so the
//! game can be written while it is played. The engine app (`cargo run -- <folder>`) is the
//! same game with the tools around it.
//!
//! `--headless` runs the game with no window and nothing drawn, for a machine with no
//! screen or an agent that only wants its rules: it listens for the debug connection
//! (`MIRA_DEBUG`, or 127.0.0.1:7878) and runs until told to stop (`mira-debug quit`).
//! `--rules <file>` keeps the game's signal rules and that file the same (see
//! `docs/SIGNALS.md`).

use std::time::Duration;

use mira::{audio::SilentAudioPlugin, input::InputPlugin, prelude::*, time::TimePlugin, transform::TransformPlugin};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut folder, mut headless, mut rules) = (None, false, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => headless = true,
            "--rules" => rules = Some(args.next().ok_or_else(|| anyhow::anyhow!("`--rules` is followed by the file to keep them in"))?),
            _ if folder.is_none() => folder = Some(arg),
            other => anyhow::bail!("not understood: `{other}` (a folder of scripts, `--headless`, `--rules <file>`)"),
        }
    }
    let Some(folder) = folder else {
        anyhow::bail!("say which game to play: a folder of scripts, as `mira-play plugins/chase`");
    };

    let mut game = App::new();
    if headless {
        // What a script may ask of the engine is all there, with no one to see or hear it.
        game.add_plugins(TimePlugin)
            .add_plugins(TransformPlugin)
            .add_plugins(InputPlugin)
            .add_plugins(SilentAudioPlugin)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<Image>>();
    } else {
        game.insert_resource(WindowSettings { title: folder.clone(), ..Default::default() }).add_plugins(DefaultPlugins);
    }
    game.add_plugins(PhysicsPlugin);
    if std::path::Path::new(&folder).is_dir() {
        game.load_scripts(&folder)?;
    } else {
        game.load_script(&folder)?;
    }
    if let Some((script, said)) = game.script_problems().next() {
        anyhow::bail!("{} does not build:\n{said}", script.display());
    }
    if let Some(rules) = rules {
        // After a first frame, so that the rules the scripts define as they start are there
        // to be written if the file is not.
        game.update();
        game.signal_rules(rules);
    }
    if !headless {
        return game.run();
    }
    let address = match game.debugger_address() {
        Some(address) => address,
        None => game.listen_for_debugger(&std::env::var("MIRA_DEBUG").unwrap_or_else(|_| "127.0.0.1:7878".to_owned()))?,
    };
    println!("{folder} is running with no window; watch it with:");
    println!("  cargo run --bin mira-debug -- --at {address} watch");
    while !game.should_exit() {
        game.update();
        std::thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}