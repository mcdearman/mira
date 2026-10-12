//! The frame as a graph of passes.
//!
//! A pass says what it reads and what it writes, by name, and the graph works out the order
//! from that: a pass runs after the ones that write what it reads. Passes that write the same
//! thing run in the order they were added, each going on from what the one before left, and
//! one that only reads a thing runs after everything that writes it.
//!
//! A plugin adds a pass of its own without knowing where in the frame it falls:
//!
//! ```ignore
//! app.world.resource_mut::<RenderGraph>().add(
//!     Pass::new("outline", outline).reads(&[slots::DEPTH]).writes(&[slots::SCENE]),
//! );
//! ```
//!
//! The graph also makes the targets it is asked for ([`RenderGraph::target`]), at the size of
//! the frame being drawn, and hands every pass the views by name ([`PassContext::view`]).

use std::collections::{HashMap, HashSet};

use crate::ecs::World;

/// The name of something passes read and write: a target, or anything else one pass makes
/// ready for another.
pub type Slot = &'static str;

/// The names of what the engine's own passes read and write.
pub mod slots {
    use super::Slot;

    /// Vertices of skinned meshes, posed for this frame.
    pub const SKINNED: Slot = "skinned";
    /// The scene as rays are traced against it.
    pub const TRACED_SCENE: Slot = "traced scene";
    /// The sun's shadow cascades.
    pub const SHADOW_MAPS: Slot = "shadow maps";
    /// The scene's colour, lit, before anything is done to the picture as a whole.
    pub const HDR: Slot = "hdr";
    /// The multisampled target `HDR` is resolved from, at the end of the main pass.
    pub const HDR_MSAA: Slot = "hdr msaa";
    /// The scene's depth.
    pub const DEPTH: Slot = "depth";
    /// How far each thing that moved has come across the picture since the last frame.
    pub const MOTION: Slot = "motion";
    /// The scene's colour as the last passes take it: `HDR`, or that blended over time.
    pub const SCENE: Slot = "scene";
    /// The picture that is shown: the window's frame, or the texture a host shows.
    pub const FRAME: Slot = "frame";
}

/// What a pass does: record its work into the frame's encoder.
pub type PassFn = fn(&mut World, &mut PassContext);

/// What a pass is given: where to record its work, and the frame's targets by name.
pub struct PassContext {
    /// Where the frame's work is recorded. Passes record into it one after another, and it
    /// is sent to the graphics card when the last has run.
    pub encoder: wgpu::CommandEncoder,
    device: wgpu::Device,
    queue: wgpu::Queue,
    views: HashMap<Slot, wgpu::TextureView>,
    notes: HashSet<&'static str>,
}

impl PassContext {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue, views: HashMap<Slot, wgpu::TextureView>) -> Self {
        Self { encoder: Self::recorder(device), device: device.clone(), queue: queue.clone(), views, notes: HashSet::new() }
    }

    fn recorder(device: &wgpu::Device) -> wgpu::CommandEncoder {
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") })
    }

    /// Sends what has been recorded so far to the graphics card, and starts recording
    /// afresh. For a pass that submits work of its own, which would otherwise reach the card
    /// before the passes that ran ahead of it.
    pub fn flush(&mut self) {
        let recorded = std::mem::replace(&mut self.encoder, Self::recorder(&self.device));
        self.queue.submit([recorded.finish()]);
    }

    /// Sends the frame's work to the graphics card.
    pub(crate) fn finish(self) {
        self.queue.submit([self.encoder.finish()]);
    }

    /// The view of a target. A pass may ask for what it declared it reads or writes; asking
    /// for one that nothing has provided is a mistake in the graph, and says so.
    pub fn view(&self, slot: Slot) -> &wgpu::TextureView {
        self.views.get(slot).unwrap_or_else(|| panic!("no pass has provided the target `{slot}`"))
    }

    /// The view of a target, if there is one this frame.
    pub fn get_view(&self, slot: Slot) -> Option<&wgpu::TextureView> {
        self.views.get(slot)
    }

    /// Says where a slot's picture is from here on: for a pass whose result is a view it
    /// owns, as the blend over time's is.
    pub fn set_view(&mut self, slot: Slot, view: wgpu::TextureView) {
        self.views.insert(slot, view);
    }

    /// Leaves word for the passes that follow, in this frame only.
    pub fn note(&mut self, word: &'static str) {
        self.notes.insert(word);
    }

    pub fn noted(&self, word: &'static str) -> bool {
        self.notes.contains(word)
    }
}

/// One step of the frame, and what it reads and writes.
#[derive(Clone)]
pub struct Pass {
    name: &'static str,
    reads: Vec<Slot>,
    writes: Vec<Slot>,
    after: Vec<&'static str>,
    before: Vec<&'static str>,
    run: PassFn,
}

impl Pass {
    pub fn new(name: &'static str, run: PassFn) -> Self {
        Self { name, reads: vec![], writes: vec![], after: vec![], before: vec![], run }
    }

    /// What the pass needs to have been written before it runs.
    pub fn reads(mut self, slots: &[Slot]) -> Self {
        self.reads.extend_from_slice(slots);
        self
    }

    /// What the pass writes. A pass that also reads the same thing goes on from what was
    /// there, after the passes added before it that write it.
    pub fn writes(mut self, slots: &[Slot]) -> Self {
        self.writes.extend_from_slice(slots);
        self
    }

    /// Runs after the pass of this name, where what they read and write does not say.
    pub fn after(mut self, pass: &'static str) -> Self {
        self.after.push(pass);
        self
    }

    /// Runs before the pass of this name.
    pub fn before(mut self, pass: &'static str) -> Self {
        self.before.push(pass);
        self
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// What a target the graph makes is like. Its size is the frame's, times `scale`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetDesc {
    pub format: wgpu::TextureFormat,
    pub samples: u32,
    pub usage: wgpu::TextureUsages,
    pub scale: f32,
}

impl TargetDesc {
    /// A target the size of the frame that passes draw into and read from.
    pub fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            samples: 1,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            scale: 1.0,
        }
    }

    pub fn samples(mut self, samples: u32) -> Self {
        self.samples = samples;
        self
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// What the target is used for, where it is not both drawn into and read from.
    pub fn usage(mut self, usage: wgpu::TextureUsages) -> Self {
        self.usage = usage;
        self
    }

    /// The size of the target for a frame of this size: never less than one pixel.
    pub fn size(&self, width: u32, height: u32) -> (u32, u32) {
        let of = |length: u32| ((length as f32 * self.scale).round() as u32).max(1);
        (of(width), of(height))
    }
}

/// A target the graph makes, and the textures made for it so far.
struct Target {
    slot: Slot,
    desc: TargetDesc,
    /// By size, most lately used last. More than one view of the scene may be drawn in a
    /// frame, each at its own size, so a few are kept.
    made: Vec<((u32, u32), wgpu::TextureView)>,
}

/// How many sizes of one target are kept before the least lately used is let go.
const SIZES_KEPT: usize = 8;

/// The passes were asked to run in a circle: each of these waits on the one before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle(pub Vec<&'static str>);

impl std::fmt::Display for Cycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "render passes wait on each other in a circle: {}", self.0.join(", "))
    }
}

/// A pass as [`RenderGraph::describe`] tells of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PassInfo {
    pub name: &'static str,
    pub reads: Vec<Slot>,
    pub writes: Vec<Slot>,
    /// How long the pass took to record its work the last time it ran, if it has run. This
    /// is the engine's time on the processor, not the graphics card's time drawing it.
    pub took: Option<std::time::Duration>,
    /// How long the graphics card took to do that work, the last time it said: none where
    /// the card cannot stamp the time between passes, or before it has first answered.
    pub on_card: Option<std::time::Duration>,
}

/// The passes of a frame and the targets made for them.
#[derive(Default)]
pub struct RenderGraph {
    passes: Vec<Pass>,
    targets: Vec<Target>,
    /// The order worked out for the passes as they stand.
    order: Option<Vec<usize>>,
    /// How long each pass took when the passes last ran.
    took: HashMap<&'static str, std::time::Duration>,
    /// How long the card took over each pass, the last time it said.
    on_card: HashMap<&'static str, std::time::Duration>,
}

impl RenderGraph {
    /// Adds a pass. One of the same name is replaced, and keeps its place among the passes
    /// that write the same things.
    pub fn add(&mut self, pass: Pass) -> &mut Self {
        match self.passes.iter_mut().find(|p| p.name == pass.name) {
            Some(there) => *there = pass,
            None => self.passes.push(pass),
        }
        self.order = None;
        self
    }

    /// Takes a pass out. Says whether there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.passes.len();
        self.passes.retain(|p| p.name != name);
        self.order = None;
        self.passes.len() != before
    }

    /// Has the graph make a target for a slot, the size of each frame drawn.
    pub fn target(&mut self, slot: Slot, desc: TargetDesc) -> &mut Self {
        match self.targets.iter_mut().find(|t| t.slot == slot) {
            Some(there) if there.desc == desc => {}
            Some(there) => {
                there.desc = desc;
                there.made.clear();
            }
            None => self.targets.push(Target { slot, desc, made: vec![] }),
        }
        self
    }

    /// The passes in the order they run, with what each reads and writes: for a debugger or
    /// an editor to show.
    pub fn describe(&mut self) -> Result<Vec<PassInfo>, Cycle> {
        let order = self.order()?;
        Ok(order
            .iter()
            .map(|&i| {
                let pass = &self.passes[i];
                PassInfo { name: pass.name, reads: pass.reads.clone(), writes: pass.writes.clone(), took: self.took.get(pass.name).copied(), on_card: self.on_card.get(pass.name).copied() }
            })
            .collect())
    }

    /// Notes how long each pass took, the last time the frame's passes were run.
    pub(crate) fn timed(&mut self, took: impl IntoIterator<Item = (&'static str, std::time::Duration)>) {
        self.took = took.into_iter().collect();
    }

    /// Notes how long the graphics card took over each pass of a frame it has drawn.
    pub(crate) fn timed_on_card(&mut self, took: impl IntoIterator<Item = (&'static str, std::time::Duration)>) {
        self.on_card = took.into_iter().collect();
    }

    /// How long the graphics card took over a whole frame's passes, the last time it said.
    pub fn on_card(&self) -> Option<std::time::Duration> {
        (!self.on_card.is_empty()).then(|| self.on_card.values().sum())
    }

    /// What each pass does, in the order they run.
    pub(crate) fn ordered(&mut self) -> Result<Vec<(&'static str, PassFn)>, Cycle> {
        let order = self.order()?;
        Ok(order.iter().map(|&i| (self.passes[i].name, self.passes[i].run)).collect())
    }

    fn order(&mut self) -> Result<Vec<usize>, Cycle> {
        if self.order.is_none() {
            self.order = Some(sort(&self.passes)?);
        }
        Ok(self.order.clone().unwrap())
    }

    /// The views of the graph's own targets for a frame of this size, made where there are
    /// none yet.
    pub(crate) fn views(&mut self, device: &wgpu::Device, width: u32, height: u32) -> HashMap<Slot, wgpu::TextureView> {
        let mut views = HashMap::new();
        for target in &mut self.targets {
            let size = target.desc.size(width, height);
            let view = match target.made.iter().position(|(made, _)| *made == size) {
                Some(at) => {
                    let found = target.made.remove(at);
                    target.made.push(found);
                    target.made.last().unwrap().1.clone()
                }
                None => {
                    let view = device
                        .create_texture(&wgpu::TextureDescriptor {
                            label: Some(target.slot),
                            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                            mip_level_count: 1,
                            sample_count: target.desc.samples,
                            dimension: wgpu::TextureDimension::D2,
                            format: target.desc.format,
                            usage: target.desc.usage,
                            view_formats: &[],
                        })
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    if target.made.len() == SIZES_KEPT {
                        target.made.remove(0);
                    }
                    target.made.push((size, view.clone()));
                    view
                }
            };
            views.insert(target.slot, view);
        }
        views
    }
}

/// The order to run passes in: each after what it waits on, and otherwise as they were added.
fn sort(passes: &[Pass]) -> Result<Vec<usize>, Cycle> {
    let count = passes.len();
    // `waits[b]` holds every pass that must have run before `b`.
    let mut waits: Vec<HashSet<usize>> = vec![HashSet::new(); count];
    let named = |name: &str| passes.iter().position(|p| p.name == name);

    let mut slots: Vec<Slot> = passes.iter().flat_map(|p| p.reads.iter().chain(&p.writes).copied()).collect();
    slots.sort_unstable();
    slots.dedup();
    for slot in slots {
        let writers: Vec<usize> = (0..count).filter(|&i| passes[i].writes.contains(&slot)).collect();
        // Those that write a thing go in the order they were added.
        for pair in writers.windows(2) {
            waits[pair[1]].insert(pair[0]);
        }
        // Those that only read it wait for the last of them.
        if let Some(&last) = writers.last() {
            for (reader, pass) in passes.iter().enumerate() {
                if pass.reads.contains(&slot) && !pass.writes.contains(&slot) {
                    waits[reader].insert(last);
                }
            }
        }
    }
    for (at, pass) in passes.iter().enumerate() {
        for other in pass.after.iter().filter_map(|name| named(name)) {
            waits[at].insert(other);
        }
        for other in pass.before.iter().filter_map(|name| named(name)) {
            waits[other].insert(at);
        }
    }

    let mut order = Vec::with_capacity(count);
    let mut done = vec![false; count];
    while order.len() < count {
        // The first pass added, of those with nothing left to wait for.
        let Some(next) = (0..count).find(|&i| !done[i] && waits[i].iter().all(|&w| done[w])) else {
            return Err(Cycle(circle(passes, &waits, &done)));
        };
        done[next] = true;
        order.push(next);
    }
    Ok(order)
}

/// A circle among the passes that could not be placed: follow what each waits on until one
/// comes round again.
fn circle(passes: &[Pass], waits: &[HashSet<usize>], done: &[bool]) -> Vec<&'static str> {
    let mut at = (0..passes.len()).find(|&i| !done[i]).expect("a pass was left over");
    let mut seen = vec![];
    while !seen.contains(&at) {
        seen.push(at);
        at = *waits[at].iter().filter(|&&w| !done[w]).min().expect("a pass left over waits on another");
    }
    let from = seen.iter().position(|&p| p == at).unwrap();
    seen[from..].iter().rev().map(|&i| passes[i].name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: &mut World, _: &mut PassContext) {}

    fn names(graph: &mut RenderGraph) -> Vec<&'static str> {
        graph.describe().unwrap().into_iter().map(|pass| pass.name).collect()
    }

    #[test]
    fn a_pass_runs_after_what_writes_what_it_reads() {
        let mut graph = RenderGraph::default();
        // Added backwards, to show the order is not the order of adding.
        graph
            .add(Pass::new("post", nothing).reads(&["scene"]).writes(&["frame"]))
            .add(Pass::new("blend", nothing).reads(&["hdr", "depth"]).writes(&["scene"]))
            .add(Pass::new("main", nothing).reads(&["shadows"]).writes(&["hdr", "depth"]))
            .add(Pass::new("shadows", nothing).writes(&["shadows"]));
        assert_eq!(names(&mut graph), ["shadows", "main", "blend", "post"]);
    }

    #[test]
    fn passes_that_write_the_same_thing_keep_the_order_they_were_added_in() {
        let mut graph = RenderGraph::default();
        graph
            .add(Pass::new("main", nothing).writes(&["hdr"]))
            .add(Pass::new("shafts", nothing).reads(&["hdr"]).writes(&["hdr"]))
            .add(Pass::new("blend", nothing).reads(&["hdr"]).writes(&["scene"]))
            .add(Pass::new("glow", nothing).reads(&["hdr"]).writes(&["hdr"]));
        // The one that only reads the colour waits for everything that writes it, the pass
        // added after it too.
        assert_eq!(names(&mut graph), ["main", "shafts", "glow", "blend"]);
    }

    #[test]
    fn passes_with_nothing_between_them_run_as_added_unless_told() {
        let mut graph = RenderGraph::default();
        graph.add(Pass::new("one", nothing)).add(Pass::new("two", nothing)).add(Pass::new("three", nothing));
        assert_eq!(names(&mut graph), ["one", "two", "three"]);
        graph.add(Pass::new("three", nothing).before("one"));
        assert_eq!(names(&mut graph), ["two", "three", "one"]);
        graph.add(Pass::new("two", nothing).after("one"));
        assert_eq!(names(&mut graph), ["three", "one", "two"]);
        // A pass named that is not there holds nothing up.
        graph.add(Pass::new("two", nothing).after("nobody"));
        assert_eq!(names(&mut graph), ["two", "three", "one"]);
    }

    #[test]
    fn a_pass_is_replaced_by_one_of_its_name_and_can_be_taken_out() {
        let mut graph = RenderGraph::default();
        graph.add(Pass::new("a", nothing).writes(&["x"])).add(Pass::new("b", nothing).reads(&["x"]));
        graph.add(Pass::new("a", nothing).reads(&["y"]).writes(&["x"]));
        assert_eq!(graph.describe().unwrap()[0], PassInfo { name: "a", reads: vec!["y"], writes: vec!["x"], took: None, on_card: None });
        graph.timed([("a", std::time::Duration::from_micros(40))]);
        assert_eq!(graph.describe().unwrap()[0].took, Some(std::time::Duration::from_micros(40)));
        assert_eq!(graph.on_card(), None);
        graph.timed_on_card([("a", std::time::Duration::from_micros(900)), ("b", std::time::Duration::from_micros(100))]);
        assert_eq!(graph.describe().unwrap()[0].on_card, Some(std::time::Duration::from_micros(900)));
        assert_eq!(graph.on_card(), Some(std::time::Duration::from_millis(1)));
        assert!(graph.remove("a"));
        assert!(!graph.remove("a"));
        assert_eq!(names(&mut graph), ["b"]);
    }

    #[test]
    fn a_circle_is_refused_and_named() {
        let mut graph = RenderGraph::default();
        graph
            .add(Pass::new("first", nothing).writes(&["a"]))
            .add(Pass::new("there", nothing).reads(&["b"]).writes(&["c"]))
            .add(Pass::new("back", nothing).reads(&["c"]).writes(&["b"]));
        let Err(Cycle(circle)) = graph.describe() else { panic!("a circle was put in order") };
        assert_eq!(circle.len(), 2);
        assert!(circle.contains(&"there") && circle.contains(&"back"));
        // Taken apart, the rest is in order again.
        graph.remove("back");
        assert_eq!(names(&mut graph), ["first", "there"]);
    }

    #[test]
    fn the_engines_own_frame_comes_out_in_the_order_it_was_written_in() {
        let mut graph = super::super::frame_graph();
        assert_eq!(
            names(&mut graph),
            ["skinning", "ray tracing", "shadows", "main", "light shafts", "motion", "blend over time", "post", "debug lines", "overlays"]
        );
        // A pass a plugin adds, saying only what it reads and writes, falls between the
        // scene being drawn and the picture being finished.
        graph.add(Pass::new("outline", nothing).reads(&[slots::DEPTH, slots::SCENE]).writes(&[slots::SCENE]));
        let order = names(&mut graph);
        let at = |name| order.iter().position(|pass| *pass == name).unwrap();
        assert!(at("blend over time") < at("outline") && at("outline") < at("post"));
    }

    #[test]
    fn a_target_is_the_frames_size_times_its_scale() {
        let half = TargetDesc::new(wgpu::TextureFormat::Rgba16Float).scale(0.5);
        assert_eq!(half.size(1920, 1080), (960, 540));
        assert_eq!(half.size(1, 1), (1, 1));
        assert_eq!(TargetDesc::new(wgpu::TextureFormat::Rgba16Float).size(801, 600), (801, 600));
    }
}
