# Shaders made of modules

A WGSL file takes in another by name, and keeps parts of itself for some builds only:

```wgsl
// water.wgsl
#import mira::pbr

#ifdef RAY_TRACING
// reflections traced against the scene
#else
// reflections from the sky alone
#endif

@fragment
fn fs(in: VertexOutput) -> @location(0) vec4<f32> {
    // ... lit with the engine's lighting
}
```

```rust
let module = gpu.shader(mira::shader!("water.wgsl"));
```

`Gpu::shader` puts the file together and compiles it: its imports taken in, the parts kept
that the device's definitions ask for. `Gpu::compose` does the same to text and gives the
text back.

## Directives

Each is a line of its own.

| Line | What it does |
|---|---|
| `#import name` | takes in the module of that name, once, however many ask for it |
| `#ifdef NAME` | keeps what follows, up to `#else` or `#endif`, where `NAME` is defined |
| `#ifndef NAME` | the same, where it is not |
| `#else`, `#endif` | the other part, and the end; `#ifdef` may be nested |

A module's own imports come before it, and the shader's own text last. `enable` lines,
which WGSL wants before everything else, are moved to the top from wherever a module has
them: the engine's ray-traced lighting asks for its extensions that way.

A mistake (a module that is not there, an `#endif` too many) is logged with the module and
the line. The shader then fails to compile: at startup loudly, and on a reload leaving the
old shader in place.

## Modules

`Gpu::shaders` is the library. The engine's modules are there from the start:

| Name | What it holds |
|---|---|
| `mira::pbr` | the engine's lighting: the view, the sun and its shadows, sky light and haze, at group 0; ray traced where the device traces rays |

A plugin adds its own before it builds its pipelines, from a file (reloaded with the rest)
or from text:

```rust
let gpu = world.resource_mut::<Gpu>();
gpu.shaders.add("game::wind", mira::shader!("wind.wgsl"));
gpu.shaders.add_text("game::limits", "const MAX_WAVES: u32 = 8;");
```

Adding one with the name of a module that is there replaces it.

## Definitions

What is defined comes from the device (`Gpu::shader_defines`): `RAY_TRACING` where it traces
rays. One source gives a variant for each set of definitions, so a shader has no need of
two files for the two cases.

`ShaderLibrary::compose(source, &["A", "B"])` puts a shader together with definitions of
your own choosing.

## Surfaces: a material's own shader

A game changes how a surface looks without a pipeline of its own. A module says what the
surface is, and an entity names it:

```wgsl
// lava.wgsl
fn surface(s: Surface, in: VertexOutput) -> Surface {
    var out = s;
    let glow = 0.5 + 0.5 * sin(time() * 3.0 + in.world_position.x);
    out.emissive = vec3<f32>(2.0, 0.4, 0.1) * glow;
    return out;
}
```

```rust
world.resource_mut::<Gpu>().shaders.add("game::lava", mira::shader!("lava.wgsl"));

commands.spawn((at, Mesh3d(mesh), Material::default(), Surface::new("game::lava")));
```

The mesh is drawn as any other: the same vertices, textures, lighting, shadows and haze.
`surface` is called last, with the surface the engine worked out (`s`: colour, normal,
roughness, metalness, what it gives off, and the rest of `Surface` in `mira::pbr`) and what
the vertex shader passed down (`in`: world position, normal, texture coordinate, and the
material's values). What it hands back is what is lit. The material's textures and the
whole of `mira::pbr` are there to use.

The entity's `Material` is the instance: its colour, roughness, textures and the rest reach
the shader as values, so every entity naming one surface is drawn by the same pipelines,
and differs only in those. `Surface` is a component like any other: it is saved in a scene,
and can be set by name in the inspector or by an agent.

A shader that is not in the library, or will not compile, is said in the log once, and its
entities are drawn the standard way. Saved again while the app runs, it is built again.

What a surface shader does not reach yet: the shadow the mesh casts, what a traced
reflection of it shows, and its vertices.

The starter level's ground is one: `crates/mira_app/src/grid.wgsl`.

## Joining strings
Shaders built by joining text still work: `gpu.pbr_wgsl()` gives the lighting as text, with
its `enable` lines already at the top, to put in front of a shader's own. A file with
`#import mira::pbr` built by `gpu.shader` is the same thing said in the shader itself.

See [HOT_RELOAD.md](HOT_RELOAD.md) for how shader files are reloaded while the app runs:
a module that is imported is watched like any other.
