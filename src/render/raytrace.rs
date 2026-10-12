//! Hardware ray tracing, through wgpu's (experimental, Vulkan-only) ray queries.
//!
//! Every opaque mesh gets a bottom-level acceleration structure once, and the scene's
//! instances are kept in top-level structures that shaders trace against, for sun shadows,
//! ambient occlusion and reflections. An instance keeps its place, so a frame touches only
//! the ones that changed.
//!
//! A structure is built again, whole, whenever any instance in it has changed. So there
//! are two: a large one of what stands still, which is seldom built, and a small one of
//! what moves, which is built every frame something in it does. A thing is among those
//! that move while it is moving and for a couple of seconds after, and rays are traced
//! against both.
//!
//! Reflections shade what they hit properly: every traced mesh's normals and texture
//! coordinates live in one storage buffer, each instance has a material entry, and all
//! base-colour textures are bound together as an array, so a reflected wall shows its real
//! stonework, lit by the sun with its own shadow ray.
//!
//! Ray queries can't yet test alpha, so cut-out geometry such as leaves stays out of the
//! traced scene and keeps casting through the shadow maps.

use std::collections::HashMap;

use glam::Mat4;
use wgpu::util::DeviceExt;

use super::{gpu::Gpu, mesh::Mesh, probes::{ProbeGrid, ProbeVolume}, Color};
use crate::assets::Assets;

/// The most instances the ray-traced scene holds at once.
pub const MAX_INSTANCES: u32 = 1 << 16;
/// How many distinct textures reflections can show; slot 0 is plain white.
pub const TEXTURE_SLOTS: u32 = 256;
/// Floats per vertex in the hit buffer: normal, then texture coordinate.
const HIT_STRIDE: usize = 5;

/// Whether to use ray tracing when the GPU supports it. Insert before `DefaultPlugins`.
#[derive(Clone, Copy, Debug)]
pub struct RayTracingSettings {
    pub enabled: bool,
    /// Meshes further from the camera than this are left out of the traced scene.
    pub max_distance: f32,
    /// Light probes for the light from all around, baked once the scene settles. Without
    /// them surfaces see only the sky's light, as if nothing stood around them.
    pub probes: Option<ProbeGrid>,
}

impl Default for RayTracingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_distance: 400.0,
            probes: None,
        }
    }
}

struct Geometry {
    positions: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Bytes from one vertex position to the next.
    stride: u64,
    size: wgpu::BlasTriangleGeometrySizeDescriptor,
    blas: wgpu::Blas,
    built: bool,
    /// Rebuilt every frame, from vertices the GPU poses (a skinned figure).
    dynamic: bool,
}

/// An instance traced against: which geometry, where, which instance of the scene it is
/// (none for one that was fixed in place), and the frame it last changed in.
#[derive(Clone, Copy)]
struct Entry {
    geometry: GeometryId,
    transform: Mat4,
    number: u32,
    touched: u64,
}

/// One of the two structures rays are traced against, and what is in it. An instance keeps
/// its place for as long as it is there, so only the places that change are told to it.
pub(crate) struct Structure {
    pub(crate) tlas: wgpu::Tlas,
    entries: Vec<Option<Entry>>,
    free: Vec<u32>,
    room: u32,
    /// Where its places begin among the materials: what an instance in it is numbered from.
    base: u32,
    /// Places it is to be told of.
    unplaced: Vec<u32>,
    built: bool,
}

impl Structure {
    fn new(device: &wgpu::Device, label: &str, room: u32, base: u32, flags: wgpu::AccelerationStructureFlags) -> Self {
        let tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
            label: Some(label),
            max_instances: room,
            flags,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        });
        Self { tlas, entries: Vec::new(), free: Vec::new(), room, base, unplaced: Vec::new(), built: false }
    }

    fn count(&self) -> usize {
        self.entries.len() - self.free.len()
    }

    /// A place, if there is one left.
    fn take_place(&mut self) -> Option<u32> {
        self.free.pop().or_else(|| {
            (self.entries.len() < self.room as usize).then(|| {
                self.entries.push(None);
                self.entries.len() as u32 - 1
            })
        })
    }

    fn set(&mut self, place: u32, entry: Entry) {
        self.entries[place as usize] = Some(entry);
        self.unplaced.push(place);
    }

    /// Empties a place and gives back what was in it.
    fn clear(&mut self, place: u32) -> Option<Entry> {
        let entry = self.entries[place as usize].take();
        if entry.is_some() {
            self.free.push(place);
            self.unplaced.push(place);
        }
        entry
    }

    /// Tells the structure of the places that changed. Answers whether it is to be built:
    /// because any did, or geometry under one of its instances is built again this frame, or
    /// it never has been.
    fn tell(&mut self, geometry: &[Geometry], pending: &[usize]) -> bool {
        let mut unplaced = std::mem::take(&mut self.unplaced);
        unplaced.sort_unstable();
        unplaced.dedup();
        let mut build = !self.built || !unplaced.is_empty();
        for place in unplaced {
            let Some(entry) = &self.entries[place as usize] else {
                self.tlas[place as usize] = None;
                continue;
            };
            let g = entry.geometry.0 as usize;
            if !geometry[g].built && !pending.contains(&g) {
                // Not built yet: left out this frame, and tried again the next.
                self.tlas[place as usize] = None;
                self.unplaced.push(place);
                continue;
            }
            let t = entry.transform.transpose().to_cols_array();
            // Row-major 3x4: the first three rows of the (column-major) matrix.
            let rows: [f32; 12] = t[..12].try_into().unwrap();
            // The custom index finds the instance's material: both are kept by place.
            self.tlas[place as usize] = Some(wgpu::TlasInstance::new(&geometry[g].blas, rows, self.base + place, 0xff));
        }
        // A figure that is posed again each frame moves under the instance that shows it.
        build |= pending.iter().any(|g| geometry[*g].dynamic) && self.entries.iter().flatten().any(|entry| geometry[entry.geometry.0 as usize].dynamic);
        self.built |= build;
        build
    }
}

/// Said of an instance of the scene in place of a place: it has not been looked at, or it
/// was, is within reach, and is left out (not a thing to trace against, or no room).
const UNKNOWN: u32 = u32::MAX;
const LEFT_OUT: u32 = u32::MAX - 1;

/// How many things that move are traced against at once. They are in a structure of their
/// own, built again whenever one of them moves, so it is kept small.
pub const MAX_MOVING: u32 = 1 << 14;
/// How many frames a thing stands still before it is counted among those that stand still.
const IDLE: u64 = 120;
/// Things are moved to the structure of those that stand still when this many are ready to
/// be, and in any case this often, in frames.
const SETTLE_MANY: usize = 256;
/// How many instances new to the scene in one frame are taken for things appearing in it,
/// and put among those that move; more than this at once is taken for a scene being loaded.
const ARRIVE_AS_MOVING: u32 = 256;
const SETTLE_AT_LEAST: u64 = 600;

/// A handle to geometry added with [`RayTracing::add_geometry`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeometryId(u32);

/// How a traced surface looks when a reflection ray hits it. Must match `HitInstance` in
/// `rt_on.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HitMaterial {
    pub color: [f32; 4],
    pub emissive: [f32; 4],
    pub(crate) geometry: u32,
    /// Slot in the texture array; 0 is plain white.
    pub texture: u32,
    pub roughness: f32,
    pub metallic: f32,
}

impl HitMaterial {
    pub fn plain(color: Color) -> Self {
        Self {
            color: color.to_array(),
            emissive: [0.0; 4],
            geometry: 0,
            texture: 0,
            roughness: 0.8,
            metallic: 0.0,
        }
    }
}

/// The ray-traced scene. Present only when the GPU supports ray queries.
pub struct RayTracing {
    /// What stands still, and what moves: two structures, so that when something moves only
    /// the small one of the things that move is built again. Rays are traced against both.
    /// A place in the first is numbered from nought and one in the second from
    /// `MAX_INSTANCES`, which is also where each one's material is kept.
    pub(crate) still: Structure,
    pub(crate) moving: Structure,
    geometry: Vec<Geometry>,
    meshes: HashMap<u32, GeometryId>,
    /// For each instance of the scene, by its number: its place, or that it has not been
    /// looked at, or that it was and is left out.
    of: Vec<u32>,
    /// What the card holds of each place's material, kept here too so a stretch can be sent.
    materials: Vec<HitMaterial>,
    /// Places whose material is to be sent.
    stale: Vec<u32>,
    /// Frames counted, and the one things were last settled in.
    frame: u64,
    settled: u64,
    /// How many instances new to the scene have been placed this frame.
    arrivals: u32,
    /// Instances of the scene to look at again next frame: a mesh or a texture of theirs
    /// was not ready.
    pub(crate) again: Vec<u32>,
    pub settings: RayTracingSettings,

    /// Normals and texture coordinates of every geometry, end to end.
    hit_vertices: Vec<f32>,
    hit_indices: Vec<u32>,
    /// (first vertex, first index) per geometry.
    hit_ranges: Vec<[u32; 2]>,
    pub(crate) vertex_buffer: wgpu::Buffer,
    pub(crate) index_buffer: wgpu::Buffer,
    pub(crate) range_buffer: wgpu::Buffer,
    pub(crate) instance_buffer: wgpu::Buffer,
    buffers_stale: bool,

    textures: Vec<wgpu::TextureView>,
    slots: HashMap<u32, u32>,
    pub(crate) sampler: wgpu::Sampler,
    /// Changes whenever the bindings shaders see must be rebuilt.
    pub(crate) generation: u64,
    pub(crate) probes: ProbeVolume,
}

fn storage(device: &wgpu::Device, label: &str, contents: &[u8]) -> wgpu::Buffer {
    // Bindings can't be empty.
    let padded;
    let contents = if contents.is_empty() {
        padded = [0u8; 16];
        &padded[..]
    } else {
        contents
    };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    })
}

impl RayTracing {
    pub(crate) fn new(gpu: &Gpu, settings: RayTracingSettings, white: wgpu::TextureView) -> Self {
        let device = &gpu.device;
        let still = Structure::new(device, "scene at rest", MAX_INSTANCES, 0, wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE);
        let moving = Structure::new(device, "scene moving", MAX_MOVING, MAX_INSTANCES, wgpu::AccelerationStructureFlags::PREFER_FAST_BUILD);
        let places = (MAX_INSTANCES + MAX_MOVING) as usize;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("hit instances"),
            size: places as u64 * std::mem::size_of::<HitMaterial>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("hit sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            still,
            moving,
            geometry: Vec::new(),
            meshes: HashMap::new(),
            of: Vec::new(),
            materials: vec![HitMaterial::plain(Color::WHITE); places],
            stale: Vec::new(),
            frame: 0,
            settled: 0,
            arrivals: 0,
            again: Vec::new(),
            settings,
            hit_vertices: Vec::new(),
            hit_indices: Vec::new(),
            hit_ranges: Vec::new(),
            vertex_buffer: storage(device, "hit vertices", &[]),
            index_buffer: storage(device, "hit indices", &[]),
            range_buffer: storage(device, "hit ranges", &[]),
            instance_buffer,
            buffers_stale: false,
            textures: vec![white],
            slots: HashMap::new(),
            sampler,
            generation: 1,
            probes: ProbeVolume::new(gpu, settings.probes),
        }
    }

    /// Adds triangles to trace against; place them with [`Self::add_fixed`] or per frame.
    pub fn add_geometry(
        &mut self,
        gpu: &Gpu,
        positions: &[[f32; 3]],
        normals: &[[f32; 3]],
        uvs: &[[f32; 2]],
        indices: &[u32],
    ) -> GeometryId {
        let device = &gpu.device;
        let size = wgpu::BlasTriangleGeometrySizeDescriptor {
            vertex_format: wgpu::VertexFormat::Float32x3,
            vertex_count: positions.len() as u32,
            index_format: Some(wgpu::IndexFormat::Uint32),
            index_count: Some(indices.len() as u32),
            flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
        };
        let blas = device.create_blas(
            &wgpu::CreateBlasDescriptor {
                label: Some("blas"),
                flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                update_mode: wgpu::AccelerationStructureUpdateMode::Build,
            },
            wgpu::BlasGeometrySizeDescriptors::Triangles {
                descriptors: vec![size.clone()],
            },
        );
        let input = |label, contents: &[u8]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::BLAS_INPUT,
            })
        };
        self.geometry.push(Geometry {
            positions: input("blas positions", bytemuck::cast_slice(positions)),
            indices: input("blas indices", bytemuck::cast_slice(indices)),
            stride: 12,
            size,
            blas,
            built: false,
            dynamic: false,
        });

        let first_vertex = (self.hit_vertices.len() / HIT_STRIDE) as u32;
        for (i, n) in normals.iter().enumerate() {
            let uv = uvs.get(i).copied().unwrap_or([0.0; 2]);
            self.hit_vertices.extend([n[0], n[1], n[2], uv[0], uv[1]]);
        }
        let first_index = self.hit_indices.len() as u32;
        self.hit_indices.extend_from_slice(indices);
        self.hit_ranges.push([first_vertex, first_index]);
        self.buffers_stale = true;
        GeometryId(self.geometry.len() as u32 - 1)
    }

    /// An instance that stays put for the rest of the run, like terrain or a building.
    pub fn add_fixed(&mut self, geometry: GeometryId, transform: Mat4, material: HitMaterial) {
        if let Some(place) = self.still.take_place() {
            self.still.set(place, Entry { geometry, transform, number: UNKNOWN, touched: 0 });
            self.paint(place, geometry, material);
        }
    }

    /// The traced copy of an engine mesh, made the first time it's needed.
    pub(crate) fn mesh_geometry(&mut self, gpu: &Gpu, meshes: &Assets<Mesh>, id: u32) -> Option<GeometryId> {
        if let Some(g) = self.meshes.get(&id) {
            return Some(*g);
        }
        let mesh = meshes.get_by_id(id)?;
        if mesh.indices.is_empty() {
            return None;
        }
        let positions: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.position).collect();
        let normals: Vec<[f32; 3]> = mesh.vertices.iter().map(|v| v.normal).collect();
        let uvs: Vec<[f32; 2]> = mesh.vertices.iter().map(|v| v.uv).collect();
        let g = self.add_geometry(gpu, &positions, &normals, &uvs, &mesh.indices);
        self.meshes.insert(id, g);
        Some(g)
    }

    /// The traced copy of a skinned mesh: built from its GPU vertex buffer (the engine's
    /// `Vertex`, positions first) and rebuilt every frame as it moves.
    pub(crate) fn skinned_geometry(&mut self, gpu: &Gpu, id: u32, mesh: &Mesh, vertices: &wgpu::Buffer, indices: &wgpu::Buffer) -> GeometryId {
        if let Some(g) = self.meshes.get(&id) {
            return *g;
        }
        let size = wgpu::BlasTriangleGeometrySizeDescriptor {
            vertex_format: wgpu::VertexFormat::Float32x3,
            vertex_count: mesh.vertices.len() as u32,
            index_format: Some(wgpu::IndexFormat::Uint32),
            index_count: Some(mesh.indices.len() as u32),
            flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
        };
        let blas = gpu.device.create_blas(
            &wgpu::CreateBlasDescriptor {
                label: Some("skinned blas"),
                flags: wgpu::AccelerationStructureFlags::PREFER_FAST_BUILD,
                update_mode: wgpu::AccelerationStructureUpdateMode::Build,
            },
            wgpu::BlasGeometrySizeDescriptors::Triangles {
                descriptors: vec![size.clone()],
            },
        );
        self.geometry.push(Geometry {
            positions: vertices.clone(),
            indices: indices.clone(),
            stride: std::mem::size_of::<super::mesh::Vertex>() as u64,
            size,
            blas,
            built: false,
            dynamic: true,
        });
        // Reflections shade it with its rest-pose normals: near enough.
        let first_vertex = (self.hit_vertices.len() / HIT_STRIDE) as u32;
        for v in &mesh.vertices {
            self.hit_vertices.extend([v.normal[0], v.normal[1], v.normal[2], v.uv[0], v.uv[1]]);
        }
        let first_index = self.hit_indices.len() as u32;
        self.hit_indices.extend_from_slice(&mesh.indices);
        self.hit_ranges.push([first_vertex, first_index]);
        self.buffers_stale = true;
        let g = GeometryId(self.geometry.len() as u32 - 1);
        self.meshes.insert(id, g);
        g
    }

    /// The texture array slot for an image, adding it if there's room.
    pub(crate) fn texture_slot(&mut self, image: u32, view: Option<&wgpu::TextureView>) -> u32 {
        if let Some(slot) = self.slots.get(&image) {
            return *slot;
        }
        let Some(view) = view else {
            return 0;
        };
        if self.textures.len() >= TEXTURE_SLOTS as usize {
            return 0;
        }
        self.textures.push(view.clone());
        let slot = self.textures.len() as u32 - 1;
        self.slots.insert(image, slot);
        self.generation += 1;
        slot
    }

    /// Every texture slot, padded with white to the array's full length.
    pub(crate) fn texture_views(&self) -> Vec<&wgpu::TextureView> {
        (0..TEXTURE_SLOTS as usize)
            .map(|i| self.textures.get(i).unwrap_or(&self.textures[0]))
            .collect()
    }

    /// Whether every piece of geometry has been built into the traced scene.
    pub(crate) fn ready(&self) -> bool {
        self.geometry.iter().all(|g| g.built || g.dynamic)
    }

    /// Identifies the traced scene's contents: its geometry and textures, and how many
    /// instances it holds.
    pub(crate) fn scene_key(&self) -> (u64, usize) {
        (self.generation, self.still.count() + self.moving.count())
    }

    /// Whether anything is among the things that move.
    pub(crate) fn any_moving(&self) -> bool {
        self.moving.count() > 0
    }

    /// Whether an instance of the scene has been looked at and found within reach: traced
    /// against, or left out for what it is. One that has not is to be looked at.
    pub(crate) fn known(&self, number: u32) -> bool {
        self.of.get(number as usize).is_some_and(|place| *place != UNKNOWN)
    }

    /// Where an instance of the scene is kept, making room to say so.
    fn place_of(&mut self, number: u32) -> &mut u32 {
        if number as usize >= self.of.len() {
            self.of.resize(number as usize + 1, UNKNOWN);
        }
        &mut self.of[number as usize]
    }

    /// The structure a place is in, and the place's number within it.
    fn structure(&mut self, place: u32) -> (&mut Structure, u32) {
        if place < MAX_INSTANCES { (&mut self.still, place) } else { (&mut self.moving, place - MAX_INSTANCES) }
    }

    /// Says what a place's material is.
    fn paint(&mut self, place: u32, geometry: GeometryId, material: HitMaterial) {
        self.materials[place as usize] = HitMaterial { geometry: geometry.0, ..material };
        self.stale.push(place);
    }

    /// Traces against an instance of the scene, or brings what is traced up to date with it.
    ///
    /// A new one goes among the things that move, where being built again costs little, and
    /// one at rest that has moved is taken there; they go back among the things that stand
    /// still once they have stood still a while ([`Self::settle`]). One whose material alone
    /// has changed has nothing built for it at all. Where there is no room it is left out.
    pub(crate) fn place(&mut self, number: u32, geometry: GeometryId, transform: Mat4, material: HitMaterial) {
        let entry = Entry { geometry, transform, number, touched: self.frame };
        let current = *self.place_of(number);
        if current != UNKNOWN && current != LEFT_OUT {
            self.paint(current, geometry, material);
            let (structure, local) = self.structure(current);
            let was = structure.entries[local as usize].as_ref().expect("an instance's place holds it");
            if was.geometry == geometry && was.transform == transform {
                return;
            }
            if current >= MAX_INSTANCES {
                structure.set(local, entry);
                return;
            }
            // At rest until now. Among the things that move if there is room for it there;
            // otherwise it is told to the structure it is in, which is built again.
            match self.moving.take_place() {
                Some(moving) => {
                    self.still.clear(current);
                    self.moving.set(moving, entry);
                    *self.place_of(number) = MAX_INSTANCES + moving;
                    self.paint(MAX_INSTANCES + moving, geometry, material);
                }
                None => self.still.set(current, entry),
            }
            return;
        }
        // New to the scene. A few at a time are things appearing in a scene that is there
        // already, and go among the things that move so that the large structure is left
        // alone. A crowd of them at once is a scene being loaded: most of it will never
        // move, and it would only fill the small structure and have to be moved out again.
        self.arrivals += 1;
        let among_moving = if self.arrivals <= ARRIVE_AS_MOVING { self.moving.take_place() } else { None };
        let place = match among_moving {
            Some(moving) => {
                self.moving.set(moving, entry);
                Some(MAX_INSTANCES + moving)
            }
            // Where there is no room among the things that stand still it is left out, and
            // not put among those that move: rays are traced against that structure too, and
            // filled with things at rest it costs every ray dear.
            None => self.still.take_place().inspect(|still| self.still.set(*still, entry)),
        };
        match place {
            Some(place) => {
                *self.place_of(number) = place;
                self.paint(place, geometry, material);
            }
            None => *self.place_of(number) = LEFT_OUT,
        }
    }

    /// Stops tracing against an instance of the scene. `within_reach` says whether it is
    /// still there and near enough, only not a thing rays can be traced against: such a one
    /// is known and not looked at again until it changes.
    pub(crate) fn leave_out(&mut self, number: u32, within_reach: bool) {
        let kept = std::mem::replace(self.place_of(number), if within_reach { LEFT_OUT } else { UNKNOWN });
        if kept != UNKNOWN && kept != LEFT_OUT {
            let (structure, local) = self.structure(kept);
            structure.clear(local);
        }
    }

    /// Takes the things that have stood still a while from among the things that move and
    /// puts them among those that stand still. That builds the large structure again, so
    /// it is done for many at once, or when the small one is filling, or now and then.
    fn settle(&mut self) {
        let idle: Vec<u32> = (0..self.moving.entries.len() as u32)
            .filter(|local| {
                self.moving.entries[*local as usize]
                    .as_ref()
                    .is_some_and(|entry| self.frame - entry.touched >= IDLE &&!self.geometry[entry.geometry.0 as usize].dynamic)
            })
            .collect();
        let filling = self.moving.count() * 4 > MAX_MOVING as usize * 3;
        let due = self.frame - self.settled >= SETTLE_AT_LEAST;
        if idle.is_empty() || !(idle.len() >= SETTLE_MANY || filling || due) {
            return;
        }
        self.settled = self.frame;
        for local in idle {
            let entry = self.moving.clear(local).expect("an idle place holds an instance");
            // Where the large structure is full, a thing at rest is left out and not kept
            // among those that move: a ray is traced against that structure too, and pays
            // for whatever is in it.
            let Some(still) = self.still.take_place() else {
                if entry.number != UNKNOWN {
                    *self.place_of(entry.number) = LEFT_OUT;
                }
                continue;
            };
            if entry.number != UNKNOWN {
                *self.place_of(entry.number) = still;
            }
            self.materials[still as usize] = self.materials[(MAX_INSTANCES + local) as usize];
            self.stale.push(still);
            self.still.set(still, entry);
        }
    }

    /// Uploads the hit buffers if geometry was added, and the materials of the instances
    /// that changed. Once a frame: it is what counts the frames things have stood still for.
    pub(crate) fn upload(&mut self, gpu: &Gpu) {
        self.frame += 1;
        self.arrivals = 0;
        self.settle();
        if self.buffers_stale {
            self.buffers_stale = false;
            self.vertex_buffer = storage(&gpu.device, "hit vertices", bytemuck::cast_slice(&self.hit_vertices));
            self.index_buffer = storage(&gpu.device, "hit indices", bytemuck::cast_slice(&self.hit_indices));
            self.range_buffer = storage(&gpu.device, "hit ranges", bytemuck::cast_slice(&self.hit_ranges));
            self.generation += 1;
        }
        const STRIDE: u64 = std::mem::size_of::<HitMaterial>() as u64;
        for run in super::instances::runs(std::mem::take(&mut self.stale), 32) {
            let stretch = &self.materials[run.start as usize..run.end as usize];
            gpu.queue.write_buffer(&self.instance_buffer, run.start as u64 * STRIDE, bytemuck::cast_slice(stretch));
        }
    }

    /// Records the acceleration structure builds for this frame: new geometry, geometry
    /// that moves, and each top-level structure in which an instance has changed.
    pub(crate) fn build(&mut self, encoder: &mut wgpu::CommandEncoder) {
        // Build a few million triangles' worth of new geometry a frame at most: a big scene
        // built all at once can keep the GPU busy long enough for Windows to reset it.
        let mut budget = 2_000_000u32;
        let pending: Vec<usize> = (0..self.geometry.len())
            .filter(|i| !self.geometry[*i].built && !self.geometry[*i].dynamic)
            .take_while(|i| {
                let triangles = self.geometry[*i].size.index_count.unwrap_or(0) / 3;
                let go = budget > 0;
                budget = budget.saturating_sub(triangles);
                go
            })
            .chain((0..self.geometry.len()).filter(|i| self.geometry[*i].dynamic))
            .collect();
        // Each structure is told of its places that changed, and is built if any did, or if
        // geometry under one of its instances is built again this frame, or if it never was.
        let build_still = self.still.tell(&self.geometry, &pending);
        let build_moving = self.moving.tell(&self.geometry, &pending);
        if !build_still && !build_moving && pending.is_empty() {
            return;
        }
        let mut structures: Vec<&wgpu::Tlas> = Vec::new();
        if build_still {
            structures.push(&self.still.tlas);
        }
        if build_moving {
            structures.push(&self.moving.tlas);
        }
        let entries: Vec<wgpu::BlasBuildEntry> = pending
            .iter()
            .map(|&i| {
                let g = &self.geometry[i];
                wgpu::BlasBuildEntry {
                    blas: &g.blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![wgpu::BlasTriangleGeometry {
                        size: &g.size,
                        vertex_buffer: &g.positions,
                        first_vertex: 0,
                        vertex_stride: g.stride,
                        index_buffer: Some(&g.indices),
                        first_index: Some(0),
                        transform_buffer: None,
                        transform_buffer_offset: None,
                    }]),
                }
            })
            .collect();
        encoder.build_acceleration_structures(entries.iter(), structures);
        drop(entries);
        for i in pending {
            self.geometry[i].built = true;
        }
    }
}

