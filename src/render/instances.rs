//! The scene as the graphics card holds it: an instance for each mesh an entity draws, kept
//! from frame to frame and written only when its entity changes.
//!
//! An entity's instances are made when it first has something to draw and are brought up to
//! date when its transform, mesh, material or surface is written; nothing is done for the
//! entities that did not change. Each frame, each view only decides which instances it sees
//! (in view, near enough for their level of detail) and sends the card that list of numbers:
//! four bytes for each, where the whole instance was sent before.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use glam::{Mat4, Vec3, Vec4};

use super::{gpu::Gpu, motion, renderer::BatchKey, Material};
use crate::ecs::Entity;

/// Where an instance is, as the mesh shaders read it: the first three rows of its model
/// matrix, the fourth being 0 0 0 1. Must match `Place` in `mesh.wgsl` and
/// `mesh_shadow.wgsl`.
///
/// It is kept apart from the material's values because it is what changes when a thing
/// moves, and a thing that moves is sent again: this is a quarter of the whole instance.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PlaceRaw {
    rows: [[f32; 4]; 3],
}

impl PlaceRaw {
    fn new(model: Mat4) -> Self {
        let rows = model.transpose();
        Self { rows: [rows.x_axis.to_array(), rows.y_axis.to_array(), rows.z_axis.to_array()] }
    }
}

/// An instance's material, as the mesh shaders read it. Must match `Look` in `mesh.wgsl`
/// and `mesh_shadow.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LookRaw {
    color: [f32; 4],
    params: [f32; 4],
    emissive: [f32; 4],
    extra: [f32; 4],
    more: [f32; 4],
}

impl LookRaw {
    fn new(m: &Material) -> Self {
        Self {
            color: m.color.to_array(),
            params: [
                m.roughness,
                m.metallic,
                if m.normal_texture.is_some() { m.normal_strength } else { 0.0 },
                m.alpha_cutoff.unwrap_or(0.0),
            ],
            emissive: [m.emissive.r, m.emissive.g, m.emissive.b, m.translucency],
            extra: [
                if m.height_texture.is_some() { m.height_scale } else { 0.0 },
                m.subsurface,
                m.weathering,
                if m.decal { 1.0 } else { 0.0 },
            ],
            more: [m.puddles, m.waves, m.flow.x, m.flow.y],
        }
    }
}

/// What the card needs of an instance to decide for itself whether a view sees it (see
/// `cull.rs`). Must match `Sight` in `cull.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SightRaw {
    /// The mesh's bounding sphere in its own space. The radius is negative until the mesh
    /// is on the card and its size known, and the instance is then taken to be in view.
    center: [f32; 3],
    radius: f32,
    /// The distances the instance is seen over.
    seen: [f32; 2],
    /// Which batch it is drawn in: see [`Instances::batch_number`].
    batch: u32,
    /// Bit 0: there is such an instance. Bit 1: it casts a shadow.
    flags: u32,
}

/// One thing an entity draws: a mesh with a material, somewhere. An entity with levels of
/// detail has one for each level, each seen over its own range of distances.
#[derive(Clone, Copy)]
pub(crate) struct Part {
    pub key: BatchKey,
    pub model: Mat4,
    pub material: Material,
    /// How far the camera may be from the entity for this part to be drawn: from, and up to.
    pub seen: (f32, f32),
    pub casts_shadow: bool,
}

/// A part as it is kept.
struct Slot {
    part: Part,
    /// Where the model was before it last moved.
    was: Mat4,
}

/// What deciding whether an instance is seen needs of it, and no more: every view reads this
/// of every instance every frame, so it is kept small and apart.
#[derive(Clone, Copy)]
struct Sight {
    /// Where the entity is, which its distance from the camera is measured to.
    origin: Vec3,
    /// The distances the part is seen over.
    seen: (f32, f32),
    /// The mesh's bounding sphere in the world. The radius is negative until the mesh is on
    /// the card and its size known.
    center: Vec3,
    radius: f32,
    /// The frame the instance last moved in.
    moved: u64,
    casts_shadow: bool,
    decal: bool,
}

impl Sight {
    fn of(part: &Part) -> Self {
        Self {
            origin: part.model.w_axis.truncate(),
            seen: part.seen,
            center: Vec3::ZERO,
            radius: -1.0,
            moved: 0,
            casts_shadow: part.casts_shadow,
            decal: part.material.decal,
        }
    }
}

/// What one view sees of the scene: the instances it draws, by batch, as ranges of the list
/// of instance numbers sent to the card.
#[derive(Default)]
pub(crate) struct Seen {
    /// Instance numbers: those of the view's batches, then those of each shadow cascade's.
    pub numbers: Vec<u32>,
    pub batches: Vec<(BatchKey, Range<u32>)>,
    /// For each shadow cascade, the batches of what casts a shadow into it.
    pub shadow_batches: Vec<Vec<(BatchKey, Range<u32>)>>,
    /// What is in view and moved this frame, for the blend over time.
    pub moved: Vec<motion::Moved>,
}

/// What a view is seen from, for deciding what it sees.
pub(crate) struct Viewpoint {
    pub view_proj: Mat4,
    pub camera: Vec3,
    /// What each shadow cascade's map takes in, as the matrix it is drawn with; none where
    /// no shadows are cast. A cascade's matrix reaches far enough toward the sun to hold what
    /// casts into it, so what lies outside it casts nothing there.
    pub cascades: Vec<Mat4>,
    /// How far from the camera a caster still matters.
    pub shadow_reach: f32,
    /// Whether frames are blended over time, which is what wants to know what moved.
    pub blended: bool,
}

/// A part in the traced scene: what rays are traced against.
pub(crate) struct Traced<'a> {
    pub mesh: u32,
    pub model: Mat4,
    pub material: &'a Material,
}

/// Every instance there is, and its copy on the card.
#[derive(Default)]
pub(crate) struct Instances {
    slots: Vec<Option<Slot>>,
    free: Vec<u32>,
    of: HashMap<Entity, Vec<u32>>,
    /// The instances of each batch, in the order batches are drawn.
    batches: BTreeMap<BatchKey, Vec<u32>>,
    /// What deciding whether each instance is seen needs of it.
    sight: Vec<Sight>,
    /// What the card holds for each instance, where it is and how it looks, kept here too so
    /// that a stretch of them can be sent again without working any of it out.
    on_card: Vec<PlaceRaw>,
    looks: Vec<LookRaw>,
    /// Instances moved, and instances whose look changed, since they were last sent.
    stale: Vec<u32>,
    stale_looks: Vec<u32>,
    /// What the card is told of each instance to decide whether it is seen, the mesh whose
    /// size that holds (or none yet), and the instances it is to be told of again.
    card_sights: Vec<SightRaw>,
    sized: Vec<u32>,
    stale_sights: Vec<u32>,
    /// Instances whose mesh was not on the card when they were last looked at.
    unsized_yet: Vec<u32>,
    sights_buffer: Option<wgpu::Buffer>,
    /// A number for every batch there has been, which it keeps: the card files what it sees
    /// under these.
    batch_numbers: HashMap<BatchKey, u32>,
    /// The instances that moved in the frame being brought up to date.
    moved_now: Vec<u32>,
    /// The frame entities were last brought up to date in.
    frame: u64,
    /// Whether the traced scene has asked what changed, and the instances that have since.
    traced: bool,
    for_tracing: Vec<u32>,
    /// Where the instances are and how they look: none until they are first sent.
    buffer: Option<wgpu::Buffer>,
    looks_buffer: Option<wgpu::Buffer>,
    /// How many instances the buffer has room for.
    room: usize,
    /// Counts the buffers made, so that what binds one knows when there is another.
    pub generation: u64,
}

impl Instances {
    fn make(device: &wgpu::Device, label: &str, size: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Where the instances are, on the card. They must have been sent once.
    pub fn buffer(&self) -> &wgpu::Buffer {
        self.buffer.as_ref().expect("instances are sent before they are drawn")
    }

    /// How the instances look, on the card. They must have been sent once.
    pub fn looks(&self) -> &wgpu::Buffer {
        self.looks_buffer.as_ref().expect("instances are sent before they are drawn")
    }

    /// What the card is told of the instances to decide what is seen. Sent once, as the
    /// others.
    pub fn sights(&self) -> &wgpu::Buffer {
        self.sights_buffer.as_ref().expect("instances are sent before they are drawn")
    }

    /// How many instances there are.
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    /// How many numbers have been given to instances, in use or free: the card goes through
    /// that many.
    pub fn numbers(&self) -> usize {
        self.slots.len()
    }

    /// How many numbers have been given to batches.
    pub fn batch_numbers(&self) -> usize {
        self.batch_numbers.len()
    }

    /// The batches in the order they are drawn, each with its number and how many instances
    /// it has.
    pub fn batches(&self) -> impl Iterator<Item = (BatchKey, u32, u32)> + '_ {
        self.batches.iter().map(|(key, numbers)| (*key, self.batch_numbers[key], numbers.len() as u32))
    }

    /// Says which frame it is, before entities are brought up to date in it.
    pub fn begin(&mut self, frame: u64) {
        self.frame = frame;
        self.moved_now.clear();
    }

    /// Brings what the card is told of an instance, to decide whether it is seen, up to
    /// date with its part.
    fn resight(&mut self, number: u32) {
        let at = number as usize;
        if self.card_sights.len() <= at {
            self.card_sights.resize(at + 1, bytemuck::Zeroable::zeroed());
            self.sized.resize(at + 1, u32::MAX);
        }
        let part = &self.slots[at].as_ref().expect("an instance to be sighted is there").part;
        let next = self.batch_numbers.len() as u32;
        let batch = *self.batch_numbers.entry(part.key).or_insert(next);
        let mut sight = SightRaw {
            center: self.card_sights[at].center,
            radius: self.card_sights[at].radius,
            seen: [part.seen.0, part.seen.1],
            batch,
            flags: 1 | (part.casts_shadow as u32) << 1,
        };
        if self.sized[at] != part.key.mesh() {
            // One whose radius is negative already is waiting for its size.
            if sight.radius >= 0.0 {
                self.unsized_yet.push(number);
            }
            (sight.center, sight.radius) = ([0.0; 3], -1.0);
            self.sized[at] = u32::MAX;
        }
        if std::mem::replace(&mut self.card_sights[at], sight) != sight {
            self.stale_sights.push(number);
        }
    }

    /// Gives the instances whose mesh has come onto the card since they were made its size.
    /// `bounds` is a mesh's bounding sphere in its own space, once it is there.
    pub fn size(&mut self, bounds: impl Fn(u32) -> Option<(Vec3, f32)>) {
        if self.unsized_yet.is_empty() {
            return;
        }
        let waiting = std::mem::take(&mut self.unsized_yet);
        for number in waiting {
            let at = number as usize;
            let Some(slot) = &self.slots[at] else { continue };
            // Let go of and taken again since, or told of a mesh that has its size: it is
            // not waiting, or is in the list a second time.
            if self.card_sights[at].radius >= 0.0 {
                continue;
            }
            match bounds(slot.part.key.mesh()) {
                Some((center, radius)) => {
                    (self.card_sights[at].center, self.card_sights[at].radius) = (center.to_array(), radius);
                    self.sized[at] = slot.part.key.mesh();
                    self.stale_sights.push(number);
                }
                None => self.unsized_yet.push(number),
            }
        }
    }

    /// What moved in the frame being brought up to date and is near enough to be drawn, for
    /// the blend over time, where the card decides what is seen and so does not say.
    pub fn moved_near(&self, camera: Vec3) -> Vec<motion::Moved> {
        let mut moved = Vec::new();
        for &number in &self.moved_now {
            let (Some(slot), sight) = (&self.slots[number as usize], &self.sight[number as usize]) else { continue };
            let away = camera.distance(sight.origin);
            if sight.moved == self.frame && self.frame != 0 && !sight.decal && away >= sight.seen.0 && away < sight.seen.1 {
                moved.push(motion::Moved { mesh: slot.part.key.mesh(), model: slot.part.model, was: slot.was });
            }
        }
        moved
    }

    /// Sets what an entity draws. Its instances are kept where their batch has not changed,
    /// and written again only where something of them has.
    pub fn set(&mut self, entity: Entity, parts: &[Part]) {
        if parts.is_empty() {
            return self.forget(entity);
        }
        // Taken out to be worked on and put back, which a changed entity that keeps its
        // instances pays nothing for.
        let mut numbers = self.of.get_mut(&entity).map(std::mem::take).unwrap_or_default();
        // More parts or fewer than before: let go of the extra, take what is missing.
        while numbers.len() > parts.len() {
            let number = numbers.pop().unwrap();
            self.release(number);
        }
        for (at, part) in parts.iter().enumerate() {
            match numbers.get(at).copied() {
                Some(number) => self.update(number, *part),
                None => numbers.push(self.take(*part)),
            }
        }
        match self.of.get_mut(&entity) {
            Some(kept) => *kept = numbers,
            None => {
                self.of.insert(entity, numbers);
            }
        }
    }

    /// Moves what an entity draws to where the entity now is, for one of which nothing else
    /// has changed: its meshes, materials and batches are left as they are. Answers false
    /// for an entity that draws nothing yet, which is to be [`set`](Self::set).
    pub fn moved(&mut self, entity: Entity, model: Mat4) -> bool {
        let Some(numbers) = self.of.get(&entity) else { return false };
        for &number in numbers {
            let slot = self.slots[number as usize].as_mut().expect("an entity's instance is there");
            if slot.part.model == model {
                continue;
            }
            let sight = &mut self.sight[number as usize];
            if motion::has_moved(&model, &slot.part.model) {
                slot.was = slot.part.model;
                sight.moved = self.frame;
                self.moved_now.push(number);
            }
            // The mesh's place in the world is worked out again when it is next looked at.
            (sight.origin, sight.radius) = (model.w_axis.truncate(), -1.0);
            slot.part.model = model;
            self.on_card[number as usize] = PlaceRaw::new(model);
            self.stale.push(number);
            if self.traced {
                self.for_tracing.push(number);
            }
        }
        true
    }

    /// Lets go of everything an entity drew.
    pub fn forget(&mut self, entity: Entity) {
        for number in self.of.remove(&entity).unwrap_or_default() {
            self.release(number);
        }
    }

    fn take(&mut self, part: Part) -> u32 {
        let slot = Slot { part, was: part.model };
        let (place, look, sight) = (PlaceRaw::new(part.model), LookRaw::new(&part.material), Sight::of(&part));
        let number = match self.free.pop() {
            Some(number) => {
                self.slots[number as usize] = Some(slot);
                self.on_card[number as usize] = place;
                self.looks[number as usize] = look;
                self.sight[number as usize] = sight;
                number
            }
            None => {
                self.slots.push(Some(slot));
                self.on_card.push(place);
                self.looks.push(look);
                self.sight.push(sight);
                self.slots.len() as u32 - 1
            }
        };
        self.batches.entry(part.key).or_default().push(number);
        self.stale.push(number);
        self.stale_looks.push(number);
        self.resight(number);
        self.changed_for_tracing(number);
        number
    }

    fn update(&mut self, number: u32, part: Part) {
        let slot = self.slots[number as usize].as_mut().expect("an entity's instance is there");
        let before = slot.part;
        let was_sighted = self.sight[number as usize];
        // The mesh's place in the world is worked out again when it is next looked at, unless
        // neither it nor the mesh has changed.
        let mut sight = Sight::of(&part);
        if motion::has_moved(&part.model, &before.model) {
            slot.was = before.model;
            sight.moved = self.frame;
            self.moved_now.push(number);
        } else {
            sight.moved = was_sighted.moved;
            if before.key.mesh() == part.key.mesh() {
                (sight.center, sight.radius) = (was_sighted.center, was_sighted.radius);
            }
        }
        self.sight[number as usize] = sight;
        slot.part = part;
        // Only what is different is sent again: a thing that moves keeps its look, and one
        // that changes colour stays where it is.
        let (place, look) = (PlaceRaw::new(part.model), LookRaw::new(&part.material));
        if std::mem::replace(&mut self.on_card[number as usize], place) != place {
            self.stale.push(number);
        }
        if std::mem::replace(&mut self.looks[number as usize], look) != look {
            self.stale_looks.push(number);
        }
        if before.key != part.key {
            Self::unlist(&mut self.batches, before.key, number);
            self.batches.entry(part.key).or_default().push(number);
        }
        self.resight(number);
        self.changed_for_tracing(number);
    }

    fn release(&mut self, number: u32) {
        if let Some(slot) = self.slots[number as usize].take() {
            Self::unlist(&mut self.batches, slot.part.key, number);
            self.free.push(number);
            // Seen from nowhere, until the number is taken again.
            self.sight[number as usize].seen = (f32::INFINITY, f32::INFINITY);
            // Nothing is there, and whoever takes the number starts afresh.
            self.card_sights[number as usize] = bytemuck::Zeroable::zeroed();
            self.sized[number as usize] = u32::MAX;
            self.stale_sights.push(number);
            self.changed_for_tracing(number);
        }
    }

    fn changed_for_tracing(&mut self, number: u32) {
        if self.traced {
            self.for_tracing.push(number);
        }
    }

    fn unlist(batches: &mut BTreeMap<BatchKey, Vec<u32>>, key: BatchKey, number: u32) {
        if let Some(numbers) = batches.get_mut(&key) {
            if let Some(at) = numbers.iter().position(|n| *n == number) {
                numbers.swap_remove(at);
            }
            if numbers.is_empty() {
                batches.remove(&key);
            }
        }
    }

    /// Sends the card the instances that have changed. Answers whether there is a new
    /// buffer, as there is when the old one has no room.
    pub fn upload(&mut self, gpu: &Gpu) -> bool {
        if self.buffer.is_none() || self.slots.len() > self.room {
            // Everything goes into the new buffer, so nothing is left stale.
            self.room = self.slots.len().next_power_of_two().max(256);
            let places = Self::make(&gpu.device, "instances", self.room * std::mem::size_of::<PlaceRaw>());
            let looks = Self::make(&gpu.device, "instances' looks", self.room * std::mem::size_of::<LookRaw>());
            let sights = Self::make(&gpu.device, "instances' sights", self.room * std::mem::size_of::<SightRaw>());
            self.generation += 1;
            self.stale.clear();
            self.stale_looks.clear();
            self.stale_sights.clear();
            gpu.queue.write_buffer(&places, 0, bytemuck::cast_slice(&self.on_card));
            gpu.queue.write_buffer(&looks, 0, bytemuck::cast_slice(&self.looks));
            gpu.queue.write_buffer(&sights, 0, bytemuck::cast_slice(&self.card_sights));
            (self.buffer, self.looks_buffer, self.sights_buffer) = (Some(places), Some(looks), Some(sights));
            return true;
        }
        fn send<T: bytemuck::Pod>(gpu: &Gpu, buffer: &wgpu::Buffer, kept: &[T], stale: Vec<u32>) {
            for run in runs(stale, NEAR) {
                let stretch = &kept[run.start as usize..run.end as usize];
                gpu.queue.write_buffer(buffer, run.start as u64 * std::mem::size_of::<T>() as u64, bytemuck::cast_slice(stretch));
            }
        }
        send(gpu, self.buffer.as_ref().unwrap(), &self.on_card, std::mem::take(&mut self.stale));
        send(gpu, self.looks_buffer.as_ref().unwrap(), &self.looks, std::mem::take(&mut self.stale_looks));
        send(gpu, self.sights_buffer.as_ref().unwrap(), &self.card_sights, std::mem::take(&mut self.stale_sights));
        false
    }

    /// What a view sees. `bounds` gives a mesh's bounding sphere in its own space, once the
    /// mesh is on the card; until then its instances are taken to be in view.
    pub fn seen(&mut self, from: &Viewpoint, bounds: impl Fn(u32) -> Option<(Vec3, f32)>) -> Seen {
        let planes = unit_planes(from.view_proj);
        let cascades: Vec<_> = from.cascades.iter().map(|cascade| unit_planes(*cascade)).collect();
        let mut seen = Seen { shadow_batches: vec![vec![]; cascades.len()], ..Default::default() };
        let mut shadow_numbers: Vec<Vec<u32>> = vec![vec![]; cascades.len()];
        for (key, numbers) in &self.batches {
            let first = seen.numbers.len() as u32;
            let first_shadows: Vec<u32> = shadow_numbers.iter().map(|numbers| numbers.len() as u32).collect();
            // A mesh's size is asked for once a batch, and only if an instance wants it.
            let mut size: Option<Option<(Vec3, f32)>> = None;
            for &number in numbers {
                let sight = &mut self.sight[number as usize];
                let away = from.camera.distance(sight.origin);
                if away < sight.seen.0 || away >= sight.seen.1 {
                    continue;
                }
                if sight.radius < 0.0 {
                    if let Some((center, radius)) = *size.get_or_insert_with(|| bounds(key.mesh())) {
                        let model = self.slots[number as usize].as_ref().expect("a batch's instance is there").part.model;
                        let scale = model.x_axis.truncate().length().max(model.y_axis.truncate().length()).max(model.z_axis.truncate().length());
                        (sight.center, sight.radius) = (model.transform_point3(center), radius * scale);
                    }
                }
                let sized = sight.radius >= 0.0;
                let (center, radius) = (sight.center, sight.radius);
                let inside = |planes: &[(Vec3, f32); 5]| !sized || !planes.iter().any(|p| p.0.dot(center) + p.1 < -radius);
                let in_view = inside(&planes);
                let casts = sight.casts_shadow && (!sized || center.distance(from.camera) - radius <= from.shadow_reach);
                if in_view {
                    seen.numbers.push(number);
                    if from.blended && sight.moved == self.frame && self.frame != 0 && !sight.decal {
                        let slot = self.slots[number as usize].as_ref().expect("a batch's instance is there");
                        seen.moved.push(motion::Moved { mesh: key.mesh(), model: slot.part.model, was: slot.was });
                    }
                }
                if casts {
                    // Into each cascade whose map it falls in, and no other.
                    for (cascade, numbers) in cascades.iter().zip(&mut shadow_numbers) {
                        if inside(cascade) {
                            numbers.push(number);
                        }
                    }
                }
            }
            if seen.numbers.len() as u32 > first {
                seen.batches.push((*key, first..seen.numbers.len() as u32));
            }
            for ((numbers, first), batches) in shadow_numbers.iter().zip(first_shadows).zip(&mut seen.shadow_batches) {
                if numbers.len() as u32 > first {
                    batches.push((*key, first..numbers.len() as u32));
                }
            }
        }
        // Each cascade's numbers follow the view's, one cascade after another, in the one list.
        for (numbers, batches) in shadow_numbers.into_iter().zip(&mut seen.shadow_batches) {
            let offset = seen.numbers.len() as u32;
            for (_, range) in batches {
                *range = range.start + offset..range.end + offset;
            }
            seen.numbers.extend(numbers);
        }
        seen
    }

    /// The instances that have changed since this was last asked, for the traced scene to
    /// bring itself up to date with: made, changed or let go. The first time, every one
    /// there is. Nothing is kept for it until it has asked once, since where no rays are
    /// traced nobody asks.
    pub fn take_traced_changes(&mut self) -> Vec<u32> {
        if !self.traced {
            self.traced = true;
            return (0..self.slots.len() as u32).filter(|number| self.slots[*number as usize].is_some()).collect();
        }
        std::mem::take(&mut self.for_tracing)
    }

    /// For every instance number, whether it is within the rays' reach of the camera and at
    /// the level of detail seen from there. Cheap: what a view reads to decide what it sees.
    pub fn within(&self, camera: Vec3, reach: f32) -> impl Iterator<Item = (u32, bool)> + '_ {
        self.sight.iter().enumerate().map(move |(number, sight)| {
            let away = camera.distance(sight.origin);
            (number as u32, away >= sight.seen.0 && away < sight.seen.1 && away <= reach)
        })
    }

    /// Whether one instance is within the rays' reach, as [`within`](Self::within) says of
    /// all of them.
    pub fn is_within(&self, number: u32, camera: Vec3, reach: f32) -> bool {
        self.sight.get(number as usize).is_some_and(|sight| {
            let away = camera.distance(sight.origin);
            away >= sight.seen.0 && away < sight.seen.1 && away <= reach
        })
    }

    /// An instance as rays are traced against it, if it is there.
    pub fn traced(&self, number: u32) -> Option<Traced<'_>> {
        let slot = self.slots.get(number as usize)?.as_ref()?;
        Some(Traced { mesh: slot.part.key.mesh(), model: slot.part.model, material: &slot.part.material })
    }
}

/// Changed instances this near each other are sent as one stretch, the unchanged ones between
/// them with them: a write costs more to begin than a few instances cost to send again.
const NEAR: u32 = 32;

/// Gathers changed instances into the stretches to send: each from its first changed number
/// to one past its last, where no two changed numbers in it are more than `near` apart.
/// Numbers come in any order and may repeat.
pub(crate) fn runs(mut numbers: Vec<u32>, near: u32) -> Vec<Range<u32>> {
    numbers.sort_unstable();
    numbers.dedup();
    let mut runs: Vec<Range<u32>> = Vec::new();
    for number in numbers {
        match runs.last_mut() {
            Some(run) if number - run.end < near => run.end = number + 1,
            _ => runs.push(number..number + 1),
        }
    }
    runs
}

/// The planes bounding what a matrix takes in, each with a normal of length one so that a
/// distance from it is in metres.
pub(crate) fn unit_planes(view_proj: Mat4) -> [(Vec3, f32); 5] {
    frustum_planes(view_proj).map(|p| {
        let length = p.truncate().length().max(1e-12);
        (p.truncate() / length, p.w / length)
    })
}

/// Planes `(normal, d)` bounding the view, inward-facing. With reversed, infinite Z there is
/// no far plane; the near plane is `z <= w`. For a shadow cascade's matrix, whose depth runs
/// the usual way, the same five are its sides and its far end: nothing is left out for being
/// too near the sun.
pub(crate) fn frustum_planes(view_proj: Mat4) -> [Vec4; 5] {
    let (x, y, z, w) = (view_proj.row(0), view_proj.row(1), view_proj.row(2), view_proj.row(3));
    [w + x, w - x, w + y, w - y, w - z]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::World;

    fn part(mesh: u32, at: Vec3) -> Part {
        let material = Material::default();
        Part { key: BatchKey::new(mesh, &material, 0), model: Mat4::from_translation(at), material, seen: (0.0, f32::INFINITY), casts_shadow: true }
    }

    /// Looking down -z from the origin, with everything in reach of the shadows.
    fn viewpoint() -> Viewpoint {
        let projection = Mat4::perspective_infinite_reverse_rh(1.0, 1.0, 0.1);
        // One cascade: a box forty metres square about the origin, looked into along -z.
        let cascade = Mat4::orthographic_rh(-20.0, 20.0, -20.0, 20.0, 0.0, 100.0) * Mat4::from_translation(Vec3::new(0.0, 0.0, -50.0));
        Viewpoint { view_proj: projection, camera: Vec3::ZERO, cascades: vec![cascade], shadow_reach: 1000.0, blended: true }
    }

    fn unit_sphere(_: u32) -> Option<(Vec3, f32)> {
        Some((Vec3::ZERO, 1.0))
    }

    #[test]
    fn an_entitys_instances_are_kept_and_given_back() {
        let mut world = World::new();
        let (a, b) = (world.spawn(()), world.spawn(()));
        let mut scene = Instances::default();
        scene.set(a, &[part(1, Vec3::ZERO)]);
        scene.set(b, &[part(1, Vec3::X), part(2, Vec3::X)]);
        assert_eq!(scene.len(), 3);
        // Set again, an entity keeps the instances it had.
        let before = scene.of[&b].clone();
        scene.set(b, &[part(1, Vec3::Y), part(2, Vec3::Y)]);
        assert_eq!(scene.of[&b], before);
        // With fewer parts it gives one back, and the next to ask takes it.
        scene.set(b, &[part(1, Vec3::Y)]);
        assert_eq!(scene.len(), 2);
        scene.set(a, &[part(1, Vec3::ZERO), part(3, Vec3::ZERO)]);
        assert_eq!(scene.of[&a][1], before[1]);
        scene.forget(a);
        scene.forget(a);
        scene.set(b, &[]);
        assert_eq!(scene.len(), 0);
        assert!(scene.batches.is_empty() && scene.of.is_empty());
    }

    #[test]
    fn a_view_sees_what_is_before_it_batch_by_batch() {
        let mut world = World::new();
        let mut scene = Instances::default();
        let ahead = Vec3::new(0.0, 0.0, -10.0);
        for (mesh, at) in [(1, ahead), (2, ahead), (1, ahead + Vec3::X), (1, -ahead), (2, Vec3::new(500.0, 0.0, -10.0))] {
            scene.set(world.spawn(()), &[part(mesh, at)]);
        }
        let seen = scene.seen(&viewpoint(), unit_sphere);
        // Two of mesh 1 and one of mesh 2 are in view; the one behind and the one far to the
        // side are not. The one behind still casts into the cascade, which holds it; the one
        // far to the side is outside the cascade's map and casts nothing there.
        let counts: Vec<(u32, u32)> = seen.batches.iter().map(|(key, range)| (key.mesh(), range.end - range.start)).collect();
        assert_eq!(counts, [(1, 2), (2, 1)]);
        let shadows: u32 = seen.shadow_batches[0].iter().map(|(_, range)| range.end - range.start).sum();
        assert_eq!(shadows, 4);
        // The cascade's numbers follow the view's in the one list.
        assert_eq!(seen.numbers.len(), 7);
        assert_eq!(seen.shadow_batches[0][0].1.start, 3);
        // A second cascade, somewhere there is nothing, gets nothing; with no cascades there
        // are no shadows at all.
        let elsewhere = Mat4::orthographic_rh(-5.0, 5.0, -5.0, 5.0, 0.0, 10.0) * Mat4::from_translation(Vec3::new(0.0, -900.0, 0.0));
        let two = Viewpoint { cascades: vec![viewpoint().cascades[0], elsewhere], ..viewpoint() };
        let seen = scene.seen(&two, unit_sphere);
        assert_eq!(seen.shadow_batches.len(), 2);
        assert!(seen.shadow_batches[1].is_empty() && seen.numbers.len() == 7);
        assert_eq!(scene.seen(&Viewpoint { cascades: vec![], ..viewpoint() }, unit_sphere).numbers.len(), 3);
        // Until a mesh's size is known, its instances are taken to be in view.
        let mut unknown = Instances::default();
        unknown.set(world.spawn(()), &[part(1, -ahead)]);
        assert_eq!(unknown.seen(&viewpoint(), |_| None).numbers.len(), 2);
    }

    #[test]
    fn a_level_of_detail_is_seen_only_from_its_own_distances() {
        let mut world = World::new();
        let mut scene = Instances::default();
        let at = Vec3::new(0.0, 0.0, -30.0);
        let (near, far) = (Part { seen: (0.0, 20.0), ..part(1, at) }, Part { seen: (20.0, 80.0), ..part(2, at) });
        scene.set(world.spawn(()), &[near, far]);
        let meshes = |scene: &mut Instances, camera: Vec3| -> Vec<u32> {
            let from = Viewpoint { camera, view_proj: viewpoint().view_proj * Mat4::from_translation(-camera), ..viewpoint() };
            scene.seen(&from, unit_sphere).batches.iter().map(|(key, _)| key.mesh()).collect()
        };
        assert_eq!(meshes(&mut scene, Vec3::ZERO), [2]);
        assert_eq!(meshes(&mut scene, Vec3::new(0.0, 0.0, -15.0)), [1]);
        assert!(meshes(&mut scene, Vec3::new(0.0, 0.0, 60.0)).is_empty());
        // The rays' reach asks the same of each level: from the origin only the far one is in.
        let within: Vec<bool> = scene.within(Vec3::ZERO, 100.0).map(|(_, within)| within).collect();
        assert_eq!(within, [false, true]);
        assert!(scene.within(Vec3::ZERO, 10.0).all(|(_, within)| !within), "beyond the rays' reach");
    }

    #[test]
    fn what_moved_this_frame_is_told_with_where_it_was() {
        let mut world = World::new();
        let (still, mover) = (world.spawn(()), world.spawn(()));
        let mut scene = Instances::default();
        let ahead = Vec3::new(0.0, 0.0, -10.0);
        scene.begin(7);
        scene.set(still, &[part(1, ahead)]);
        scene.set(mover, &[part(1, ahead + Vec3::X)]);
        // New things have not moved.
        assert!(scene.seen(&viewpoint(), unit_sphere).moved.is_empty());
        scene.begin(8);
        scene.set(still, &[part(1, ahead)]);
        scene.set(mover, &[part(1, ahead + Vec3::Y)]);
        let moved = scene.seen(&viewpoint(), unit_sphere).moved;
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].was, Mat4::from_translation(ahead + Vec3::X));
        assert_eq!(moved[0].model, Mat4::from_translation(ahead + Vec3::Y));
        // The frame after, it is at rest again; and nothing is told where nothing asks.
        scene.begin(9);
        assert!(scene.seen(&viewpoint(), unit_sphere).moved.is_empty());
        scene.begin(10);
        scene.set(mover, &[part(1, ahead)]);
        assert!(scene.seen(&Viewpoint { blended: false, ..viewpoint() }, unit_sphere).moved.is_empty());
    }

    #[test]
    fn an_entity_that_only_moved_has_only_its_place_brought_up_to_date() {
        let mut world = World::new();
        let (mover, stranger) = (world.spawn(()), world.spawn(()));
        let mut scene = Instances::default();
        let ahead = Vec3::new(0.0, 0.0, -10.0);
        scene.begin(3);
        scene.set(mover, &[part(1, ahead), part(2, ahead)]);
        assert!(!scene.moved(stranger, Mat4::IDENTITY), "one that draws nothing yet is to be set");
        let _ = scene.take_traced_changes();
        scene.stale.clear();
        scene.stale_looks.clear();
        scene.begin(4);
        // Where it was already: nothing to send.
        assert!(scene.moved(mover, Mat4::from_translation(ahead)));
        assert!(scene.stale.is_empty());
        // Elsewhere: both its instances are sent, where they are and not how they look, and
        // the view is told they moved and from where.
        assert!(scene.moved(mover, Mat4::from_translation(ahead + Vec3::X)));
        assert_eq!(scene.stale, [0, 1]);
        assert!(scene.stale_looks.is_empty());
        assert_eq!(scene.take_traced_changes(), [0, 1]);
        let seen = scene.seen(&viewpoint(), unit_sphere);
        assert_eq!(seen.moved.len(), 2);
        assert_eq!(seen.moved[0].was, Mat4::from_translation(ahead));
        assert_eq!(scene.batches.len(), 2);
    }

    #[test]
    fn the_card_is_told_what_it_needs_to_decide_what_is_seen() {
        let mut world = World::new();
        let (a, b) = (world.spawn(()), world.spawn(()));
        let mut scene = Instances::default();
        scene.set(a, &[Part { seen: (0.0, 20.0), ..part(1, Vec3::ZERO) }, Part { seen: (20.0, 80.0), casts_shadow: false, ..part(2, Vec3::ZERO) }]);
        scene.set(b, &[part(1, Vec3::X)]);
        // Each batch has a number, which the instances in it are filed under.
        assert_eq!(scene.batch_numbers(), 2);
        assert_eq!(scene.card_sights[0].batch, scene.card_sights[2].batch);
        assert_ne!(scene.card_sights[0].batch, scene.card_sights[1].batch);
        assert_eq!((scene.card_sights[1].seen, scene.card_sights[1].flags), ([20.0, 80.0], 1));
        assert_eq!(scene.card_sights[0].flags, 3);
        // A mesh's size is not known until it is on the card: asked for until it is, then given
        // to every instance of it, and those are sent again.
        assert!(scene.card_sights.iter().all(|sight| sight.radius < 0.0));
        scene.stale_sights.clear();
        scene.size(|mesh| (mesh == 1).then_some((Vec3::Y, 2.0)));
        assert_eq!(scene.stale_sights, [0, 2]);
        assert_eq!((scene.card_sights[0].center, scene.card_sights[0].radius), ([0.0, 1.0, 0.0], 2.0));
        assert_eq!(scene.unsized_yet, [1]);
        scene.size(unit_sphere);
        assert!(scene.unsized_yet.is_empty() && scene.card_sights[1].radius == 1.0);
        // Moving changes nothing of it; another mesh has its size asked for again; and one
        // let go of is not there.
        scene.stale_sights.clear();
        scene.moved(b, Mat4::from_translation(Vec3::Z));
        assert!(scene.stale_sights.is_empty());
        scene.set(b, &[part(3, Vec3::Z)]);
        assert_eq!((scene.card_sights[2].radius, scene.unsized_yet.as_slice()), (-1.0, &[2][..]));
        scene.forget(a);
        assert_eq!(scene.card_sights[0].flags, 0);
        assert_eq!(scene.batches().map(|(key, _, instances)| (key.mesh(), instances)).collect::<Vec<_>>(), [(3, 1)]);
    }

    #[test]
    fn an_instance_whose_batch_changes_moves_to_the_new_one() {
        let mut world = World::new();
        let entity = world.spawn(());
        let mut scene = Instances::default();
        scene.set(entity, &[part(1, Vec3::ZERO)]);
        scene.set(entity, &[part(2, Vec3::ZERO)]);
        let meshes: Vec<u32> = scene.batches.keys().map(BatchKey::mesh).collect();
        assert_eq!(meshes, [2]);
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn the_traced_scene_is_told_what_changed_and_what_is_in_reach() {
        let mut world = World::new();
        let (near, far) = (world.spawn(()), world.spawn(()));
        let mut scene = Instances::default();
        scene.set(near, &[part(1, Vec3::new(0.0, 0.0, -10.0))]);
        scene.set(far, &[part(2, Vec3::new(0.0, 0.0, -100.0))]);
        // The first time it asks, everything there is; after that, only what has changed.
        assert_eq!(scene.take_traced_changes(), [0, 1]);
        assert!(scene.take_traced_changes().is_empty());
        scene.set(near, &[part(1, Vec3::new(0.0, 0.0, -11.0))]);
        assert_eq!(scene.take_traced_changes(), [0]);
        assert_eq!(scene.traced(0).map(|part| part.mesh), Some(1));
        // What is in reach depends on where the camera is.
        let in_reach = |scene: &Instances, camera: Vec3| -> Vec<u32> { scene.within(camera, 50.0).filter(|(_, within)| *within).map(|(number, _)| number).collect() };
        assert_eq!(in_reach(&scene, Vec3::ZERO), [0]);
        assert_eq!(in_reach(&scene, Vec3::new(0.0, 0.0, -60.0)), [0, 1]);
        assert!(scene.is_within(1, Vec3::new(0.0, 0.0, -60.0), 50.0) && !scene.is_within(1, Vec3::ZERO, 50.0));
        // One that is gone is told of, is not there to trace, and is in reach of nothing.
        scene.forget(near);
        assert_eq!(scene.take_traced_changes(), [0]);
        assert!(scene.traced(0).is_none());
        assert_eq!(in_reach(&scene, Vec3::ZERO), [] as [u32; 0]);
    }
    #[test]
    fn changed_instances_are_sent_as_runs_of_neighbours() {
        // Neighbours only, where nothing nearby counts as near.
        assert_eq!(runs(vec![5, 1, 2, 2, 9, 7, 3], 1), [1..4, 5..6, 7..8, 9..10]);
        // Those a few apart go together, with what lies between them.
        assert_eq!(runs(vec![40, 0, 10, 100, 20], 16), [0..21, 40..41, 100..101]);
        assert!(runs(vec![], 16).is_empty());
    }
}