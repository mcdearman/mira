//! Loads a native plugin into an engine with no window and says what it did: for trying a
//! plugin built by any language's tools against the real `MiraApi`, without writing a host.
//!
//! ```text
//! mira-plugin-check <library> [--frames N] [--entities N] [--show NAME]...
//!                   [--reload [LIBRARY]] [--broken LIBRARY] [--signals]
//! ```
//!
//! The engine has what a script may ask of it (time, transforms, input, sound with no output,
//! physics, meshes and images with nothing drawn) and `--entities` entities with a
//! transform each, at the origin (2 unless asked), before the plugin is loaded. Frames are a
//! sixtieth of a second each. After each run of `--frames` frames (10 unless asked) it
//! prints every entity that has a transform, with where it is and, for each `--show NAME`, the
//! bytes of that component as 32-bit integers and as floats; the plugin's loads; signals,
//! with `--signals`; and every system that failed, with its message.
//!
//! `--reload` loads the plugin again, as a hot reload does, from the same library or from
//! another (a rebuild with a change), and runs and reports again. `--broken` then offers a
//! library that should be refused, and runs and reports again: the version before it should
//! still be running.
//!
//! Exits with 1 if the plugin cannot be loaded at first, or a reload that should have
//! happened did not; otherwise 0, whatever the plugin did.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use mira::{
    audio::SilentAudioPlugin,
    input::InputPlugin,
    live::Live,
    prelude::*,
    signal::Signals,
    time::TimePlugin,
    transform::TransformPlugin,
};

struct Asked {
    library: PathBuf,
    frames: u32,
    entities: u32,
    show: Vec<String>,
    /// Load again: from the same library (`None` inside) or another.
    reload: Option<Option<PathBuf>>,
    broken: Option<PathBuf>,
    signals: bool,
}

fn asked() -> anyhow::Result<Asked> {
    let mut args = std::env::args().skip(1).peekable();
    let mut asked = Asked { library: PathBuf::new(), frames: 10, entities: 2, show: Vec::new(), reload: None, broken: None, signals: false };
    let mut library = None;
    let number = |value: Option<String>, what: &str| -> anyhow::Result<u32> { value.and_then(|value| value.parse().ok()).ok_or_else(|| anyhow::anyhow!("`{what}` is followed by a number")) };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => asked.frames = number(args.next(), "--frames")?,
            "--entities" => asked.entities = number(args.next(), "--entities")?,
            "--show" => asked.show.push(args.next().ok_or_else(|| anyhow::anyhow!("`--show` is followed by a component's name"))?),
            "--signals" => asked.signals = true,
            "--reload" => asked.reload = Some(args.next_if(|next| !next.starts_with("--")).map(PathBuf::from)),
            "--broken" => asked.broken = Some(args.next().map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("`--broken` is followed by a library"))?),
            _ if library.is_none() && !arg.starts_with("--") => library = Some(PathBuf::from(arg)),
            other => anyhow::bail!("not understood: `{other}`"),
        }
    }
    asked.library = library.ok_or_else(|| anyhow::anyhow!("say which library to load: mira-plugin-check <library> [--frames N] [--entities N] [--show NAME] [--reload [LIBRARY]] [--broken LIBRARY] [--signals]"))?;
    Ok(asked)
}

/// Puts a library where the engine loads it from, seen to be newer than what was there.
fn put(from: &Path, to: &Path, later: u64) -> anyhow::Result<()> {
    std::fs::copy(from, to).map_err(|err| anyhow::anyhow!("can't copy {}: {err}", from.display()))?;
    let file = std::fs::File::options().write(true).open(to)?;
    file.set_modified(SystemTime::now() + Duration::from_secs(later))?;
    Ok(())
}

fn report(game: &mut App, asked: &Asked, when: &str) {
    println!("--- {when}");
    let loads: Vec<String> = game.native_plugins().loaded().map(|(name, loads)| format!("{name} (load {loads})")).collect();
    println!("plugins: {}", if loads.is_empty() { "none".to_owned() } else { loads.join(", ") });
    let mut placed: Vec<(Entity, Vec3)> = game.world.query::<(Entity, &Transform)>().iter().map(|(entity, at)| (entity, at.translation)).collect();
    placed.sort_by_key(|(entity, _)| entity.to_bits());
    for (entity, at) in placed {
        let mut line = format!("entity {}: at ({}, {}, {})", entity.to_bits(), at.x, at.y, at.z);
        for name in &asked.show {
            line += &format!("; {name} = {}", shown(game, entity, name));
        }
        println!("{line}");
    }
    if asked.signals {
        for node in game.world.resource::<Signals>().graph() {
            println!("signal {} = {:?} ({})", node.name, node.value, node.kind);
        }
    }
    let failures = game.world.resource::<Live>().failures();
    if failures.is_empty() {
        println!("failures: none");
    }
    for failure in failures {
        println!("failure in {} on frame {}: {}", failure.system, failure.frame, failure.message);
    }
}

/// A component's bytes on an entity, as integers and as floats, or why there are none.
fn shown(game: &App, entity: Entity, name: &str) -> String {
    let Some(id) = game.world.named_component_id(name) else {
        return "no such component".to_owned();
    };
    let component = game.world.named_component(id).expect("it was just found");
    let size = component.layout.size();
    // SAFETY: nothing else is using the world while this looks; the storage holds a value of
    // `size` bytes for the entity if it has one.
    let Some(value) = game.world.erased_storage(component.key).and_then(|storage| unsafe { storage.value_ptr(entity) }) else {
        return "none on it".to_owned();
    };
    // SAFETY: as above.
    let bytes = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) };
    let (whole, floats): (Vec<String>, Vec<String>) = bytes.as_chunks::<4>().0.iter().map(|word| (i32::from_ne_bytes(*word).to_string(), f32::from_ne_bytes(*word).to_string())).unzip();
    format!("[{}] as integers, [{}] as floats", whole.join(", "), floats.join(", "))
}

fn main() -> anyhow::Result<()> {
    let asked = asked()?;
    // The engine loads from a copy that is replaced for each reload, named as the library is.
    let folder = std::env::temp_dir().join(format!("mira-plugin-check-{}", std::process::id()));
    std::fs::create_dir_all(&folder)?;
    let loaded = folder.join(asked.library.file_name().ok_or_else(|| anyhow::anyhow!("the library has no file name"))?);
    put(&asked.library, &loaded, 0)?;

    let mut game = App::new();
    game.add_plugins(TimePlugin)
        .add_plugins(TransformPlugin)
        .add_plugins(InputPlugin)
        .add_plugins(SilentAudioPlugin)
        .add_plugins(PhysicsPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Image>>();
    game.world.resource_mut::<Time>().set_fixed_step(Some(Duration::from_secs_f64(1.0 / 60.0)));
    for _ in 0..asked.entities {
        game.world.spawn(Transform::IDENTITY);
    }
    if let Err(err) = game.load_native_plugin(&loaded) {
        println!("not loaded: {err:#}");
        std::process::exit(1);
    }
    println!("loaded {}", asked.library.display());
    let run = |game: &mut App| {
        for _ in 0..asked.frames {
            game.update();
        }
    };
    run(&mut game);
    report(&mut game, &asked, &format!("after {} frames", asked.frames));

    let mut failed = false;
    if let Some(again) = &asked.reload {
        put(again.as_deref().unwrap_or(&asked.library), &loaded, 2)?;
        let reloaded = game.reload_native_plugins();
        println!("reloaded: {reloaded}");
        failed |= reloaded != 1;
        run(&mut game);
        report(&mut game, &asked, &format!("after the reload and {} frames more", asked.frames));
    }
    if let Some(broken) = &asked.broken {
        put(broken, &loaded, 4)?;
        let reloaded = game.reload_native_plugins();
        println!("reloaded: {reloaded} (a broken library should not be)");
        failed |= reloaded != 0;
        run(&mut game);
        report(&mut game, &asked, &format!("after the broken library and {} frames more", asked.frames));
    }
    let _ = std::fs::remove_dir_all(&folder);
    if failed {
        std::process::exit(1);
    }
    Ok(())
}
