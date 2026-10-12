# The frame as a graph of passes

A frame is drawn by passes: the shadow maps, the scene, the light shafts, the blend over
time, the last touches to the picture. Each says what it reads and what it writes, by name,
and the `RenderGraph` resource works out the order from that. Nothing in the renderer lists
the passes in order; a plugin adds one of its own without knowing where in the frame it
falls.

```rust
use mira::render::{slots, Pass, PassContext, RenderGraph};

fn outline(world: &mut World, cx: &mut PassContext) {
    let (depth, scene) = (cx.view(slots::DEPTH).clone(), cx.view(slots::SCENE).clone());
    let mut pass = cx.encoder.begin_render_pass(/* draws into `scene`, reading `depth` */);
    // ...
}

app.world.resource_mut::<RenderGraph>().add(
    Pass::new("outline", outline)
        .reads(&[slots::DEPTH, slots::SCENE])
        .writes(&[slots::SCENE]),
);
```

## The order

- A pass runs after the passes that write what it reads.
- Passes that write the same thing run in the order they were added, each going on from
  what the one before left. A pass that reads and writes a thing is one of these.
- A pass that only reads a thing runs after everything that writes it.
- Where that says nothing, passes run in the order they were added, unless one says
  `.after("name")` or `.before("name")`.

Passes that wait on each other in a circle are refused: the error names them, and nothing is
drawn until one is taken out. `RenderGraph::describe` gives the passes in the order they
run, with what each reads and writes and two times: how long it last took to record its
work, on the processor, and how long the graphics card last took to do that work, where the
card can say (see [PERFORMANCE.md](PERFORMANCE.md)). A running game tells the same over the
debug connection (`mira-debug passes`) and to an agent (`mira_passes`).

Adding a pass with the name of one that is there replaces it, so a plugin can put its own
`"post"` in place of the engine's. `remove` takes one out.

## The engine's passes

| Pass | Reads | Writes |
|---|---|---|
| `skinning` | | `skinned` |
| `ray tracing` | `skinned` | `traced scene` |
| `shadows` | `skinned` | `shadow maps` |
| `main` | `skinned`, `shadow maps`, `traced scene` | `hdr msaa`, `hdr`, `depth` |
| `light shafts` | `hdr`, `depth`, `shadow maps` | `hdr` |
| `motion` | `depth` | `motion` |
| `blend over time` | `hdr`, `depth`, `motion` | `scene` |
| `post` | `scene` | `frame` |
| `debug lines` | `depth`, `frame` | `frame` |
| `overlays` | `frame` | `frame` |

The names are in `mira::render::slots`. The main pass still draws what plugins push onto
`DrawFunctions`, `TransparentDrawFunctions` and `ShadowDrawFunctions`: those add draws to a
pass, where the graph adds passes to the frame.

## Targets

A pass is handed the frame's targets by name: `cx.view(slots::DEPTH)`. `frame` is the
picture that is shown. Every other target the graph makes itself, at the size of the frame
being drawn: the engine's `hdr`, `hdr msaa`, `depth` and `motion`, and any a plugin asks for:

```rust
graph.target("outline mask", TargetDesc::new(wgpu::TextureFormat::R8Unorm).scale(0.5));
```

Where more than one view of the scene is drawn in a frame (see `ViewTarget`), a target is
kept for each size, up to eight. A target holds nothing from one frame to the next: what
must last, as the history the blend over time keeps, belongs to the pass that keeps it.

A pass whose result is a view of its own, as the blend over time's is, says where the slot's
picture now is with `cx.set_view`. `cx.note` leaves a word for the passes that follow, in
that frame only.

## Over the finished picture

What is drawn over the picture is passes too, after `post`: the debug lines, then the
`overlays` pass, which runs the functions pushed onto `Overlays` (an interface made with
`mira_ui` is one). Those submit work of their own, so the pass first sends on what the frame
has recorded so far: `cx.flush()`, for any pass that does the same. A pass that records
into `cx.encoder` needs none of that.

Only the screenshot is outside the graph: it is taken of the frame once every pass has run.
