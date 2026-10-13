//! Shaders made of modules.
//!
//! A WGSL file may take in another by name, and keep parts of itself for some builds only:
//!
//! ```wgsl
//! #import mira::pbr
//!
//! #ifdef RAY_TRACING
//! // ...
//! #else
//! // ...
//! #endif
//! ```
//!
//! [`ShaderLibrary`] holds the modules by name and puts a shader together from them: each
//! module once, however many ask for it, in the order they are first asked for. What is
//! defined decides which parts are kept, so one source gives a variant for each set of
//! definitions. `enable` lines, which WGSL wants before everything else, are moved to the top
//! from wherever a module has them.
//!
//! A directive is a line of its own: `#import name`, `#ifdef NAME`, `#ifndef NAME`, `#else`,
//! `#endif`. `#ifdef` may be nested.

use std::collections::{BTreeSet, HashMap};

use super::Shader;

/// The engine's lighting: the view, the sun and its shadows, sky light and haze, at group 0,
/// with ray-traced shadows and reflections where `RAY_TRACING` is defined.
pub const PBR: &str = "mira::pbr";
/// Defined for shaders built on a device that traces rays.
pub const RAY_TRACING: &str = "RAY_TRACING";
/// Defined where the scene is drawn with several samples a pixel, and its depth and what
/// moved are read as such.
pub const MSAA: &str = "MSAA";

/// Where a module's text comes from.
#[derive(Clone)]
enum Source {
    /// A file, read again when shaders are reloaded.
    File(Shader),
    Text(String),
}

impl Source {
    fn text(&self) -> String {
        match self {
            Source::File(shader) => shader.source(),
            Source::Text(text) => text.clone(),
        }
    }
}

/// Why a shader could not be put together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeError {
    /// The module the trouble is in; the shader itself is `"the shader"`.
    pub module: String,
    /// The line of that module, counted from one.
    pub line: usize,
    pub what: String,
}

impl std::fmt::Display for ComposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, line {}: {}", self.module, self.line, self.what)
    }
}

impl std::error::Error for ComposeError {}

/// Shader modules by name.
#[derive(Clone, Default)]
pub struct ShaderLibrary {
    modules: HashMap<String, Source>,
}

impl ShaderLibrary {
    /// A library holding the engine's own modules.
    pub fn engine() -> Self {
        let mut library = Self::default();
        library
            .add(PBR, crate::shader!("lighting.wgsl"))
            .add("mira::pbr::core", crate::shader!("pbr.wgsl"))
            .add("mira::pbr::traced", crate::shader!("rt_on.wgsl"))
            .add("mira::pbr::untraced", crate::shader!("rt_off.wgsl"));
        library
    }

    /// Adds a module from a file, to be read again when shaders are reloaded. One of the
    /// same name is replaced.
    pub fn add(&mut self, name: &str, shader: Shader) -> &mut Self {
        self.modules.insert(name.to_owned(), Source::File(shader));
        self
    }

    /// Adds a module from text.
    pub fn add_text(&mut self, name: &str, text: impl Into<String>) -> &mut Self {
        self.modules.insert(name.to_owned(), Source::Text(text.into()));
        self
    }

    pub fn contains(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    /// Puts a shader together: its imports taken in, the parts kept that `defines` asks
    /// for, and `enable` lines moved to the top.
    pub fn compose(&self, source: &str, defines: &[&str]) -> Result<String, ComposeError> {
        let mut composing = Composing { library: self, defines, taken: BTreeSet::new(), enables: vec![], body: String::new() };
        composing.take("the shader", source)?;
        let mut whole = String::new();
        for enable in &composing.enables {
            whole.push_str(enable);
            whole.push('\n');
        }
        whole.push_str(&composing.body);
        Ok(whole)
    }
}

struct Composing<'a> {
    library: &'a ShaderLibrary,
    defines: &'a [&'a str],
    /// The modules taken in so far: each goes in once.
    taken: BTreeSet<String>,
    enables: Vec<String>,
    body: String,
}

impl Composing<'_> {
    fn take(&mut self, module: &str, text: &str) -> Result<(), ComposeError> {
        let wrong = |line: usize, what: String| ComposeError { module: module.to_owned(), line: line + 1, what };
        // For each `#ifdef` open: whether its part is kept, whether the one around it is,
        // and whether its `#else` has been met.
        let mut open: Vec<(bool, bool, bool)> = vec![];
        for (at, line) in text.lines().enumerate() {
            let kept = open.last().is_none_or(|(kept, _, _)| *kept);
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                [how @ ("#ifdef" | "#ifndef"), name] => {
                    let defined = self.defines.contains(name);
                    open.push((kept && (defined == (*how == "#ifdef")), kept, false));
                }
                ["#ifdef" | "#ifndef", ..] => return Err(wrong(at, "`#ifdef` takes one name".into())),
                ["#else"] => match open.last_mut() {
                    Some((_, _, true)) => return Err(wrong(at, "a second `#else` for one `#ifdef`".into())),
                    Some((kept, around, other)) => (*kept, *other) = (*around && !*kept, true),
                    None => return Err(wrong(at, "`#else` with no `#ifdef` before it".into())),
                },
                ["#endif"] => {
                    if open.pop().is_none() {
                        return Err(wrong(at, "`#endif` with no `#ifdef` before it".into()));
                    }
                }
                _ if !kept => {}
                ["#import", name] => {
                    if self.taken.insert((*name).to_owned()) {
                        let Some(source) = self.library.modules.get(*name) else {
                            return Err(wrong(at, format!("there is no shader module called `{name}`")));
                        };
                        self.body.push_str(&format!("// {name}\n"));
                        self.take(name, &source.text())?;
                    }
                }
                ["#import", ..] => return Err(wrong(at, "`#import` takes one name".into())),
                ["enable", ..] if line.trim_end().ends_with(';') => {
                    let enable = line.trim().to_owned();
                    if !self.enables.contains(&enable) {
                        self.enables.push(enable);
                    }
                }
                _ => {
                    self.body.push_str(line);
                    self.body.push('\n');
                }
            }
        }
        match open.is_empty() {
            true => Ok(()),
            false => Err(wrong(text.lines().count().saturating_sub(1), "an `#ifdef` is never closed with `#endif`".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> ShaderLibrary {
        let mut library = ShaderLibrary::default();
        library
            .add_text("light", "#import maths\nfn lit() -> f32 { return half(2.0); }")
            .add_text("maths", "fn half(x: f32) -> f32 { return x * 0.5; }")
            .add_text("rays", "enable wgpu_ray_query;\n#import maths\nfn trace() {}");
        library
    }

    #[test]
    fn a_module_is_taken_in_once_however_many_ask_for_it() {
        let whole = library().compose("#import light\n#import maths\nfn main() {}", &[]).unwrap();
        assert_eq!(whole.matches("fn half").count(), 1);
        // What a module needs comes before it, and the shader's own text last.
        let at = |what: &str| whole.find(what).unwrap();
        assert!(at("fn half") < at("fn lit") && at("fn lit") < at("fn main"));
    }

    #[test]
    fn what_is_defined_decides_what_is_kept() {
        let source = "#ifdef RAYS\n#import rays\nconst traced = true;\n#else\nconst traced = false;\n#endif\nfn main() {}";
        let library = library();
        let with = library.compose(source, &["RAYS"]).unwrap();
        assert!(with.contains("traced = true") && with.contains("fn trace") && !with.contains("traced = false"));
        let without = library.compose(source, &[]).unwrap();
        assert!(without.contains("traced = false") && !without.contains("fn trace") && !without.contains("enable"));
        let not = library.compose("#ifndef RAYS\nconst plain = 1;\n#endif", &["RAYS"]).unwrap();
        assert!(!not.contains("plain"));
    }

    #[test]
    fn a_part_inside_one_that_is_dropped_is_dropped_whatever_it_asks() {
        let source = "#ifdef A\n#ifdef B\nconst both = 1;\n#else\nconst only_a = 1;\n#endif\n#else\nconst no_a = 1;\n#endif";
        let library = ShaderLibrary::default();
        let kept = |defines: &[&str]| library.compose(source, defines).unwrap();
        assert!(kept(&["A", "B"]).contains("both") && !kept(&["A", "B"]).contains("only_a"));
        assert!(kept(&["A"]).contains("only_a") && !kept(&["A"]).contains("no_a"));
        // Without A, neither inner part is kept, the `#else` of the inner one included.
        assert_eq!(kept(&["B"]).trim(), "const no_a = 1;");
    }

    #[test]
    fn enable_lines_go_to_the_top_once() {
        let whole = library().compose("fn main() {}\n#import rays\nenable wgpu_ray_query;", &[]).unwrap();
        assert!(whole.starts_with("enable wgpu_ray_query;\n"));
        assert_eq!(whole.matches("enable").count(), 1);
    }

    #[test]
    fn mistakes_say_where_they_are() {
        let library = library();
        let wrong = |source: &str| library.compose(source, &[]).unwrap_err();
        let missing = wrong("fn main() {}\n#import nothing");
        assert_eq!((missing.module.as_str(), missing.line), ("the shader", 2));
        assert!(missing.what.contains("nothing"));
        assert!(wrong("#endif").what.contains("no `#ifdef`"));
        assert!(wrong("#else").what.contains("no `#ifdef`"));
        assert!(wrong("#ifdef A\n#else\n#else\n#endif").what.contains("second"));
        assert!(wrong("#ifdef A\nfn main() {}").what.contains("never closed"));
        // A mistake in a module is told as that module's.
        let mut library = library;
        library.add_text("broken", "fn ok() {}\n#endif");
        let inside = library.compose("#import broken", &[]).unwrap_err();
        assert_eq!((inside.module.as_str(), inside.line), ("broken", 2));
    }

    /// No one machine builds both: a device either traces rays or does not. So both are
    /// read here, by the same reader the device uses.
    #[test]
    fn every_engine_shader_that_imports_the_lighting_reads_as_wgsl_both_ways() {
        let library = ShaderLibrary::engine();
        let shaders = [
            crate::shader!("mesh.wgsl"),
            crate::shader!("sky.wgsl"),
            crate::shader!("volumetric.wgsl"),
            crate::shader!("../voxel/voxel.wgsl"),
        ];
        for shader in shaders {
            for defines in [&[][..], &[RAY_TRACING][..]] {
                let whole = library.compose(shader.embedded, defines).unwrap_or_else(|mistake| panic!("{}: {mistake}", shader.name));
                if let Err(wrong) = wgpu::naga::front::wgsl::parse_str(&whole) {
                    panic!("{} with {defines:?}:\n{}", shader.name, wrong.emit_to_string(&whole));
                }
            }
        }
        // The probes are baked only where rays are traced.
        let probes = library.compose(crate::shader!("probes.wgsl").embedded, &[RAY_TRACING]).unwrap();
        if let Err(wrong) = wgpu::naga::front::wgsl::parse_str(&probes) {
            panic!("probes.wgsl:\n{}", wrong.emit_to_string(&probes));
        }
    }

    /// A surface shader is the mesh shader with the surface's module taken in, and no one
    /// machine builds it both traced and not.
    #[test]
    fn the_mesh_shader_reads_as_wgsl_with_a_surface_shader_taken_in() {
        let mut library = ShaderLibrary::engine();
        library.add_text(
            "game::glow",
            "fn surface(s: Surface, in: VertexOutput) -> Surface {\n    var out = s;\n    out.emissive = in.color.rgb * (0.5 + 0.5 * sin(time()));\n    return out;\n}",
        );
        let source = format!("{}\n#import game::glow\n", crate::shader!("mesh.wgsl").embedded);
        for defines in [&["SURFACE"][..], &["SURFACE", RAY_TRACING][..]] {
            let whole = library.compose(&source, defines).unwrap();
            assert!(whole.contains("s = surface(s, in);"));
            if let Err(wrong) = wgpu::naga::front::wgsl::parse_str(&whole) {
                panic!("with {defines:?}:\n{}", wrong.emit_to_string(&whole));
            }
        }
        // Without one, the mesh shader does not ask for it.
        assert!(!library.compose(crate::shader!("mesh.wgsl").embedded, &[]).unwrap().contains("surface(s, in)"));
    }

    #[test]
    fn the_engines_lighting_comes_with_and_without_ray_tracing() {        let library = ShaderLibrary::engine();
        let plain = library.compose("#import mira::pbr", &[]).unwrap();
        let traced = library.compose("#import mira::pbr", &[RAY_TRACING]).unwrap();
        let directive = |text: &str| text.lines().any(|line| line.trim_start().starts_with('#'));
        assert!(!plain.starts_with("enable ") && !directive(&plain));
        assert!(traced.starts_with("enable ") && !directive(&traced));
        let scene = "var scene: acceleration_structure";
        assert!(traced.contains(scene) && !plain.contains(scene));
    }
}
