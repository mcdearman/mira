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

Measured on Windows 11 with an Intel Core i7-14700KF and an RTX 5070, Vulkan, the window
hidden, a tenth of the things moving unless said. The roadmap's test names the M2 Pro, which
has not been measured.

With ray tracing, as that machine runs by default:

| Things | | A frame (mean) | Extract | Prepare | Render | Frames a second |
|---|---|---|---|---|---|---|
| 1,000 | before | 3.5 ms | 0.1 ms | 0.1 ms | 3.1 ms | 286 |
| | now | 3.2 ms | 0.05 ms | 0.1 ms | 2.8 ms | 311 |
| 20,000 | before | 12.4 ms | 2.7 ms | 2.0 ms | 7.0 ms | 81 |
| | now | 8.3 ms | 0.9 ms | 1.2 ms | 5.5 ms | 120 |
| 100,000 | before | 46.2 ms | 16.6 ms | 11.2 ms | 15.3 ms | 22 |
| | now | 26.8 ms | 3.2 ms | 6.4 ms | 14.1 ms | 37 |

Without it (`rays=off`), as a machine that cannot trace rays runs, at 100,000:

| Moving | | A frame (mean) | Extract | Prepare | Render | Frames a second |
|---|---|---|---|---|---|---|
| a tenth | instances kept | 21.0 ms | 3.4 ms | 3.9 ms | 10.5 ms | 48 |
| | and shadows by cascade | 13.4 ms | 3.2 ms | 4.2 ms | 3.0 ms | 74 |
| none | instances kept | 21.1 ms | 0.4 ms | 1.0 ms | 16.9 ms | 47 |
| | and shadows by cascade | 5.4 ms | 0.4 ms | 2.0 ms | 0.1 ms | 185 |

Without ray tracing the roadmap's 16.7 ms is met on this machine, with a tenth of the
things moving. With it, 100,000 is still 26.7 ms.

"Before" is 2026-10-12, when every instance was gathered and sent again each frame. "Now" is
with instances kept on the card (below).

## Instances kept on the card

An instance, one for each mesh an entity draws, is made once and stays on the card from
frame to frame (`src/render/instances.rs`). Each frame only the entities whose transform,
mesh, material, levels of detail or surface were written are looked at, and only their
instances are sent again; an entity that is despawned or loses its mesh gives its instance
back. Each view then decides what it sees and sends the card that list of numbers, four
bytes for each, and the mesh shaders read an instance through its number.

A level of detail is an instance of its own, seen over its own range of distances, so
choosing the level is part of deciding what is seen.

What the second table says:

- **Gathering no longer grows with what stands still.** With nothing moving, Extract and
  Prepare together are 1.4 ms at 100,000, where they were 28.
- **With instances kept and nothing else, the frame was the same 21 ms whether a tenth moved
  or nothing did**, and what Extract and Prepare gave up, Render took. That is the mark of
  the card being what is waited for. Turning the sun's shadows off (`shadows=off`) took the
  frame from 21.2 ms to 4.6: every caster was drawn into every one of the four shadow
  cascades, so the card drew the field five times.
- **With ray tracing the traced scene is the next cost**: every instance is put into it
  again each frame, moved or not.

## Shadows by cascade

A cascade's map takes in a box of the world, long enough toward the sun to hold what casts
into it. A caster is now drawn into the cascades whose box it falls in, where it was drawn
into all four: the near cascades are small, so most of a wide scene is in one or two.
Deciding it is part of deciding what the view sees, from the same small record.

So the next work is for the traced scene to keep what it was given as the instances now
do, which is what a frame with ray tracing still pays for; then to decide on the card what
each view and each cascade sees, with meshes in shared buffers so that a pipeline's draws
are one call.