//! Scripts: plugins the engine builds itself, from their source.
//!
//! A game can be a folder of scripts and nothing else. [`App::load_scripts`] takes each
//! Meadow file (`.mw`) and each Haskell file (`.hs`) in a folder, builds it into a plugin,
//! and loads it; from then on a script
//! that is saved is built again, behind the game, and the game goes on with the new one as
//! soon as it is ready. One that does not build is told of, with what the compiler said,
//! and the game carries on with the last that did.
//!
//! What is needed to build a Haskell script is GHC, on the path; a Meadow script, MeadowBoot
//! (`MIRA_MEADOWBOOT`, or `meadowboot` on the path, with what it needs set as Meadow says:
//! `MEADOW`, `MEADOWBOOT_STD`, and clang on the path). The bindings a script is built with
//! ([`bindings/meadow`](../../../bindings/meadow), [`Mira.hs`](../../../bindings/haskell/Mira.hs)
//! with its C half and `mira.h`) are carried inside the engine and written out beside what
//! is built, so a game's folder needs none of them, and no shell is asked to do anything.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
};

use anyhow::Context;

use super::{library_file_name, stamp, Stamp};
use crate::app::App;

/// The bindings as they were when the engine was built.
const BINDINGS: [(&str, &str); 3] = [
    ("Mira.hs", include_str!("../../bindings/haskell/Mira.hs")),
    ("mira_hs.c", include_str!("../../bindings/haskell/cbits/mira_hs.c")),
    ("mira.h", include_str!("../../include/mira.h")),
];

/// The Meadow bindings, as a package of their own that a script's package depends on.
const MEADOW_BINDINGS: [(&str, &str); 2] = [
    ("Meadow.toml", include_str!("../../bindings/meadow/Meadow.toml")),
    ("src/Lib.mw", include_str!("../../bindings/meadow/src/Lib.mw")),
];

/// Writes a file only if it is not already so, so that what was built from it stays built.
fn written(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|there| there == text) {
        return Ok(());
    }
    std::fs::write(path, text).map_err(|err| format!("can't write {}: {err}", path.display()))
}

/// A turn at building a script, held by a file that is there while the build is: whoever
/// makes the file has the turn, and takes the file away when done.
struct Turn(PathBuf);

impl Turn {
    fn take(file: &Path) -> Result<Turn, String> {
        let began = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(file) {
                Ok(_) => return Ok(Turn(file.to_owned())),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Left by a program that was stopped in the middle of a build: no build
                    // takes as long as this.
                    let left = std::fs::metadata(file).and_then(|file| file.modified()).is_ok_and(|made| made.elapsed().is_ok_and(|age| age.as_secs() > 300));
                    if left || began.elapsed().as_secs() > 600 {
                        let _ = std::fs::remove_file(file);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(err) => return Err(format!("can't make {}: {err}", file.display())),
            }
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Builds a Haskell script into the plugin `name`, in the folder `out`, and says where the
/// library is, or why there is none: what the compiler said, as it said it.
///
/// What is built along the way is kept under `out/haskell/<name>`, so a script built again
/// takes a moment. The library is put in its place in one step, whole.
pub fn build_haskell(name: &str, source: &Path, out: &Path) -> Result<PathBuf, String> {
    let work = out.join("haskell").join(name);
    let bindings = out.join("haskell").join("bindings");
    for folder in [&work, &bindings] {
        std::fs::create_dir_all(folder).map_err(|err| format!("can't make {}: {err}", folder.display()))?;
    }
    // One build of a script at a time, though two programs have the same game open (the
    // engine app and the player, say): the second waits for the first.
    let _turn = Turn::take(&work.join("building"))?;
    for (file, text) in BINDINGS {
        written(&bindings.join(file), text)?;
    }
    // GHC writes a C file's object beside its source, so each script has the glue's source
    // to itself: two building at once would otherwise trip over each other.
    written(&work.join("mira_hs.c"), BINDINGS[1].1)?;
    let library = library_file_name(name);
    let mut ghc = Command::new("ghc");
    ghc.args(["-O1", "-shared", "-threaded", "-flink-rts", "-Wall", "-Wno-missing-signatures"]);
    // One GHC runtime is shared between every Haskell script and every version of one that
    // is loaded again. Not on Windows, where GHC has none to share: each carries its own.
    if !cfg!(windows) {
        ghc.args(["-dynamic", "-fPIC"]);
    }
    let output = ghc
        .arg(format!("-I{}", bindings.display()))
        .arg(format!("-i{}", bindings.display()))
        .arg("-outputdir")
        .arg(&work)
        .arg(work.join("mira_hs.c"))
        .arg(source)
        .arg("-o")
        .arg(work.join(&library))
        .output()
        .map_err(|err| format!("GHC could not be run ({err}); a Haskell script needs `ghc` on the path"))?;
    if !output.status.success() {
        let said = [output.stdout, output.stderr].concat();
        return Err(String::from_utf8_lossy(&said).trim().to_owned());
    }
    let built = out.join(&library);
    std::fs::rename(work.join(&library), &built).map_err(|err| format!("can't put {} in its place: {err}", built.display()))?;
    Ok(built)
}

/// Builds a Meadow script into the plugin `name`, in the folder `out`, as
/// [`build_haskell`] does a Haskell one.
///
/// The script is made a package of its own under `out/meadow/<name>`, with the bindings
/// beside it as the package it depends on, `Mira`, and MeadowBoot builds it.
pub fn build_meadow(name: &str, source: &Path, out: &Path) -> Result<PathBuf, String> {
    let meadow = out.join("meadow");
    let package = meadow.join(name);
    let bindings = meadow.join("Mira");
    for folder in [package.join("src"), bindings.join("src")] {
        std::fs::create_dir_all(&folder).map_err(|err| format!("can't make {}: {err}", folder.display()))?;
    }
    let _turn = Turn::take(&package.join("building"))?;
    for (file, text) in MEADOW_BINDINGS {
        written(&bindings.join(file), text)?;
    }
    // A package's name is a module's, so it begins with a capital.
    let title: String = name.chars().take(1).flat_map(char::to_uppercase).chain(name.chars().skip(1)).collect();
    written(&package.join("Meadow.toml"), &format!("[package]\nname = \"{title}\"\nversion = \"0.1.0\"\n\n[dependencies]\nMira = {{ path = \"../Mira\" }}\n"))?;
    let text = std::fs::read_to_string(source).map_err(|err| format!("can't read {}: {err}", source.display()))?;
    written(&package.join("src").join("Main.mw"), &text)?;

    let boot = std::env::var_os("MIRA_MEADOWBOOT").map_or_else(|| PathBuf::from("meadowboot"), PathBuf::from);
    let began = std::time::SystemTime::now();
    let output = Command::new(&boot)
        .arg("build")
        .arg(&package)
        .output()
        .map_err(|err| format!("MeadowBoot could not be run ({err}); a Meadow script needs it: `meadowboot` on the path, or MIRA_MEADOWBOOT saying where it is"))?;
    if !output.status.success() {
        let said = [output.stdout, output.stderr].concat();
        return Err(String::from_utf8_lossy(&said).trim().to_owned());
    }
    // Where MeadowBoot puts the library: named after the package, or as `program` before it
    // learnt to. The one it has just written is the one wanted.
    let silo = |within: &Path| within.join("native").join("silo");
    let candidates = [
        silo(&package.join("target").join("debug")).join(library_file_name(&title)),
        silo(&package.join("target").join("debug").join("link").join("target").join("release")).join(library_file_name("program")),
    ];
    let made = candidates
        .iter()
        .filter_map(|path| Some((std::fs::metadata(path).ok()?.modified().ok()?, path)))
        .filter(|(when, _)| *when >= began - std::time::Duration::from_secs(1))
        .max_by_key(|(when, _)| *when)
        .map(|(_, path)| path.clone())
        .ok_or_else(|| format!("MeadowBoot said it built {name} and no library was found where it puts one"))?;
    let built = out.join(library_file_name(name));
    let beside = out.join(format!("{}.writing", library_file_name(name)));
    std::fs::copy(&made, &beside).map_err(|err| format!("can't copy {}: {err}", made.display()))?;
    std::fs::rename(&beside, &built).map_err(|err| format!("can't put {} in its place: {err}", built.display()))?;
    Ok(built)
}

/// Builds a script by what it is written in, as its file says.
fn build(name: &str, source: &Path, out: &Path) -> Result<PathBuf, String> {
    match source.extension().and_then(|ends| ends.to_str()) {
        Some("mw") => build_meadow(name, source, out),
        _ => build_haskell(name, source, out),
    }
}

/// A script the engine keeps built.
pub(super) struct Script {
    name: String,
    source: PathBuf,
    out: PathBuf,
    /// The source as it was when it was last built, or last began to be.
    built: Option<Stamp>,
    /// A build under way, which says how it went when it is done.
    building: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    /// Why the script as it stands could not be built.
    problem: Option<String>,
}

impl App {
    /// Builds every script in a folder (Meadow, `*.mw`, and Haskell, `*.hs`) and loads each
    /// as a plugin, named after its file. What is built goes in `.mira/build` inside the folder. A script that
    /// is saved afterwards is built again and taken up while the game runs.
    ///
    /// Scripts that do not build are logged with what the compiler said, and are built and
    /// loaded once they do. It is an error only if the folder cannot be read.
    pub fn load_scripts(&mut self, folder: impl AsRef<Path>) -> anyhow::Result<&mut Self> {
        let folder = folder.as_ref();
        let mut sources: Vec<PathBuf> = std::fs::read_dir(folder)
            .with_context(|| format!("reading the scripts in {}", folder.display()))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ends| ends == "hs" || ends == "mw"))
            .collect();
        sources.sort();
        for source in sources {
            self.load_script(source)?;
        }
        Ok(self)
    }

    /// Builds one script and loads it as a plugin named after its file, and keeps
    /// it built as [`load_scripts`](Self::load_scripts) does.
    pub fn load_script(&mut self, source: impl AsRef<Path>) -> anyhow::Result<&mut Self> {
        let source = source.as_ref().to_owned();
        let name = source.file_stem().and_then(|stem| stem.to_str()).context("a script's file has no name")?.to_lowercase();
        let out = source.parent().unwrap_or(Path::new(".")).join(".mira").join("build");
        let mut script = Script { name, built: stamp(&source), source, out, building: None, problem: None };
        match build(&script.name, &script.source, &script.out) {
            Ok(library) => {
                self.load_native_plugin(&library)?;
            }
            Err(said) => {
                log::error!("the script {} does not build:\n{said}", script.source.display());
                script.problem = Some(said);
            }
        }
        self.native.scripts.push(script);
        Ok(self)
    }

    /// The scripts that do not build as they stand, each with what the compiler said.
    pub fn script_problems(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.native.scripts.iter().filter_map(|script| Some((script.source.as_path(), script.problem.as_deref()?)))
    }

    /// Whether any script is being built at this moment.
    pub fn scripts_building(&self) -> bool {
        self.native.scripts.iter().any(|script| script.building.is_some())
    }

    /// Takes in the builds that have finished, and begins one for each script saved since it
    /// was last built. Once a frame.
    pub(super) fn keep_scripts_built(&mut self) {
        let mut scripts = std::mem::take(&mut self.native.scripts);
        for script in &mut scripts {
            if let Some(finished) = script.building.as_ref().and_then(|building| building.try_recv().ok()) {
                script.building = None;
                match finished {
                    Ok(library) => {
                        if script.problem.take().is_some() {
                            log::info!("the script {} builds again", script.source.display());
                        }
                        // One that never built before is not loaded yet; one that was is
                        // seen to have changed and loaded again by the plugins' own watch.
                        let loaded = self.native.plugins.iter().any(|plugin| plugin.source == library);
                        if !loaded {
                            if let Err(err) = self.load_native_plugin(&library) {
                                log::error!("the script {} was built and could not be loaded: {err:#}", script.source.display());
                            }
                        }
                    }
                    Err(said) => {
                        log::error!("the script {} does not build; the game goes on with the last that did:\n{said}", script.source.display());
                        script.problem = Some(said);
                    }
                }
            }
            let now = stamp(&script.source);
            if script.building.is_none() && now.is_some() && now != script.built {
                script.built = now;
                let (done, building) = mpsc::channel();
                let (name, source, out) = (script.name.clone(), script.source.clone(), script.out.clone());
                let began = std::thread::Builder::new().name(format!("building {name}")).spawn(move || {
                    let _ = done.send(build(&name, &source, &out));
                });
                match began {
                    Ok(_) => script.building = Some(building),
                    Err(err) => log::error!("no thread to build {} on: {err}", script.source.display()),
                }
            }
        }
        // Scripts loaded while these were looked at (there are none today) are kept too.
        scripts.append(&mut self.native.scripts);
        self.native.scripts = scripts;
    }
}
