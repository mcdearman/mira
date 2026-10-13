//! Particles: smoke, dust, spray, sparks and flashes.
//!
//! A particle is a soft puff that faces the camera. It is born with a velocity, slows by its
//! drag, rises or sinks, is carried by the wind as it slows, spreads, and thins away over its
//! life. Smoke is lit by the sun and the sky, shadowed and hazed as the scene is; a flash is
//! light of its own, added to the picture.
//!
//! Anything may throw particles out, by calling [`Particles::emit`]: a musket's discharge is
//! a dozen at once, each a little different. A [`ParticleEmitter`] on an entity lets them out
//! steadily from where the entity is, for a chimney or a fire.
//!
//! They are moved on the processor and drawn in one instanced draw, back to front, after
//! everything opaque. There may be sixteen thousand at once.

use glam::{Quat, Vec2, Vec3};

use super::{main_depth_state, main_multisample, Gpu, Image, RenderFrame, TextureArray, ViewBinding, HDR_FORMAT};
use crate::{
    ecs::{Component, Query, Res, ResMut, World},
    time::Time,
    transform::GlobalTransform,
    voxel::fbm,
};

const PUFF_SIZE: u32 = 256;
const PUFF_LAYERS: u32 = 4;
/// The most particles there are at once: those let out past it are lost.
pub const MAX_PARTICLES: usize = 16_384;

/// One particle. Make one with [`Particles::puff`] and change what is to differ.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Seconds since it was let out, and how many it lasts.
    pub age: f32,
    pub life: f32,
    /// Radius when born, and how many times bigger it spreads to.
    pub size: f32,
    pub spread: f32,
    /// Its colour; for a flash, the light it gives.
    pub color: Vec3,
    pub opacity: f32,
    /// Upward drift, metres a second: hot smoke climbs, cold powder smoke hangs, spray falls.
    pub rise: f32,
    /// How quickly it loses its own velocity.
    pub drag: f32,
    pub rotation: f32,
    pub spin: f32,
    /// Which of the puff shapes it is drawn with.
    pub layer: u32,
    /// Light of its own, added to the picture, where smoke is lit and covers what is behind.
    pub flash: bool,
}

impl Default for Particle {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            age: 0.0,
            life: 10.0,
            size: 0.5,
            spread: 4.0,
            color: Vec3::splat(0.8),
            opacity: 0.9,
            rise: 0.2,
            drag: 1.2,
            rotation: 0.0,
            spin: 0.0,
            layer: 0,
            flash: false,
        }
    }
}

/// Every particle there is.
pub struct Particles {
    particles: Vec<Particle>,
    seed: u32,
    /// The breeze, which carries smoke off as it slows.
    pub wind: Vec3,
}

impl Default for Particles {
    fn default() -> Self {
        Self { particles: Vec::new(), seed: 0, wind: Vec3::ZERO }
    }
}

impl Particles {
    /// A number from nought up to one, different each time: for making particles that
    /// differ a little from each other.
    pub fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Lets a particle out. Lost if there are as many as there may be.
    pub fn emit(&mut self, particle: Particle) {
        if self.particles.len() < MAX_PARTICLES {
            self.particles.push(particle);
        }
    }

    /// A puff of pale smoke, turned and shaped at random, to be changed and let out.
    pub fn puff(&mut self, position: Vec3, velocity: Vec3) -> Particle {
        let rotation = self.random() * std::f32::consts::TAU;
        let layer = ((self.random() * PUFF_LAYERS as f32) as u32).min(PUFF_LAYERS - 1);
        let spin = (self.random() - 0.5) * 0.2;
        Particle { position, velocity, rotation, layer, spin, ..Default::default() }
    }

    pub fn len(&self) -> usize {
        self.particles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Moves them on by a moment, and lets go of those whose time is up.
    fn step(&mut self, dt: f32) {
        let wind = self.wind;
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.life {
                return false;
            }
            p.velocity *= (-p.drag * dt).exp();
            // Smoke picks up the breeze as it slows.
            let carried = 1.0 - (-p.age * 0.6).exp();
            p.position += (p.velocity + wind * carried + Vec3::Y * p.rise) * dt;
            p.rotation += p.spin * dt;
            true
        });
    }
}

/// Lets particles out steadily from where its entity is: so many a second, each like
/// `particle` but turned and shaped at random, and thrown a little aside by `scatter`. The
/// particle's velocity is turned as the entity is.
#[derive(Clone, Copy, Debug)]
pub struct ParticleEmitter {
    /// How many a second.
    pub rate: f32,
    /// What each is like when let out; where it is, is where the entity is.
    pub particle: Particle,
    /// How far each one's velocity strays from the particle's, in metres a second.
    pub scatter: f32,
    /// What is owed of a particle since the last was let out.
    owed: f32,
}

impl Component for ParticleEmitter {}

impl ParticleEmitter {
    pub fn new(rate: f32, particle: Particle) -> Self {
        Self { rate, particle, scatter: 0.0, owed: 0.0 }
    }

    pub fn with_scatter(mut self, scatter: f32) -> Self {
        self.scatter = scatter;
        self
    }
}

/// Lets out what each emitter owes, and moves every particle on.
pub(crate) fn simulate(time: Res<Time>, mut particles: ResMut<Particles>, mut emitters: Query<(&mut ParticleEmitter, &GlobalTransform)>) {
    let dt = time.delta_secs();
    for (mut emitter, at) in &mut emitters {
        emitter.owed += emitter.rate.max(0.0) * dt;
        let (_, turn, position) = at.0.to_scale_rotation_translation();
        while emitter.owed >= 1.0 {
            emitter.owed -= 1.0;
            let aside = Vec3::new(particles.random() - 0.5, particles.random() - 0.5, particles.random() - 0.5) * 2.0 * emitter.scatter;
            let like = emitter.particle;
            let born = particles.puff(position, turn_by(turn, like.velocity) + aside);
            particles.emit(Particle { position, velocity: born.velocity, rotation: born.rotation, layer: born.layer, spin: born.spin, age: 0.0, ..like });
        }
    }
    particles.step(dt);
}

fn turn_by(turn: Quat, velocity: Vec3) -> Vec3 {
    turn * velocity
}

/// A particle as the card draws it. Must match `Instance` in `particles.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    position: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
}

impl Instance {
    fn of(p: &Particle) -> Self {
        let t = p.age / p.life;
        // Quick to billow, slow to spread; thins away over its life.
        let grow = 1.0 - (-p.age * 1.2).exp();
        let radius = p.size * (1.0 + (p.spread - 1.0) * (grow * 0.7 + t * 0.3));
        let fade_in = (p.age / 0.15).min(1.0);
        let opacity = p.opacity * fade_in * (1.0 - t).powf(1.5) / (1.0 + t * 2.0);
        Self {
            position: [p.position.x, p.position.y, p.position.z, radius],
            color: [p.color.x, p.color.y, p.color.z, opacity],
            params: [p.rotation, p.layer as f32, if p.flash { 1.0 } else { 0.0 }, 0.6],
        }
    }
}

pub(crate) struct ParticleRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
    count: u32,
}

/// Soft, lumpy puffs, each a little different.
fn puff_images() -> Vec<Image> {
    (0..PUFF_LAYERS)
        .map(|layer| {
            let n = PUFF_SIZE;
            let mut data = Vec::with_capacity((n * n * 4) as usize);
            for y in 0..n {
                for x in 0..n {
                    let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                    let q = Vec3::new(p.x * 2.2, p.y * 2.2, layer as f32 * 7.3);
                    let billow = fbm(71 + layer, q, 5);
                    let edge = 1.0 - (p.length() / (0.62 + 0.45 * billow)).powf(2.5);
                    let alpha = edge.clamp(0.0, 1.0) * (0.55 + 0.6 * billow).min(1.0);
                    let shade = (0.8 + 0.35 * fbm(83 + layer, q * 1.7, 3)).min(1.0);
                    let c = (shade * 255.0) as u8;
                    data.extend([c, c, c, (alpha * 255.0) as u8]);
                }
            }
            Image::from_rgba(n, n, data, false)
        })
        .collect()
}

impl ParticleRenderer {
    fn new(gpu: &Gpu, view: &ViewBinding) -> Self {
        let device = &gpu.device;
        let puffs = TextureArray::from_images(gpu, &puff_images(), false);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particles layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particles"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&puffs.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&puffs.sampler) },
            ],
        });
        let shader = gpu.shader(crate::shader!("particles.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particles pipeline layout"),
            bind_group_layouts: &[Some(&view.layout), Some(&layout)],
            immediate_size: 0,
        });
        // What a particle covers is taken from what is behind it, and what it gives is added:
        // so smoke and flashes go through the one pipeline.
        let premultiplied = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState { color: premultiplied, alpha: premultiplied }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: Some(main_depth_state(false)),
            multisample: main_multisample(false),
            multiview_mask: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particles"),
            size: (MAX_PARTICLES * std::mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, bind_group, buffer, count: 0 }
    }
}

/// The particles as the view sees them, furthest first, so that nearer smoke covers farther.
fn in_order(particles: &[Particle], camera: Vec3) -> Vec<Instance> {
    let mut far_first: Vec<(f32, Instance)> = particles.iter().map(|p| (p.position.distance_squared(camera), Instance::of(p))).collect();
    far_first.sort_by(|a, b| b.0.total_cmp(&a.0));
    far_first.into_iter().map(|(_, instance)| instance).collect()
}

/// Sends the card the particles for the view being drawn. The renderer is made the first
/// time there is anything to draw.
pub(crate) fn prepare(world: &mut World) {
    if !world.contains_resource::<Gpu>() || !world.contains_resource::<ViewBinding>() {
        return;
    }
    let any = !world.resource::<Particles>().is_empty();
    if !world.contains_resource::<ParticleRenderer>() {
        if !any {
            return;
        }
        let renderer = ParticleRenderer::new(world.resource::<Gpu>(), world.resource::<ViewBinding>());
        world.insert_resource(renderer);
    }
    let data = in_order(&world.resource::<Particles>().particles, world.resource::<RenderFrame>().camera_position);
    if !data.is_empty() {
        world.resource::<Gpu>().queue.write_buffer(&world.resource::<ParticleRenderer>().buffer, 0, bytemuck::cast_slice(&data));
    }
    world.resource_mut::<ParticleRenderer>().count = data.len() as u32;
}

pub(crate) fn draw(world: &World, pass: &mut wgpu::RenderPass<'_>) {
    let Some(renderer) = world.get_resource::<ParticleRenderer>().filter(|renderer| renderer.count > 0) else { return };
    pass.set_pipeline(&renderer.pipeline);
    pass.set_bind_group(1, &renderer.bind_group, &[]);
    pass.set_vertex_buffer(0, renderer.buffer.slice(..));
    pass.draw(0..4, 0..renderer.count);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_particle_slows_rises_is_carried_and_goes() {
        let mut particles = Particles { wind: Vec3::new(2.0, 0.0, 0.0), ..Default::default() };
        let puff = particles.puff(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0));
        particles.emit(Particle { life: 2.0, rise: 1.0, drag: 3.0, ..puff });
        for _ in 0..60 {
            particles.step(1.0 / 60.0);
        }
        let now = particles.particles[0];
        // A second on: it has lost most of its own speed, has risen, and the wind has begun
        // to carry it.
        assert!(now.velocity.length() < 1.0 && now.position.z < -2.0 && now.position.z > -4.0);
        assert!((now.position.y - 1.0).abs() < 0.05);
        assert!(now.position.x > 0.2 && now.position.x < 2.0);
        // Its time up, it is gone.
        for _ in 0..61 {
            particles.step(1.0 / 60.0);
        }
        assert!(particles.is_empty());
    }

    #[test]
    fn no_more_are_kept_than_there_may_be_and_puffs_differ() {
        let mut particles = Particles::default();
        let (a, b) = (particles.puff(Vec3::ZERO, Vec3::ZERO), particles.puff(Vec3::ZERO, Vec3::ZERO));
        assert!(a.rotation != b.rotation && a.layer < PUFF_LAYERS && b.layer < PUFF_LAYERS);
        for _ in 0..MAX_PARTICLES + 10 {
            particles.emit(a);
        }
        assert_eq!(particles.len(), MAX_PARTICLES);
    }

    #[test]
    fn particles_are_drawn_furthest_first_growing_and_thinning() {
        let near = Particle { position: Vec3::new(0.0, 0.0, -2.0), age: 5.0, ..Default::default() };
        let far = Particle { position: Vec3::new(0.0, 0.0, -20.0), age: 0.5, ..Default::default() };
        let drawn = in_order(&[near, far], Vec3::ZERO);
        assert_eq!(drawn[0].position[2], -20.0);
        // The older one has spread further and thinned more.
        assert!(drawn[1].position[3] > drawn[0].position[3] && drawn[1].color[3] < drawn[0].color[3]);
    }

    #[test]
    fn an_emitter_lets_out_so_many_a_second_from_where_it_is() {
        use crate::app::{App, Stage};
        let mut app = App::new();
        app.add_plugins(crate::time::TimePlugin).add_plugins(crate::transform::TransformPlugin);
        app.init_resource::<Particles>().add_systems(Stage::Last, simulate);
        app.world.resource_mut::<Time>().set_fixed_step(Some(std::time::Duration::from_millis(20)));
        let like = Particle { velocity: Vec3::new(0.0, 0.0, -3.0), life: 100.0, ..Default::default() };
        // Turned a quarter round, so that what it throws forward goes along -x.
        let turned = crate::transform::Transform::from_xyz(4.0, 1.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
        app.world.spawn((turned, ParticleEmitter::new(10.0, like)));
        for _ in 0..101 {
            app.update();
        }
        // Two seconds at ten a second, less the frame before the entity had a place.
        let particles = app.world.resource::<Particles>();
        assert!((19..=20).contains(&particles.len()), "{} let out", particles.len());
        assert!(particles.particles.iter().all(|p| p.position.x < 4.0 && (p.position.y - 1.0).abs() < 25.0 && p.velocity.x <= 0.0));
    }
}
