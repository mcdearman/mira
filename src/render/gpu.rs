use std::sync::Arc;

use anyhow::Context;

use super::{compose::{self, ShaderLibrary}, Shader};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The scene is lit and blended in linear HDR, then tone mapped onto the swapchain.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// How many samples a pixel of the scene has where it is multisampled: see [`samples`].
pub const MSAA_SAMPLES: u32 = 4;

/// Whether the scene is drawn with several samples a pixel (multisampling), which smooths
/// the edges of things as they are drawn. Read once, when the renderer starts: insert it
/// before `DefaultPlugins`. `MIRA_MSAA=0` or `=1` turns it off or on whatever this says.
///
/// The blend over time (`PostProcess::taa`) smooths edges too, from frame to frame, so with
/// that on a scene may do without this, and the card then shades and stores a quarter as
/// many samples: where it is shading that costs, as with rays traced for every fragment,
/// that is most of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Multisampling(pub bool);

impl Default for Multisampling {
    fn default() -> Self {
        Self(true)
    }
}

static SAMPLES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(MSAA_SAMPLES);

/// How many samples a pixel of the scene has: [`MSAA_SAMPLES`], or one where the scene is
/// not multisampled. Pipelines that draw into the main pass, and targets they draw into, are
/// made for this many; [`main_multisample`] says so for a pipeline.
pub fn samples() -> u32 {
    SAMPLES.load(std::sync::atomic::Ordering::Relaxed)
}

/// Says whether the scene is multisampled, before anything is made for drawing it.
pub(crate) fn set_multisampling(on: bool) {
    SAMPLES.store(if on { MSAA_SAMPLES } else { 1 }, std::sync::atomic::Ordering::Relaxed);
}
/// How far things have moved across the picture in a frame (see `motion.rs`).
pub const MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// What the motion target holds where nothing that moved was drawn: further than anything
/// can move in a frame.
pub const MOTION_CLEAR: wgpu::Color = wgpu::Color { r: 4.0, g: 4.0, b: 0.0, a: 0.0 };
/// Depth is reversed (near = 1, far = 0) for precision far from the camera, so nearer
/// fragments have *greater* depth. Pipelines drawing into the main pass should use this.
pub const DEPTH_COMPARE: wgpu::CompareFunction = wgpu::CompareFunction::GreaterEqual;
pub const DEPTH_CLEAR: f32 = 0.0;

/// Depth-stencil state for an ordinary opaque pipeline in the main pass.
pub fn main_depth_state(write: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(write),
        depth_compare: Some(DEPTH_COMPARE),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

/// Multisample state for pipelines drawing into the main pass. `alpha_to_coverage` gives
/// alpha-tested foliage soft, anti-aliased edges, where there are samples to cover.
pub fn main_multisample(alpha_to_coverage: bool) -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: samples(),
        mask: !0,
        alpha_to_coverage_enabled: alpha_to_coverage && samples() > 1,
    }
}

/// The wgpu device and surface. The targets a frame draws into are made by the render graph
/// (see `graph`), at the size given here.
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// The window's surface. None when a host owns the device and shows the frames itself:
    /// see [`Gpu::hosted`].
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    /// Hardware ray queries are available and enabled.
    pub ray_tracing: bool,
    /// The shader modules a shader may import by name: the engine's, and any a plugin adds
    /// before it builds its pipelines. See `compose`.
    pub shaders: ShaderLibrary,
}

impl Gpu {
    /// Opens the GPU. With `ray_tracing`, prefers a Vulkan adapter with hardware ray queries,
    /// and quietly falls back to ordinary rendering where there isn't one.
    pub async fn new(window: Arc<winit::window::Window>, vsync: bool, ray_tracing: bool) -> anyhow::Result<Self> {
        let size = window.inner_size();
        // Ray queries, and an array of every texture so reflections can show what they hit.
        let rt_features = wgpu::Features::EXPERIMENTAL_RAY_QUERY
            | wgpu::Features::TEXTURE_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING;
        let open = |backends: wgpu::Backends| {
            // The window stands in for the display it is on, which some platforms need
            // before they can make a surface.
            let mut descriptor =
                wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()));
            descriptor.backends = backends;
            let instance = wgpu::Instance::new(descriptor);
            let surface = instance.create_surface(window.clone());
            (instance, surface)
        };
        let pick = |instance: &wgpu::Instance, surface: &wgpu::Surface<'static>| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            }))
            .ok()
        };
        let mut chosen = None;
        if ray_tracing {
            // Ray queries exist only on Vulkan for now.
            let (instance, surface) = open(wgpu::Backends::VULKAN);
            if let Ok(surface) = surface {
                if let Some(adapter) = pick(&instance, &surface) {
                    if adapter.features().contains(rt_features) {
                        chosen = Some((surface, adapter, true));
                    } else {
                        log::info!("{:?} can't trace rays; using shadow maps only", adapter.get_info().name);
                    }
                }
            }
        }
        let (surface, adapter, traced) = match chosen {
            Some(c) => c,
            None => {
                let (instance, surface) = open(wgpu::Backends::all());
                let surface = surface?;
                let adapter = pick(&instance, &surface).context("no compatible GPU adapter")?;
                (surface, adapter, false)
            }
        };
        let info = adapter.get_info();
        log::info!(
            "using adapter {:?} ({:?}){}",
            info.name,
            info.backend,
            if traced { " with ray tracing" } else { "" }
        );

        let available = adapter.limits();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mira device"),
                // Lines for polygons, where there are any, are for the wireframe view; block
                // compression, for pictures processed ahead of time; the card's clock, to
                // say how long it takes over each pass; indirect draws that begin at an
                // instance of our choosing, for the card to decide what is seen.
                required_features: (if traced { rt_features } else { wgpu::Features::empty() })
                    | (adapter.features()
                        & (wgpu::Features::POLYGON_MODE_LINE
                            | wgpu::Features::TEXTURE_COMPRESSION_BC
                            | wgpu::Features::INDIRECT_FIRST_INSTANCE
                            | wgpu::Features::TIMESTAMP_QUERY
                            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)),
                required_limits: wgpu::Limits {
                    // Big merged scenery meshes.
                    max_buffer_size: available.max_buffer_size,
                    max_blas_primitive_count: if traced { available.max_blas_primitive_count } else { 0 },
                    max_blas_geometry_count: if traced { available.max_blas_geometry_count } else { 0 },
                    max_tlas_instance_count: if traced { available.max_tlas_instance_count } else { 0 },
                    max_acceleration_structures_per_shader_stage: if traced {
                        available.max_acceleration_structures_per_shader_stage
                    } else {
                        0
                    },
                    max_binding_array_elements_per_shader_stage: if traced {
                        available.max_binding_array_elements_per_shader_stage
                    } else {
                        0
                    },
                    ..Default::default()
                },
                // Ray queries are among the features wgpu calls experimental, and gives
                // only to a device that was opened saying it accepts them as they are.
                experimental_features: if traced {
                    // SAFETY: nothing is asked of them but what wgpu documents: the
                    // structures are built before a frame's queries are made of them.
                    unsafe { wgpu::ExperimentalFeatures::enabled() }
                } else {
                    wgpu::ExperimentalFeatures::disabled()
                },
                ..Default::default()
            })
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            // COPY_SRC lets screenshots read the frame back, where the platform allows it.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | (caps.usages & wgpu::TextureUsages::COPY_SRC),
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            alpha_mode: caps.alpha_modes[0],
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            device,
            queue,
            surface: Some(surface),
            config,
            ray_tracing: traced,
            shaders: ShaderLibrary::engine(),
        })
    }

    /// A renderer on a device that someone else opened, with no window of its own: frames
    /// are drawn into a texture (see [`frame_texture`](super::frame_texture)) for the host to
    /// show. For an editor, or any program that puts the game inside its own interface.
    /// `format` is what the frames are drawn as; ray tracing is off, since it depends on
    /// what the device was opened with.
    pub fn hosted(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        Self {
            device,
            queue,
            surface: None,
            config,
            ray_tracing: false,
            shaders: ShaderLibrary::engine(),
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.reconfigure();
    }

    pub fn reconfigure(&mut self) {
        if let Some(surface) = &self.surface {
            surface.configure(&self.device, &self.config);
        }
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }
}

impl Gpu {
    /// What is defined for shaders built on this device: `RAY_TRACING` where it traces rays.
    pub fn shader_defines(&self) -> Vec<&'static str> {
        let mut defines = if self.ray_tracing { vec![compose::RAY_TRACING] } else { vec![] };
        if samples() > 1 {
            defines.push(compose::MSAA);
        }
        defines
    }

    /// Puts a shader together for this device: the modules it imports taken in from
    /// [`shaders`](Self::shaders), and the parts kept that this device's definitions ask for.
    /// A mistake in the directives is logged and the text handed back as it came, so that
    /// compiling it fails and says so: at startup loudly, on a reload leaving the old shader.
    pub fn compose(&self, source: &str) -> String {
        self.shaders.compose(source, &self.shader_defines()).unwrap_or_else(|mistake| {
            log::error!("shader not put together: {mistake}");
            source.to_owned()
        })
    }

    /// Puts a shader together as [`compose`](Self::compose) does, with more defined than
    /// the device itself defines, and says what was wrong where something was.
    pub fn compose_with(&self, source: &str, also: &[&str]) -> Result<String, compose::ComposeError> {
        let mut defines = self.shader_defines();
        defines.extend_from_slice(also);
        self.shaders.compose(source, &defines)
    }

    /// Compiles a shader file for this device, its imports taken in.
    pub fn shader(&self, shader: Shader) -> wgpu::ShaderModule {
        self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(shader.name),
            source: wgpu::ShaderSource::Wgsl(self.compose(&shader.source()).into()),
        })
    }

    /// The engine's lighting as text, ray traced where the device allows, for a shader built
    /// by joining strings. A shader file says `#import mira::pbr` instead and is built with
    /// [`shader`](Self::shader).
    pub fn pbr_wgsl(&self) -> String {
        self.compose(&format!("#import {}", compose::PBR))
    }
}
