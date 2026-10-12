// Standard meshes drawn into a shadow cascade.

@group(0) @binding(0) var<uniform> light_view_proj: mat4x4<f32>;

@group(1) @binding(0) var base_color_texture: texture_2d<f32>;
@group(1) @binding(3) var material_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

// As in mesh.wgsl: the instances kept on the card, and the numbers of those to draw.
struct Instance {
    model: mat4x4<f32>,
    normal: mat3x3<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    emissive: vec4<f32>,
    extra: vec4<f32>,
    more: vec4<f32>,
};

@group(2) @binding(0) var<storage, read> instances: array<Instance>;
@group(2) @binding(1) var<storage, read> seen: array<u32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) cutoff: f32,
    @location(2) @interpolate(flat) alpha: f32,
};

@vertex
fn vs_main(vertex: VertexInput, @builtin(instance_index) at: u32) -> VertexOutput {
    let instance = instances[seen[at]];
    var out: VertexOutput;
    out.clip_position = light_view_proj * instance.model * vec4<f32>(vertex.position, 1.0);
    out.uv = vertex.uv;
    out.cutoff = instance.params.w;
    out.alpha = instance.color.a;
    return out;
}

// Only alpha-tested materials need a fragment stage: holes in leaves let light through.
@fragment
fn fs_mask(in: VertexOutput) {
    let alpha = textureSample(base_color_texture, material_sampler, in.uv).a * in.alpha;
    if alpha < in.cutoff {
        discard;
    }
}
