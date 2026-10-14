//! Scripts: plugins the engine builds itself, from their source.
//!
//! A game can be a folder of scripts and nothing else. [`App::load_scripts`] takes each
//! Haskell file in a folder, builds it into a plugin, and loads it; from then on a script
//! that is saved is built again, behind the game, and the game goes on with the new one as
//! soon as it is ready. One that does not build is told of, with what the compiler said,
//! and the game carries on with the last that did.
//!
//! What is needed to build a script is GHC, on the path. The bindings a script is built
//! with ([`Mira.hs`](../../../bindings/haskell/Mira.hs), their C half, and `mira.h`) are
//! carried inside the engine and written out beside what is built, so a game's folder needs
//! none of them, and no shell is asked to do anything.

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

/// Writes a file only if it is not already so, so that what was built from it stays built.
fn written(path: &Path, text: &str) -> Result<(), String> {
    if std::fs::read_to_string(path).is_ok_and(|there| there == text) {
        return Ok(());
    }
    std::fs::write(path, text).map_err(|err| format!("can't write {}: {err}", path.display()))
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
    /// Builds every Haskell script (`*.hs`) in a folder and loads each as a plugin, named
    /// after its file. What is built goes in `.mira/build` inside the folder. A script that
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
            .filter(|path| path.extension().is_some_and(|ends| ends == "hs"))
            .collect();
        sources.sort();
        for source in sources {
            self.load_script(source)?;
        }
        Ok(self)
    }

    /// Builds one Haskell script and loads it as a plugin named after its file, and keeps
    /// it built as [`load_scripts`](Self::load_scripts) does.
    pub fn load_script(&mut self, source: impl AsRef<Path>) -> anyhow::Result<&mut Self> {
        let source = source.as_ref().to_owned();
        let name = source.file_stem().and_then(|stem| stem.to_str()).context("a script's file has no name")?.to_lowercase();
        let out = source.parent().unwrap_or(Path::new(".")).join(".mira").join("build");
        let mut script = Script { name, built: stamp(&source), source, out, building: None, problem: None };
        match build_haskell(&script.name, &script.source, &script.out) {
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
                    let _ = done.send(build_haskell(&name, &source, &out));
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
