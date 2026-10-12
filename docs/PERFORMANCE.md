# What a frame costs

## Measuring

```sh
MIRA_HIDDEN=1 cargo run --release --example instances -- frames=300
MIRA_HIDDEN=1 cargo run --release --example instances -- count=20000 moving=0.5 frames=300
```

`examples/instances` lays out a field of cubes and balls (`count`, 100,000 unless told), a
part of them moving (`moving`, a tenth), runs the frames asked for, and prints what a frame
cost: the mean and the worst, each stage, and each pass that draws it. Without `frames=` it
runs on, to be looked at.

Any running game says the same over its debug connection: `mira-debug profile` for frames,
stages and the costliest systems, `mira-debug passes` for the passes (see
[LIVE.md](LIVE.md) and [RENDER_GRAPH.md](RENDER_GRAPH.md)).

The times are the engine's own on the processor: gathering the frame, and recording and
handing over its work. They are not the graphics card's time drawing it, nor the wait for
the display. `Render` in the example's report is what is left of the mean frame once the
other stages are taken away, since the report is made inside that stage.

## Where it stands

The roadmap's test for the renderer is 100,000 mesh instances at sixty frames a second,
which is 16.7 ms a frame.

Measured on 2026-10-12 on Windows 11 with an Intel Core i7-14700KF and an RTX 5070, Vulkan,
ray tracing on, the window hidden, a tenth of the things moving. The roadmap's test names
the M2 Pro, which has not been measured:

| Things | A frame (mean) | Extract | Prepare | Render | Frames a second |
|---|---|---|---|---|---|
| 1,000 | 3.5 ms | 0.1 ms | 0.1 ms | 3.1 ms | 286 |
| 20,000 | 12.4 ms | 2.7 ms | 2.0 ms | 7.0 ms | 81 |
| 100,000 | 46.2 ms | 16.6 ms | 11.2 ms | 15.3 ms | 22 |

What the numbers say:

- **Extract and Prepare grow with every thing there is, moving or not.** Each frame, every
  entity is read, tested against the view, put in a list, sorted, and written out again as
  an instance; the whole instance buffer (about 20 MB at 100,000) is sent to the card anew.
  Nine tenths of the things had not moved.
- **Recording the draws is nothing**: the main pass takes under 0.01 ms, because instances of
  one mesh and material are already one draw.
- **Render is mostly not the passes.** They come to about 4.5 ms at 100,000, of which ray
  tracing's scene is 3.4; the rest is handing the frame's work and its buffers to the card.
- On this machine ray tracing is on, which a machine without it does not pay for.

So the next work is to keep what was gathered: an instance stays on the card from frame to
frame and is written only when its entity changes, and the card, not the processor, decides
what is in view.
