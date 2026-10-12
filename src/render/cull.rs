//! What each view sees, decided on the graphics card.
//!
//! The processor's way ([`Instances::seen`]) goes through every instance every frame for
//! the frame's view and for each shadow cascade, and sends the card the numbers of those
//! seen. Here the card goes through them itself, in a compute pass (`cull.wgsl`): it holds
//! where each instance is already, and is told once what else it needs (its mesh's size, the
//! distances it is drawn over, its batch). It lists what each view sees batch by batch, and
//! writes how many there are of each into the draw the batch is then drawn with.
//!
//! So a frame costs the processor nothing for an instance that has not changed: what is
//! sent is five numbers a batch for each view.

use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    gpu::Gpu,
    instances::{unit_planes, Instances, Viewpoint},
};

/// The frame's view, the sun's four cascades, and the lights that cast shadows: each has a
/// place of its own among the views, used or not.
pub(crate) const VIEWS: usize = LIGHTS + super::lights::SHADOWED;
/// Where the lights' views begin.
pub(crate) const LIGHTS: usize = 1 + super::shadow::CASCADES;
/// The bytes of a draw as the card reads one for an indirect draw: how many indices, how
/// many instances, the first index, what is added to an index, the first instance.
pub(crate) const DRAW: u64 = 20;

/// Must match `Views` in `cull.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewsRaw {
    camera: [f32; 4],
    counts: [u32; 4],
    planes: [[f32; 4]; VIEWS * 5],
}

/// Whether the card can decide what is seen: it must be able to begin an indirect draw's
/// instances at a number of our choosing. `MIRA_CULL=cpu` keeps the processor's way.
pub(crate) fn supported(gpu: &Gpu) -> bool {
    gpu.device.features().contains(wgpu::Features::INDIRECT_FIRST_INSTANCE) && std::env::var("MIRA_CULL").as_deref() != Ok("cpu")
}

pub(crate) struct Culler {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    views: wgpu::Buffer,
    /// A draw for each batch, for each view, and how many batches there is room for.
    pub(crate) draws: wgpu::Buffer,
    room: usize,
    /// What the pass is bound to, with the buffers it was bound for: the instances' (by
    /// their generation), the draws' and the list's (by their sizes).
    bound: Option<((u64, u64, u64), wgpu::BindGroup)>,
    /// How many instances the pass goes through, set as the frame is prepared.
    numbers: AtomicU32,
}

impl Culler {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let (read, written) = (wgpu::BufferBindingType::Storage { read_only: true }, wgpu::BufferBindingType::Storage { read_only: false });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("culling layout"),
            entries: &[buffer(0, read), buffer(1, read), buffer(2, wgpu::BufferBindingType::Uniform), buffer(3, written), buffer(4, written)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("culling pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("culling"),
            layout: Some(&pipeline_layout),
            module: &crate::shader!("cull.wgsl").module(device),
            entry_point: Some("cull"),
            compilation_options: Default::default(),
            cache: None,
        });
        let views = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("culling views"),
            size: std::mem::size_of::<ViewsRaw>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let room = 64;
        Self { pipeline, layout, views, draws: Self::draws(device, room), room, bound: None, numbers: AtomicU32::new(0) }
    }

    fn draws(device: &wgpu::Device, room: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("draws by batch"),
            size: (VIEWS * room) as u64 * DRAW,
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Where in the draws a batch's is, for a view: nought is the frame's, then the cascades.
    pub(crate) fn draw(&self, view: usize, batch: u32) -> u64 {
        (view * self.room + batch as usize) as u64 * DRAW
    }

    /// The views a frame has, each with its place among them: the frame's, the cascades',
    /// and the shadow-casting lights'.
    fn views(from: &Viewpoint) -> Vec<(usize, glam::Mat4)> {
        let cascades = from.cascades.iter().take(LIGHTS - 1).enumerate().map(|(i, matrix)| (1 + i, *matrix));
        let lights = from.lights.iter().take(VIEWS - LIGHTS).enumerate().map(|(i, matrix)| (LIGHTS + i, *matrix));
        std::iter::once((0, from.view_proj)).chain(cascades).chain(lights).collect()
    }

    /// How many numbers the list of what is seen must have room for: every view may see
    /// every instance.
    pub(crate) fn listed(scene: &Instances, from: &Viewpoint) -> usize {
        Self::views(from).len() * scene.numbers()
    }

    /// Readies the pass for a view of the scene: the draws are set out with nothing in
    /// them, each told where its stretch of the list begins, and the pass is told the
    /// view. `seen` is the list it writes, and `indices` says how many indices a mesh has
    /// once it is on the card.
    pub(crate) fn prepare(&mut self, gpu: &Gpu, scene: &Instances, from: &Viewpoint, seen: &wgpu::Buffer, indices: impl Fn(u32) -> Option<u32>) {
        if scene.batch_numbers() > self.room {
            self.room = scene.batch_numbers().next_power_of_two();
            self.draws = Self::draws(&gpu.device, self.room);
        }
        // A view sees at most all of a batch, so each batch has that much of the list for
        // each view, one view after another.
        let mut draws = vec![[0u32; 5]; VIEWS * self.room];
        let mut first = 0;
        let views = Self::views(from);
        for (view, _) in &views {
            for (key, batch, instances) in scene.batches() {
                draws[view * self.room + batch as usize] = [indices(key.mesh()).unwrap_or(0), 0, 0, 0, first];
                first += instances;
            }
        }
        gpu.queue.write_buffer(&self.draws, 0, bytemuck::cast_slice(&draws));

        let mut raw = ViewsRaw {
            camera: from.camera.extend(from.shadow_reach).to_array(),
            // Which of the views there are this frame, a bit for each.
            counts: [LIGHTS as u32, self.room as u32, scene.numbers() as u32, views.iter().fold(0, |mask, (view, _)| mask | 1 << view)],
            planes: [[0.0; 4]; VIEWS * 5],
        };
        for (view, matrix) in views {
            for (at, (normal, distance)) in unit_planes(matrix).into_iter().enumerate() {
                raw.planes[view * 5 + at] = normal.extend(distance).to_array();
            }
        }
        gpu.queue.write_buffer(&self.views, 0, bytemuck::bytes_of(&raw));
        self.numbers.store(scene.numbers() as u32, Ordering::Relaxed);

        let made_for = (scene.generation, self.draws.size(), seen.size());
        if self.bound.as_ref().is_none_or(|(was, _)| *was != made_for) {
            fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
                wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() }
            }
            let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("culling"),
                layout: &self.layout,
                entries: &[entry(0, scene.buffer()), entry(1, scene.sights()), entry(2, &self.views), entry(3, &self.draws), entry(4, seen)],
            });
            self.bound = Some((made_for, group));
        }
    }

    /// Records the pass.
    pub(crate) fn run(&self, encoder: &mut wgpu::CommandEncoder) {
        let (Some((_, group)), numbers) = (&self.bound, self.numbers.load(Ordering::Relaxed)) else { return };
        if numbers == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("culling"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(numbers.div_ceil(64), 1, 1);
    }
}
