//! Saved games.
//!
//! A saved game is a [`Scene`] of the whole world, with a few words about it, kept in a file
//! of its own under a name: a slot. [`SaveGames`] is the folder they are in. Saving writes
//! the world as it is; loading makes the world what was saved, and nothing else.
//!
//! What is saved is what a scene holds: the registered components of every entity not
//! marked [`NotSaved`](crate::reflect::NotSaved), and the registered resources. A game makes
//! its own state part of its saves by registering it (`app.register_type`,
//! `app.register_resource_type`): see [SCENES.md](../docs/SCENES.md).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};

use crate::{
    ecs::World,
    reflect::{json, Scene, Spawned, TypeRegistry, Value},
    time::Time,
};

/// The version of the file a save is written as: what is around the scene, not the scene.
const VERSION: i64 = 1;
const ENDING: &str = "save.json";

/// What a saved game says of itself, without the game being loaded: what a list of saves
/// shows.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveInfo {
    /// The name it is kept under.
    pub slot: String,
    /// A few words the game gave it: where the player was, what they had done.
    pub note: String,
    /// When it was saved.
    pub saved_at: SystemTime,
    /// How long the game had been played, by its own clock.
    pub played: Duration,
    /// How many entities it holds.
    pub entities: usize,
}

/// The folder saved games are kept in. A resource, where a game wants one to hand.
#[derive(Clone, Debug)]
pub struct SaveGames {
    folder: PathBuf,
}

impl SaveGames {
    /// Saves kept in this folder, which is made when the first is saved.
    pub fn in_folder(folder: impl Into<PathBuf>) -> Self {
        Self { folder: folder.into() }
    }

    /// Saves kept where the system keeps a program's own files for the person using it,
    /// under the game's name: `%APPDATA%\<game>\saves` on Windows,
    /// `~/Library/Application Support/<game>/saves` on a Mac, and `$XDG_DATA_HOME` or
    /// `~/.local/share` elsewhere. Beside the program, in `saves`, where none of those can
    /// be found. On Android they are among the app's own files, which need no name: the
    /// system keeps each app's apart.
    pub fn for_game(game: &str) -> Self {
        #[cfg(target_os = "android")]
        if let Some(files) = crate::window::android_files() {
            return Self::in_folder(files.join("saves"));
        }
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = if cfg!(windows) {
            var("APPDATA")
        } else if cfg!(target_os = "macos") {
            var("HOME").map(|home| home.join("Library").join("Application Support"))
        } else {
            var("XDG_DATA_HOME").or_else(|| var("HOME").map(|home| home.join(".local").join("share")))
        };
        Self::in_folder(home.map_or_else(|| PathBuf::from("saves"), |home| home.join(game).join("saves")))
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// The file a slot is kept in. A slot's name is letters, digits, spaces, `-` and `_`,
    /// so that it is a file's name on any system and can lead nowhere else.
    fn file(&self, slot: &str) -> anyhow::Result<PathBuf> {
        let plain = !slot.is_empty() && slot.len() <= 64 && slot.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')) && slot.trim() == slot;
        if !plain {
            bail!("`{slot}` cannot name a save: letters, digits, spaces, `-` and `_` only");
        }
        Ok(self.folder.join(format!("{slot}.{ENDING}")))
    }

    /// Saves the world under a name, in place of whatever was there. It is written beside
    /// where it goes and then put in place whole, so that a save cut short leaves the one
    /// before it as it was.
    pub fn save(&self, world: &World, slot: &str, note: &str) -> anyhow::Result<SaveInfo> {
        let path = self.file(slot)?;
        let scene = Scene::capture(world, world.resource::<TypeRegistry>());
        let info = SaveInfo {
            slot: slot.to_owned(),
            note: note.to_owned(),
            saved_at: SystemTime::now(),
            played: world.get_resource::<Time>().map_or(Duration::ZERO, |time| time.elapsed()),
            entities: scene.entities.len(),
        };
        let since = info.saved_at.duration_since(UNIX_EPOCH).unwrap_or_default();
        let file = Value::Map(vec![
            ("mira_save".to_owned(), Value::Int(VERSION)),
            ("note".to_owned(), Value::Text(info.note.clone())),
            ("saved_at".to_owned(), Value::Int(since.as_secs() as i64)),
            ("played".to_owned(), Value::Float(info.played.as_secs_f64())),
            ("entity_count".to_owned(), Value::Int(info.entities as i64)),
            ("scene".to_owned(), scene.to_value()),
        ]);
        std::fs::create_dir_all(&self.folder).with_context(|| format!("making {}", self.folder.display()))?;
        let beside = path.with_extension("writing");
        std::fs::write(&beside, json::to_string(&file)).with_context(|| format!("writing {}", beside.display()))?;
        std::fs::rename(&beside, &path).with_context(|| format!("putting {} in place", path.display()))?;
        Ok(info)
    }

    fn read(path: &Path, slot: &str) -> anyhow::Result<(SaveInfo, Value)> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let file = json::parse(&text).map_err(|err| anyhow::anyhow!("{}: {err}", path.display()))?;
        match file.field("mira_save") {
            Some(Value::Int(VERSION)) => {}
            Some(Value::Int(other)) => bail!("{} is a version {other} save, and this engine reads version {VERSION}", path.display()),
            _ => bail!("{} is not a saved game", path.display()),
        }
        let number = |name: &str| file.field(name).and_then(Value::as_f64).unwrap_or(0.0).max(0.0);
        let info = SaveInfo {
            slot: slot.to_owned(),
            note: match file.field("note") {
                Some(Value::Text(note)) => note.clone(),
                _ => String::new(),
            },
            saved_at: UNIX_EPOCH + Duration::from_secs(number("saved_at") as u64),
            played: Duration::from_secs_f64(number("played")),
            entities: number("entity_count") as usize,
        };
        Ok((info, file))
    }

    /// What a save says of itself, without loading it.
    pub fn info(&self, slot: &str) -> anyhow::Result<SaveInfo> {
        Ok(Self::read(&self.file(slot)?, slot)?.0)
    }

    /// Makes the world what was saved under a name: what a scene would capture of it now is
    /// despawned and what was saved is put in its place, its resources with it. Answers what
    /// was made, and what could not be: a component of a type this game no longer has is
    /// left out and told of, and the rest of the save is loaded.
    pub fn load(&self, world: &mut World, slot: &str) -> anyhow::Result<(SaveInfo, Spawned)> {
        let path = self.file(slot)?;
        let (info, file) = Self::read(&path, slot)?;
        let scene = file.field("scene").context("the save has no scene").and_then(|scene| Ok(Scene::from_value(scene)?)).with_context(|| format!("in {}", path.display()))?;
        let spawned = world.resource_scope(|world, registry: &mut TypeRegistry| scene.replace(world, registry));
        Ok((info, spawned))
    }

    /// Whether there is a save of this name.
    pub fn exists(&self, slot: &str) -> bool {
        self.file(slot).is_ok_and(|path| path.is_file())
    }

    /// Does away with a save. One that is not there is no error.
    pub fn delete(&self, slot: &str) -> anyhow::Result<()> {
        let path = self.file(slot)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err).with_context(|| format!("removing {}", path.display())),
        }
    }

    /// The saves there are, the latest first. A file in the folder that is not a save, or
    /// cannot be read, is passed over.
    pub fn list(&self) -> Vec<SaveInfo> {
        let Ok(files) = std::fs::read_dir(&self.folder) else { return Vec::new() };
        let mut saves: Vec<SaveInfo> = files
            .filter_map(|file| {
                let path = file.ok()?.path();
                let slot = path.file_name()?.to_str()?.strip_suffix(&format!(".{ENDING}"))?.to_owned();
                Some(Self::read(&path, &slot).ok()?.0)
            })
            .collect();
        saves.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then_with(|| a.slot.cmp(&b.slot)));
        saves
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::App,
        ecs::Component,
        reflect::{NotSaved, Reflect},
        transform::{Parent, Transform, TransformPlugin},
    };

    #[derive(Clone, Debug, PartialEq, Default, Reflect)]
    #[reflect(name = "test.Health", default)]
    struct Health {
        current: f32,
    }

    impl Component for Health {}

    #[derive(Clone, Debug, PartialEq, Default, Reflect)]
    #[reflect(name = "test.Score", default)]
    struct Score {
        points: u32,
    }

    /// A component the game has not registered: nothing of it is saved.
    struct Scratch;

    impl Component for Scratch {}

    fn game() -> App {
        let mut app = App::new();
        app.add_plugins(crate::time::TimePlugin).add_plugins(TransformPlugin);
        app.register_type::<Health>().register_resource_type::<Score>();
        app.world.insert_resource(Score { points: 0 });
        app
    }

    /// A folder of its own for a test, emptied first.
    fn saves(test: &str) -> SaveGames {
        let folder = std::env::temp_dir().join(format!("mira-saves-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        SaveGames::in_folder(folder)
    }

    #[test]
    fn a_game_saved_is_the_game_loaded_whatever_has_happened_since() {
        let saves = saves("round-trip");
        let mut app = game();
        let hero = app.world.spawn((Transform::from_xyz(1.0, 2.0, 3.0), Health { current: 80.0 }));
        let sword = app.world.spawn((Transform::from_xyz(0.5, 0.0, 0.0), Parent(hero)));
        let overlay = app.world.spawn((Transform::IDENTITY, NotSaved));
        app.world.resource_mut::<Score>().points = 7;
        app.update();

        let info = saves.save(&app.world, "before the bridge", "Chapter 2").unwrap();
        assert_eq!((info.entities, info.note.as_str()), (2, "Chapter 2"));
        assert!(saves.exists("before the bridge") && !saves.exists("another"));

        // Things happen: the hero is hurt and moves, the sword is lost, a monster comes.
        app.world.get_mut::<Health>(hero).unwrap().current = 5.0;
        app.world.despawn(sword);
        app.world.spawn((Transform::from_xyz(9.0, 0.0, 0.0), Health { current: 300.0 }));
        app.world.resource_mut::<Score>().points = 99;

        let (read, spawned) = saves.load(&mut app.world, "before the bridge").unwrap();
        assert_eq!(read.note, "Chapter 2");
        assert!(spawned.skipped.is_empty(), "{:?}", spawned.skipped);
        assert_eq!(spawned.entities.len(), 2);
        // The world is what was saved and nothing else: one with health, at 80; the sword
        // back, under the hero as it now is; no monster; the score as it was.
        let with_health: Vec<f32> = spawned.entities.iter().filter_map(|entity| app.world.get::<Health>(*entity)).map(|health| health.current).collect();
        assert_eq!(with_health, [80.0]);
        let new_hero = spawned.entities[0];
        assert_eq!(app.world.get::<Parent>(spawned.entities[1]).map(|parent| parent.0), Some(new_hero));
        assert_eq!(app.world.get::<Transform>(new_hero).unwrap().translation, glam::Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(app.world.resource::<Score>().points, 7);
        let health = app.world.storage::<Health>().map_or(0, |storage| storage.entities().len());
        assert_eq!(health, 1, "the monster is gone");
        // What was marked as not to be saved is left where it was.
        assert!(app.world.has::<NotSaved>(overlay));
        let _ = std::fs::remove_dir_all(saves.folder());
    }

    #[test]
    fn a_save_is_read_in_another_run_of_the_game() {
        let saves = saves("another-run");
        let mut first = game();
        first.world.spawn((Transform::from_xyz(4.0, 0.0, 0.0), Health { current: 12.0 }, Scratch));
        first.world.resource_mut::<Score>().points = 3;
        saves.save(&first.world, "slot_1", "").unwrap();
        // A new run: other entities are there, under ids that mean something else.
        let mut second = game();
        for _ in 0..5 {
            second.world.spawn((Transform::IDENTITY, Health { current: 1.0 }));
        }
        let (_, spawned) = saves.load(&mut second.world, "slot_1").unwrap();
        assert_eq!(spawned.entities.len(), 1);
        assert_eq!(second.world.storage::<Health>().unwrap().entities().len(), 1);
        assert_eq!(second.world.get::<Health>(spawned.entities[0]).unwrap().current, 12.0);
        assert_eq!(second.world.resource::<Score>().points, 3);
        assert!(second.world.storage::<Scratch>().is_none_or(|scratch| scratch.entities().is_empty()), "what is not registered is not saved");
        let _ = std::fs::remove_dir_all(saves.folder());
    }

    #[test]
    fn saves_are_listed_the_latest_first_and_done_away_with() {
        let saves = saves("listing");
        assert!(saves.list().is_empty(), "no folder yet is no saves");
        let app = game();
        saves.save(&app.world, "first", "a").unwrap();
        saves.save(&app.world, "second", "b").unwrap();
        // Saved again, a slot holds the later one and is still one slot.
        saves.save(&app.world, "first", "c").unwrap();
        std::fs::write(saves.folder().join("notes.txt"), "not a save").unwrap();
        std::fs::write(saves.folder().join("broken.save.json"), "{ not json").unwrap();
        let listed = saves.list();
        assert_eq!(listed.len(), 2);
        assert_eq!(saves.info("first").unwrap().note, "c");
        assert!(listed.windows(2).all(|pair| pair[0].saved_at >= pair[1].saved_at));
        saves.delete("first").unwrap();
        saves.delete("first").unwrap();
        assert_eq!(saves.list().iter().map(|save| save.slot.as_str()).collect::<Vec<_>>(), ["second"]);
        // Nothing half-written is left beside them.
        let left: Vec<String> = std::fs::read_dir(saves.folder()).unwrap().map(|file| file.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(!left.iter().any(|name| name.ends_with(".writing")), "{left:?}");
        let _ = std::fs::remove_dir_all(saves.folder());
    }

    #[test]
    fn a_name_that_leads_elsewhere_and_a_file_that_is_no_save_are_refused() {
        let saves = saves("refused");
        let mut app = game();
        for slot in ["", "../outside", "a/b", "c:\\d", "dot.dot", " padded ", &"x".repeat(65)] {
            assert!(saves.save(&app.world, slot, "").is_err(), "`{slot}`");
            assert!(!saves.exists(slot));
        }
        assert!(saves.load(&mut app.world, "never saved").is_err());
        std::fs::create_dir_all(saves.folder()).unwrap();
        std::fs::write(saves.folder().join("old.save.json"), r#"{"mira_save": 99, "scene": {}}"#).unwrap();
        std::fs::write(saves.folder().join("other.save.json"), r#"{"version": 1, "entities": []}"#).unwrap();
        assert!(saves.load(&mut app.world, "old").unwrap_err().to_string().contains("version 99"));
        assert!(saves.load(&mut app.world, "other").unwrap_err().to_string().contains("not a saved game"));
        // The world is as it was after a load that was refused.
        let before = app.world.spawn(Health { current: 1.0 });
        assert!(saves.load(&mut app.world, "old").is_err() && app.world.has::<Health>(before));
        assert!(SaveGames::for_game("Sacred Sites").folder().ends_with(std::path::Path::new("Sacred Sites").join("saves")) || SaveGames::for_game("x").folder() == std::path::Path::new("saves"));
        let _ = std::fs::remove_dir_all(saves.folder());
    }
}