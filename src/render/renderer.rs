use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use glam::Vec3;
use wgpu::util::DeviceExt;

use super::{
    cull::{self, Culler},
    gpu::{main_depth_state, main_multisample, Gpu, HDR_FORMAT},
    image::Image,
    instances::{Instances, Viewpoint},
    mesh::{Mesh, Vertex},
    shadow::{shadow_depth_state, ShadowMaps},
    view::ViewBinding,
    Material, RenderFrame,
};
use crate::assets::Assets;

/// What a batch of instances shares: the mesh, its textures and its pipeline variant.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct BatchKey {
    variant: u8,
    /// The surface shader, counted from one; nought is the standard surface.
    surface: u16,
    mesh: u32,
    textures: TextureSet,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
struct TextureSet {
    base_color: u32,
    normal: u32,
    metallic_roughness: u32,
    height: u32,
}

const NONE: u32 = u32::MAX;

impl BatchKey {
    pub(crate) fn new(mesh: u32, m: &Material, surface: u16) -> Self {
        let id = |h: Option<crate::assets::Handle<Image>>| h.map_or(NONE, |h| h.id());
        Self {
            // Decals sort after everything else, so they lie over what is already drawn.
            variant: m.alpha_cutoff.is_some() as u8 | (m.double_sided as u8) << 1 | (m.decal as u8) << 2,
            surface,
            mesh,
            textures: TextureSet {
                base_color: id(m.base_color_texture),
                normal: id(m.normal_texture),
                metallic_roughness: id(m.metallic_roughness_texture),
                height: id(m.height_texture),
            },
        }
    }

    pub(crate) fn mesh(&self) -> u32 {
        self.mesh
    }

    fn masked(&self) -> bool {
        self.variant & 1 != 0
    }
}

struct GpuMesh {
    /// Bounding sphere in mesh space, for culling.
    center: Vec3,
    radius: f32,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

struct Batch {
    key: BatchKey,
    instances: Range<u32>,
}

/// One of the main pass's pipelines for meshes, drawing with `shader`.
fn main_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    masked: bool,
    double_sided: bool,
    decal: bool,
    lines: bool,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mesh pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(Vertex::layout())],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: decal.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: (!double_sided).then_some(wgpu::Face::Back),
            polygon_mode: if lines { wgpu::PolygonMode::Line } else { wgpu::PolygonMode::Fill },
            ..Default::default()
        },
        depth_stencil: Some(if decal {
            // Tested against the surface beneath but not written, and pulled a little
            // toward the eye (depth is reversed) so it never flickers through it.
            wgpu::DepthStencilState {
                bias: wgpu::DepthBiasState {
                    constant: 8,
                    slope_scale: 1.5,
                    clamp: 0.0,
                },
                ..main_depth_state(false)
            }
        } else {
            main_depth_state(true)
        }),
        multisample: main_multisample(masked && !decal),
        multiview_mask: None,
        cache: None,
    })
}

/// The main pass's pipelines for one shader, by `BatchKey::variant`.
fn main_pipelines(device: &wgpu::Device, layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule) -> [wgpu::RenderPipeline; 8] {
    std::array::from_fn(|variant| main_pipeline(device, layout, shader, variant & 1 != 0, variant & 2 != 0, variant & 4 != 0, false))
}

/// The numbers of the instances a view sees, as the card holds them.
struct SeenOnCard {
    numbers: wgpu::Buffer,
    /// How many numbers there is room for.
    room: usize,
    /// The instances and the numbers bound together for the mesh shaders, with the
    /// generation of the instance buffer it was made for.
    bound: Option<(u64, wgpu::BindGroup)>,
}

/// A surface shader an entity named, and the pipelines that draw with it.
struct SurfacePipelines {
    /// None until they are made, and where the shader could not be built: such a surface is
    /// drawn the standard way.
    pipelines: Option<[wgpu::RenderPipeline; 8]>,
    tried: bool,
}

/// Draws every `Mesh3d` with one instanced draw call per mesh and material.
pub struct MeshRenderer {
    /// Indexed by `BatchKey::variant`: bit 0 alpha-tested, bit 1 double-sided, bit 2 decal.
    pipelines: [wgpu::RenderPipeline; 8],
    /// What the main pass's pipelines are laid out as, kept for those made later.
    layout: wgpu::PipelineLayout,
    /// The surface shaders entities have named, in the order they were first met: a batch
    /// knows one by its place here, counted from one. Named while the frame is gathered,
    /// which only reads the renderer.
    surface_names: Mutex<Vec<String>>,
    /// The pipelines made for each of those, where its shader could be built.
    surfaces: Vec<SurfacePipelines>,
    /// Draws triangles as their edges, where the graphics card can.
    wire: Option<wgpu::RenderPipeline>,
    /// Whether to draw with `wire` this frame.
    pub(crate) wireframe: bool,
    shadow_opaque: wgpu::RenderPipeline,
    shadow_masked: wgpu::RenderPipeline,
    material_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    defaults: [wgpu::TextureView; 4],
    images: HashMap<u32, wgpu::TextureView>,
    /// The pictures that are drawn with their pixels sharp, and the sampler that does it.
    pixelated: std::collections::HashSet<u32>,
    sharp: wgpu::Sampler,
    materials: HashMap<TextureSet, wgpu::BindGroup>,
    /// Every instance there is, kept from frame to frame, and its copy on the card.
    pub(crate) scene: Instances,
    /// What the mesh shaders read instances through: the instances, and the numbers of those
    /// the view being drawn sees.
    scene_layout: wgpu::BindGroupLayout,
    /// Those numbers on the card.
    seen: Option<SeenOnCard>,
    meshes: HashMap<u32, GpuMesh>,
    batches: Vec<Batch>,
    /// For each shadow cascade, what casts into it.
    shadow_batches: Vec<Vec<Batch>>,
    /// The cascade being drawn, said by the shadow pass before each one's draws.
    cascade: AtomicUsize,
    /// Decides on the card what each view sees, where the card can: see `cull.rs`. Then
    /// the two lists above are empty, and every batch is drawn with a draw the card filled
    /// in: the batches in the order they are drawn, each with its number, and how many
    /// cascades there are draws for.
    culler: Option<Culler>,
    drawn: Vec<(BatchKey, u32)>,
    cascades: usize,
    light_maps: usize,
}

impl MeshRenderer {
    pub fn new(gpu: &Gpu, view: &ViewBinding, shadows: &ShadowMaps) -> Self {
        let device = &gpu.device;
        let shader = gpu.shader(crate::shader!("mesh.wgsl"));
        let shadow_shader = crate::shader!("mesh_shadow.wgsl").module(device);

        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
            },
            count: None,
        };
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instances layout"),
            entries: &[storage(0), storage(1), storage(2)],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh pipeline layout"),
            bind_group_layouts: &[Some(&view.layout), Some(&material_layout), Some(&scene_layout)],
            immediate_size: 0,
        });
        let pipelines = main_pipelines(device, &layout, &shader);
        let wire = device
            .features()
            .contains(wgpu::Features::POLYGON_MODE_LINE)
            .then(|| main_pipeline(device, &layout, &shader, false, true, false, true));

        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh shadow layout"),
            bind_group_layouts: &[Some(&shadows.layout), Some(&material_layout), Some(&scene_layout)],
            immediate_size: 0,
        });
        let shadow_pipeline = |masked: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh shadow pipeline"),
                layout: Some(&shadow_layout),
                vertex: wgpu::VertexState {
                    module: &shadow_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(Vertex::layout())],
                    compilation_options: Default::default(),
                },
                fragment: masked.then(|| wgpu::FragmentState {
                    module: &shadow_shader,
                    entry_point: Some("fs_mask"),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                // Both faces cast, so thin and open geometry still blocks the sun.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(shadow_depth_state()),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        // For pixel art: the nearest pixel where a picture is shown larger than it is, and
        // smoothed as any other where it is shown smaller.
        let sharp = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("pixelated material sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let solid = |rgba, srgb| Image::solid(rgba, srgb).upload(device, &gpu.queue);
        let defaults = [
            solid([255, 255, 255, 255], true),
            solid([128, 128, 255, 255], false),
            solid([255, 255, 255, 255], false),
            solid([0, 0, 0, 255], false),
        ];

        Self {
            pipelines,
            layout,
            surface_names: Mutex::default(),
            surfaces: Vec::new(),
            wire,
            wireframe: false,
            shadow_opaque: shadow_pipeline(false),
            shadow_masked: shadow_pipeline(true),
            material_layout,
            sampler,
            defaults,
            images: HashMap::new(),
            pixelated: Default::default(),
            sharp,
            materials: HashMap::new(),
            scene: Instances::default(),
            scene_layout,
            seen: None,
            meshes: HashMap::new(),
            batches: Vec::new(),
            shadow_batches: Vec::new(),
            culler: cull::supported(gpu).then(|| Culler::new(gpu)),
            drawn: Vec::new(),
            cascades: 0,
            light_maps: 0,
            cascade: AtomicUsize::new(0),
        }
    }

    /// Takes the pipelines of a renderer freshly built from the current shaders, keeping
    /// this one's meshes, textures and materials.
    pub(crate) fn adopt_pipelines(&mut self, fresh: Self) {
        self.pipelines = fresh.pipelines;
        self.layout = fresh.layout;
        // Its layout is the one the new pipelines were made with, so what is bound to it is
        // bound again.
        self.scene_layout = fresh.scene_layout;
        if let Some(seen) = &mut self.seen {
            seen.bound = None;
        }
        // The surface shaders are built again from what they now say, when next drawn.
        self.surfaces.clear();
        self.wire = fresh.wire;
        self.shadow_opaque = fresh.shadow_opaque;
        self.shadow_masked = fresh.shadow_masked;
    }

    /// Uploads meshes that were added or modified, and frees removed ones.
    pub fn sync_meshes(&mut self, gpu: &Gpu, meshes: &mut Assets<Mesh>) {
        let (modified, removed) = meshes.take_changes();
        for id in removed {
            self.meshes.remove(&id);
        }
        for id in modified {
            let Some(mesh) = meshes.get_by_id(id) else {
                continue;
            };
            if mesh.indices.is_empty() {
                self.meshes.remove(&id);
                continue;
            }
            // Skinning writes posed vertices into the buffer, and the ray-traced scene can build
            // from it directly.
            let traced = if gpu.ray_tracing { wgpu::BufferUsages::BLAS_INPUT } else { wgpu::BufferUsages::empty() };
            let vertices = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh vertices"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE | traced,
                });
            let indices = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh indices"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX | traced,
                });
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for v in &mesh.vertices {
                lo = lo.min(Vec3::from(v.position));
                hi = hi.max(Vec3::from(v.position));
            }
            let center = (lo + hi) * 0.5;
            let radius = mesh
                .vertices
                .iter()
                .map(|v| Vec3::from(v.position).distance(center))
                .fold(0.0, f32::max);
            self.meshes.insert(
                id,
                GpuMesh {
                    center,
                    radius,
                    vertices,
                    indices,
                    index_count: mesh.indices.len() as u32,
                },
            );
        }
    }

    /// Uploads new images. Material bind groups that used a changed image are rebuilt.
    pub fn sync_images(&mut self, gpu: &Gpu, images: &mut Assets<Image>) {
        let (modified, removed) = images.take_changes();
        if modified.is_empty() && removed.is_empty() {
            return;
        }
        for id in &removed {
            self.images.remove(id);
            self.pixelated.remove(id);
        }
        // Mip chains take a while for big textures; make them on every core at once.
        let arrived: Vec<(u32, &Image)> = modified.iter().filter_map(|id| images.get_by_id(*id).map(|image| (*id, image))).collect();
        for (id, image) in &arrived {
            if image.pixelated {
                self.pixelated.insert(*id);
            } else {
                self.pixelated.remove(id);
            }
        }
        // What was processed ahead of time goes up as it is.
        for (id, image) in &arrived {
            if let Some(processed) = &image.processed {
                self.images.insert(*id, processed.upload(&gpu.device, &gpu.queue));
            }
        }
        let pending: Vec<(u32, &Image)> = arrived.into_iter().filter(|(_, image)| image.processed.is_none()).collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let chains: Vec<Vec<(u32, u32, Vec<u8>)>> = std::thread::scope(|s| {
            let chunk = pending.len().div_ceil(threads).max(1);
            let jobs: Vec<_> = pending
                .chunks(chunk)
                .map(|part| s.spawn(move || part.iter().map(|(_, image)| image.mip_chain()).collect::<Vec<_>>()))
                .collect();
            jobs.into_iter().flat_map(|j| j.join().expect("mip generation panicked")).collect()
        });
        // Send the uploads in batches of their own, so no one submission keeps the GPU busy for
        // long (Windows resets a GPU that is unresponsive for two seconds).
        let mut batch = 0usize;
        for ((id, image), levels) in pending.iter().zip(&chains) {
            self.images.insert(*id, image.upload_levels(&gpu.device, &gpu.queue, levels));
            batch += levels.iter().map(|(_, _, data)| data.len()).sum::<usize>();
            if batch > 128 << 20 {
                gpu.queue.submit([]);
                batch = 0;
            }
        }
        gpu.queue.submit([]);
        let changed = |t: &TextureSet| {
            [t.base_color, t.normal, t.metallic_roughness, t.height]
                .iter()
                .any(|id| modified.contains(id) || removed.contains(id))
        };
        self.materials.retain(|set, _| !changed(set));
    }

    fn material(&mut self, gpu: &Gpu, set: TextureSet) {
        if self.materials.contains_key(&set) {
            return;
        }
        let pick = |id: u32, slot: usize| {
            self.images.get(&id).unwrap_or(&self.defaults[slot])
        };
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &self.material_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(pick(set.base_color, 0)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(pick(set.normal, 1)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(pick(set.metallic_roughness, 2)),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(pick(set.height, 3)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    // A material whose colour is pixel art is drawn sharp throughout.
                    resource: wgpu::BindingResource::Sampler(if self.pixelated.contains(&set.base_color) { &self.sharp } else { &self.sampler }),
                },
            ],
        });
        self.materials.insert(set, bind_group);
    }

    /// How a batch knows the surface shader of this name: its place among those named so
    /// far, counted from one. Its pipelines are made when it is first drawn.
    pub(crate) fn surface(&self, name: &str) -> u16 {
        let mut names = self.surface_names.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let at = names.iter().position(|known| known == name).unwrap_or_else(|| {
            names.push(name.to_owned());
            names.len() - 1
        });
        at as u16 + 1
    }

    /// Makes the pipelines of surface shaders that have been named and not yet built. The
    /// mesh shader is put together with the surface's module taken in, and `SURFACE` defined
    /// so that it is asked. A shader that is not there, or will not compile, is said once
    /// and its surfaces are drawn the standard way.
    fn build_surfaces(&mut self, gpu: &Gpu) {
        let names = self.surface_names.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        while self.surfaces.len() < names.len() {
            self.surfaces.push(SurfacePipelines { pipelines: None, tried: false });
        }
        for (surface, name) in self.surfaces.iter_mut().zip(&names).filter(|(surface, _)| !surface.tried) {
            surface.tried = true;
            let source = format!("{}\n#import {name}\n", crate::shader!("mesh.wgsl").source());
            let whole = match gpu.compose_with(&source, &["SURFACE"]) {
                Ok(whole) => whole,
                Err(mistake) => {
                    log::error!("the surface shader `{name}` is drawn the standard way: {mistake}");
                    continue;
                }
            };
            // Collected, where wgpu would otherwise stop the program at the first.
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(whole.into()),
            });
            let pipelines = main_pipelines(&gpu.device, &self.layout, &module);
            match pollster::block_on(scope.pop()) {
                Some(error) => log::error!("the surface shader `{name}` is drawn the standard way:\n{error}"),
                None => surface.pipelines = Some(pipelines),
            }
        }
    }

    /// How many instances are kept on the card: one for each mesh an entity draws, and one
    /// for each of its levels of detail.
    pub fn instance_count(&self) -> usize {
        self.scene.len()
    }

    /// The bounding sphere of an uploaded mesh, in mesh space.
    pub fn bounds(&self, mesh: u32) -> Option<(Vec3, f32)> {
        self.meshes.get(&mesh).map(|m| (m.center, m.radius))
    }

    /// Decides what the view sees and readies its draws: the instances that changed are
    /// sent to the card, and then only the numbers of those in view, by batch, for the view
    /// and for the shadow cascades.
    pub(crate) fn prepare_draws(&mut self, gpu: &Gpu, frame: &mut RenderFrame, from: &Viewpoint) {
        self.build_surfaces(gpu);
        let meshes = &self.meshes;
        let on_card = self.culler.is_some();
        if on_card {
            self.scene.size(|mesh| meshes.get(&mesh).map(|m| (m.center, m.radius)));
        }
        self.scene.upload(gpu);
        // Where the card decides what is seen, it is given room to list it; otherwise it is
        // decided here and the list sent.
        let seen = if on_card {
            self.drawn = self.scene.batches().map(|(key, number, _)| (key, number)).collect();
            self.cascades = from.cascades.len();
            self.light_maps = from.lights.len();
            frame.moved = if from.blended { self.scene.moved_near(from.camera) } else { Vec::new() };
            None
        } else {
            let seen = self.scene.seen(from, |mesh| meshes.get(&mesh).map(|m| (m.center, m.radius)));
            let batch = |(key, instances): &(BatchKey, Range<u32>)| Batch { key: *key, instances: instances.clone() };
            self.batches = seen.batches.iter().map(batch).collect();
            self.shadow_batches = seen.shadow_batches.iter().map(|cascade| cascade.iter().map(batch).collect()).collect();
            frame.moved = seen.moved;
            Some(seen.numbers)
        };
        let sets: Vec<_> = self.batches.iter().chain(self.shadow_batches.iter().flatten()).map(|b| b.key.textures).chain(self.drawn.iter().map(|(key, _)| key.textures)).collect();
        for set in sets {
            self.material(gpu, set);
        }

        let listed = seen.as_ref().map_or_else(|| Culler::listed(&self.scene, from), Vec::len);
        if self.seen.as_ref().is_none_or(|on_card| listed > on_card.room) {
            let room = listed.next_power_of_two().max(256);
            let numbers = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances in view"),
                size: (room * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.seen = Some(SeenOnCard { numbers, room, bound: None });
        }
        let SeenOnCard { numbers, bound, .. } = self.seen.as_mut().unwrap();
        match (&seen, &mut self.culler) {
            (Some(seen), _) => gpu.queue.write_buffer(numbers, 0, bytemuck::cast_slice(seen)),
            (None, Some(culler)) => {
                let meshes = &self.meshes;
                culler.prepare(gpu, &self.scene, from, numbers, |mesh| meshes.get(&mesh).map(|m| m.index_count));
            }
            (None, None) => unreachable!("what is seen is decided here or on the card"),
        }
        // Bound again whenever either buffer is another one.
        if bound.as_ref().is_none_or(|(generation, _)| *generation != self.scene.generation) {
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("instances"),
                layout: &self.scene_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.scene.buffer().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: numbers.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.scene.looks().as_entire_binding() },
                ],
            });
            *bound = Some((self.scene.generation, group));
        }
    }

    /// Whether the card decides what each view sees, which the shadows of lights with a
    /// place need.
    pub(crate) fn decides_on_card(&self) -> bool {
        self.culler.is_some()
    }

    /// Records the pass in which the card decides what each view sees, where it does.
    pub(crate) fn cull(&self, encoder: &mut wgpu::CommandEncoder) {
        if let Some(culler) = &self.culler {
            culler.run(encoder);
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        match (&self.wire, self.wireframe) {
            (Some(wire), true) => self.draw_with(pass, 0, &self.batches, |_| wire),
            _ => self.draw_with(pass, 0, &self.batches, |key| {
                let own = key.surface.checked_sub(1).and_then(|at| self.surfaces.get(at as usize));
                &own.and_then(|surface| surface.pipelines.as_ref()).unwrap_or(&self.pipelines)[key.variant as usize]
            }),
        }
    }

    /// A mesh's vertex and index buffers on the GPU, and how many indices there are.
    pub(crate) fn buffers(&self, mesh: u32) -> Option<(&wgpu::Buffer, &wgpu::Buffer, u32)> {
        self.meshes.get(&mesh).map(|m| (&m.vertices, &m.indices, m.index_count))
    }

    /// A mesh's vertex and index buffers on the GPU.
    pub(crate) fn vertex_buffer(&self, mesh: u32) -> Option<&wgpu::Buffer> {
        self.meshes.get(&mesh).map(|m| &m.vertices)
    }

    pub(crate) fn index_buffer(&self, mesh: u32) -> Option<&wgpu::Buffer> {
        self.meshes.get(&mesh).map(|m| &m.indices)
    }

    /// The uploaded texture for an image.
    pub(crate) fn image_view(&self, image: u32) -> Option<&wgpu::TextureView> {
        self.images.get(&image)
    }

    /// Says which shadow cascade the draws that follow are for.
    pub(crate) fn begin_cascade(&self, cascade: usize) {
        self.cascade.store(cascade, Ordering::Relaxed);
    }

    /// Draws what casts a shadow into the cascade being drawn.
    pub fn draw_shadows(&self, pass: &mut wgpu::RenderPass<'_>) {
        let cascade = self.cascade.load(Ordering::Relaxed);
        let batches = match self.shadow_batches.get(cascade) {
            Some(batches) => batches.as_slice(),
            // Where the card decided what is seen: a cascade there are draws for, or, after
            // the cascades, a light's shadow map.
            None if self.culler.is_some() && (cascade < self.cascades.min(cull::LIGHTS - 1) || (cull::LIGHTS - 1..cull::LIGHTS - 1 + self.light_maps).contains(&cascade)) => &[],
            None => return,
        };
        self.draw_with(pass, 1 + cascade, batches, |key| {
            if key.masked() {
                &self.shadow_masked
            } else {
                &self.shadow_opaque
            }
        });
    }

    fn draw_with<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'_>,
        view: usize,
        batches: &[Batch],
        pipeline: impl Fn(&BatchKey) -> &'a wgpu::RenderPipeline,
    ) {
        if batches.is_empty() && self.drawn.is_empty() {
            return;
        }
        let Some((_, instances)) = self.seen.as_ref().and_then(|seen| seen.bound.as_ref()) else {
            return;
        };
        pass.set_bind_group(2, instances, &[]);
        let mut current: Option<*const wgpu::RenderPipeline> = None;
        // Where the card decided what the view sees, every batch is drawn with the draw it
        // filled in, which is of no instances where it saw none.
        if let Some(culler) = &self.culler {
            for (key, number) in &self.drawn {
                let (Some(mesh), Some(material)) = (self.meshes.get(&key.mesh), self.materials.get(&key.textures)) else { continue };
                let p = pipeline(key);
                if current != Some(p as *const _) {
                    pass.set_pipeline(p);
                    current = Some(p as *const _);
                }
                pass.set_bind_group(1, material, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed_indirect(&culler.draws, culler.draw(view, *number));
            }
            return;
        }
        for batch in batches {
            let Some(mesh) = self.meshes.get(&batch.key.mesh) else {
                continue;
            };
            let Some(material) = self.materials.get(&batch.key.textures) else {
                continue;
            };
            let p = pipeline(&batch.key);
            if current != Some(p as *const _) {
                pass.set_pipeline(p);
                current = Some(p as *const _);
            }
            pass.set_bind_group(1, material, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, batch.instances.clone());
        }
    }
}
