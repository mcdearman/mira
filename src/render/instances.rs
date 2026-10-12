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

use glam::{Mat3, Mat4, Vec3, Vec4};

use super::{gpu::Gpu, motion, renderer::BatchKey, Material};
use crate::ecs::Entity;

/// An instance as the mesh shaders read it. Must match `Instance` in `mesh.wgsl` and
/// `mesh_shadow.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct InstanceRaw {
    model: [[f32; 4]; 4],
    /// The rows of a 3 by 3 matrix, each with a fourth number that is not used: the card
    /// lays a `mat3x3` out that way in a buffer.
    normal: [[f32; 4]; 3],
    color: [f32; 4],
    params: [f32; 4],
    emissive: [f32; 4],
    extra: [f32; 4],
    more: [f32; 4],
}

impl InstanceRaw {
    fn new(model: Mat4, m: &Material) -> Self {
        // Inverse-transpose keeps normals perpendicular under non-uniform scale.
        let normal = Mat3::from_mat4(model).inverse().transpose();
        Self {
            model: model.to_cols_array_2d(),
            normal: [normal.x_axis.extend(0.0).to_array(), normal.y_axis.extend(0.0).to_array(), normal.z_axis.extend(0.0).to_array()],
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
    /// What the card holds for each instance, kept here too so that a stretch of them can be
    /// sent again without working any of it out.
    on_card: Vec<InstanceRaw>,
    /// Instances changed since they were last sent to the card.
    stale: Vec<u32>,
    /// The frame entities were last brought up to date in.
    frame: u64,
    /// None until instances are first sent.
    buffer: Option<wgpu::Buffer>,
    /// How many instances the buffer has room for.
    room: usize,
    /// Counts the buffers made, so that what binds one knows when there is another.
    pub generation: u64,
}

const STRIDE: u64 = std::mem::size_of::<InstanceRaw>() as u64;

impl Instances {
    fn make(device: &wgpu::Device, room: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: room as u64 * STRIDE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// The instances on the card. They must have been sent once.
    pub fn buffer(&self) -> &wgpu::Buffer {
        self.buffer.as_ref().expect("instances are sent before they are drawn")
    }

    /// How many instances there are.
    pub fn len(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    /// Says which frame it is, before entities are brought up to date in it.
    pub fn begin(&mut self, frame: u64) {
        self.frame = frame;
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

    /// Lets go of everything an entity drew.
    pub fn forget(&mut self, entity: Entity) {
        for number in self.of.remove(&entity).unwrap_or_default() {
            self.release(number);
        }
    }

    fn take(&mut self, part: Part) -> u32 {
        let slot = Slot { part, was: part.model };
        let (raw, sight) = (InstanceRaw::new(part.model, &part.material), Sight::of(&part));
        let number = match self.free.pop() {
            Some(number) => {
                self.slots[number as usize] = Some(slot);
                self.on_card[number as usize] = raw;
                self.sight[number as usize] = sight;
                number
            }
            None => {
                self.slots.push(Some(slot));
                self.on_card.push(raw);
                self.sight.push(sight);
                self.slots.len() as u32 - 1
            }
        };
        self.batches.entry(part.key).or_default().push(number);
        self.stale.push(number);
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
        } else {
            sight.moved = was_sighted.moved;
            if before.key.mesh() == part.key.mesh() {
                (sight.center, sight.radius) = (was_sighted.center, was_sighted.radius);
            }
        }
        self.sight[number as usize] = sight;
        slot.part = part;
        self.on_card[number as usize] = InstanceRaw::new(part.model, &part.material);
        if before.key != part.key {
            Self::unlist(&mut self.batches, before.key, number);
            self.batches.entry(part.key).or_default().push(number);
        }
        self.stale.push(number);
    }

    fn release(&mut self, number: u32) {
        if let Some(slot) = self.slots[number as usize].take() {
            Self::unlist(&mut self.batches, slot.part.key, number);
            self.free.push(number);
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
            let buffer = Self::make(&gpu.device, self.room);
            self.generation += 1;
            self.stale.clear();
            gpu.queue.write_buffer(&buffer, 0, bytemuck::cast_slice(&self.on_card));
            self.buffer = Some(buffer);
            return true;
        }
        let buffer = self.buffer.as_ref().unwrap();
        for run in runs(std::mem::take(&mut self.stale), NEAR) {
            let stretch = &self.on_card[run.start as usize..run.end as usize];
            gpu.queue.write_buffer(buffer, run.start as u64 * STRIDE, bytemuck::cast_slice(stretch));
        }
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

    /// The parts rays are traced against: every one near enough to the camera for its level
    /// of detail, in view or not.
    pub fn traced(&self, camera: Vec3) -> impl Iterator<Item = Traced<'_>> {
        self.slots.iter().flatten().filter_map(move |slot| {
            let away = camera.distance(slot.part.model.w_axis.truncate());
            (away >= slot.part.seen.0 && away < slot.part.seen.1).then_some(Traced { mesh: slot.part.key.mesh(), model: slot.part.model, material: &slot.part.material })
        })
    }

}

/// Changed instances this near each other are sent as one stretch, the unchanged ones between
/// them with them: a write costs more to begin than a few instances cost to send again.
const NEAR: u32 = 32;

/// Gathers changed instances into the stretches to send: each from its first changed number
/// to one past its last, where no two changed numbers in it are more than `near` apart.
/// Numbers come in any order and may repeat.
fn runs(mut numbers: Vec<u32>, near: u32) -> Vec<Range<u32>> {
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
fn unit_planes(view_proj: Mat4) -> [(Vec3, f32); 5] {
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
        assert_eq!(scene.traced(Vec3::ZERO).map(|part| part.mesh).collect::<Vec<_>>(), [2]);
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
    fn changed_instances_are_sent_as_runs_of_neighbours() {
        // Neighbours only, where nothing nearby counts as near.
        assert_eq!(runs(vec![5, 1, 2, 2, 9, 7, 3], 1), [1..4, 5..6, 7..8, 9..10]);
        // Those a few apart go together, with what lies between them.
        assert_eq!(runs(vec![40, 0, 10, 100, 20], 16), [0..21, 40..41, 100..101]);
        assert!(runs(vec![], 16).is_empty());
    }
}