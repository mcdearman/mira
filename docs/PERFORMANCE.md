# What a frame costs

## Measuring

```sh
MIRA_HIDDEN=1 cargo run --release --example instances -- frames=300
MIRA_HIDDEN=1 cargo run --release --example instances -- count=20000 moving=0.5 frames=300
```

`examples/instances` lays out a field of cubes and balls (`count`, 100,000 unless told), a
part of them moving (`moving`, a tenth), runs the frames asked for, and prints what a frame
cost. Without `frames=` it runs on, to be looked at. `rays=off`, `shadows=off`,
`lights=N`, `size=WxH`, `lods=on`, `scale=`, `spots=N` and `lanterns=N` take a part of
the frame away or add to it, to see what it costs.

A frame is paid for twice, and the two are told apart:

- **On the processor**: the engine's own work, gathering the frame, recording it and handing
  it over, with what the graphics library does when it is handed over. `FrameStats` times
  it, by frame, by stage and by system.
- **On the card**: the graphics card drawing what it was handed. The card stamps the time at
  the end of each pass and the engine reads the stamps back a few frames later
  (`src/render/card_clock.rs`), so each pass has two times: recording its work, and the
  card doing it. A card that cannot stamp the time between passes says nothing.

The slower of the two is what the frame rate can reach. Any running game says both over its
debug connection: `mira-debug profile` for frames (`mean_millis` on the processor,
`card_millis` on the card), stages and the costliest systems, `mira-debug passes` for the
passes (see [LIVE.md](LIVE.md) and [RENDER_GRAPH.md](RENDER_GRAPH.md)).

Two things to know when reading the processor's figure:

- A hidden window runs its frames at about sixty a second, and a shown one at the display's
  pace. While the card keeps up, its time does not show on the processor at all. Once it
  takes longer than the frames are apart, the engine waits for it, and the wait is counted
  in the frame. So a processor figure near 17 ms may be the card's, not the engine's.
- `Render` in the example's report is what is left of the mean frame once the other stages
  are taken away, since the report is made inside that stage.

## Where it stands

The roadmap's test for the renderer is 100,000 mesh instances and 1,000 lights at sixty
frames a second, which is 16.7 ms a frame.

Measured on Windows 11 with an Intel Core i7-14700KF and an RTX 5070, at 1280 by 720, the
window hidden, a tenth of the things moving unless said. The roadmap's test names the M2
Pro, which has not been measured.

Without ray tracing (`rays=off`), as a machine that cannot trace rays runs:

| Things | Moving | Lights | On the processor | On the card | Shadows | Main pass | Frames a second |
|---|---|---|---|---|---|---|---|
| 1,000 | a tenth | | 0.9 ms | 1.4 ms | 0.2 ms | 0.8 ms | 707 |
| 20,000 | a tenth | | 3.1 ms | 6.0 ms | 2.1 ms | 3.4 ms | 166 |
| 100,000 | none | | 5.3 ms | 13.4 ms | 5.5 ms | 7.5 ms | 75 |
| 100,000 | a tenth | | 13.1 ms | 13.6 ms | 5.6 ms | 7.4 ms | 74 |
| 100,000 | a tenth | 1,000 | 13.2 ms | 15.3 ms | 5.7 ms | 9.1 ms | 65 |

With ray tracing, as that machine runs by default:

| Things | Moving | On the processor | On the card | Shadows | Main pass | Frames a second |
|---|---|---|---|---|---|---|
| 1,000 | a tenth | 3.1 ms | 2.3 ms | 0.2 ms | 1.5 ms | 323 |
| 20,000 | a tenth | 8.1 ms | 9.0 ms | 2.1 ms | 6.2 ms | 111 |
| 100,000 | none | 6.5 ms | 16.1 ms | 5.6 ms | 10.1 ms | 62 |
| 100,000 | a hundredth | 18.6 ms | 16.4 ms | 5.6 ms | 10.0 ms | 54 |
| 100,000 | a tenth | 24.5 ms | 16.7 ms | 5.5 ms | 10.4 ms | 41 |

What that says:

- **The roadmap's test is met on this machine without ray tracing**, at 65 frames a second,
  and by little: the card takes 15.3 ms of the 16.7.
- **The card is what holds the frame back**, not the engine, wherever nothing much moves. At
  100,000 it spends 5.5 ms drawing the sun's shadow maps and 7.5 to 10 ms on the main pass,
  nearly all of it on triangles the field need not have (see levels of detail, below).
- **With ray tracing and things moving, the processor is**, and the time is in the traced
  scene (below).

Until 2026-10-12 the figures in this document were the processor's alone, and some of what
was said of the card from them was wrong. They are replaced above; what was wrong is said
where it belongs below.

## Instances kept on the card

An instance, one for each mesh an entity draws, is made once and stays on the card from
frame to frame (`src/render/instances.rs`). Each frame only the entities whose transform,
mesh, material, levels of detail or surface were written are looked at, and only their
instances are sent again; an entity that is despawned or loses its mesh gives its instance
back. Each view then decides what it sees and sends the card that list of numbers, four
bytes for each, and the mesh shaders read an instance through its number.

A level of detail is an instance of its own, seen over its own range of distances, so
choosing the level is part of deciding what is seen.

Gathering no longer grows with what stands still. Before it, at 100,000 things, gathering
and preparing a frame took 28 ms on the processor whether anything moved or not; with
nothing moving they now take 1.4 ms.

## Shadows by cascade

A cascade's map takes in a box of the world, long enough toward the sun to hold what casts
into it. A caster is now drawn into the cascades whose box it falls in, where it was drawn
into all four: the near cascades are small, so most of a wide scene is in one or two.
Deciding it is part of deciding what the view sees, from the same small record.

Before it, with every caster drawn into all four, the card fell behind the frames and the
processor's figure at 100,000 was 21 ms, moving or not; `shadows=off` took it to 4.6 ms.
The shadows are still the card's second largest cost: 5.5 ms at 100,000, and with
`shadows=off` the card's whole frame goes from 13.6 ms to 7.4.

## A thousand lights

`lights=N` stands that many lamps over the field, each reaching the things about it (see
[LIGHTS.md](LIGHTS.md)). On the processor a thousand cost a tenth of a millisecond: sorting
them into the view's cells. On the card they cost 1.7 ms in the main pass at 100,000
things, where each fragment goes through the few lamps in its own cell.

(This document said before that a thousand lamps cost nothing that shows. That was the
processor's figure.)

## The traced scene

Where rays are traced, the scene they are traced against was gathered again every frame:
every instance looked up and listed, its material sent, and the structure the card traces
against built anew.

It now keeps what it has (`src/render/raytrace.rs`). An instance has a place in the
structure for as long as it is traced against, and a frame looks only at the instances that
changed and at those the camera's moving has brought within the rays' reach, taken out of
it, or changed the level of detail of. Their materials are sent, their places told to the
structure, and nothing else is touched. Where nothing changed, nothing is built.

What tracing costs, at 100,000 things:

- **On the card, about 2.6 ms at rest**: the main pass is 10.1 ms with rays against 7.5
  without.
- **When anything within the rays' reach moves, the structure is built again, whole**, from
  every instance in it (65,536 at most, which is what it holds at 100,000 things). The
  graphics library does that whenever any one of them has changed.
- **Building it costs the card 0.3 ms.** It costs the processor about 12 ms, however little
  moved, which is the difference between 6.5 ms at rest and 18.6 ms with a hundredth of the
  things moving: 2.7 ms recording the build, and most of the rest when the frame's work is
  handed over (8.5 ms, against 1.5 without rays), where the library goes through every
  instance of the structure.

(This document said before that the 12 ms was the card building the structure. The time was
found by taking the stages away from the frame, which could not tell the card's work from
the library's on the processor. The card's clock says it is the library's.)

### Two structures

What cuts it is fewer instances in what has to be built again. So there are two structures:
one of the things that stand still, of 65,536 at most and made to be quick to trace, and one
of the things that move, of 16,384 at most and made to be quick to build. A thing that
moves is taken from the first to the second, and goes back once it has stood still for 120
frames, many at a time so that the large one is seldom built. A scene arriving all at once
goes straight to the large one. Where the small one is full, a thing that moves stays in
the large one, which is then built as before.

A ray is traced against both, and against the second only while there is something in it,
so a scene at rest pays nothing for it.

With ray tracing, the figures in the tables above are from before this. Beside them:

| Things | Moving | | On the processor | On the card | Frames a second |
|---|---|---|---|---|---|
| 100,000 | none | one structure | 6.5 ms | 16.1 ms | 62 |
| | | two | 6.4 ms | 16.7 ms | 60 |
| 100,000 | a hundredth | one structure | 18.6 ms | 16.4 ms | 54 |
| | | two | 19.1 ms | 18.0 ms | 52 |
| 100,000 | a tenth | one structure | 24.5 ms | 16.7 ms | 41 |
| | | two | 18.7 ms | 18.3 ms | 53 |
| 100,000 with levels of detail | none | one structure | 7.2 ms | 5.8 ms | 139 |
| | | two | 7.1 ms | 6.1 ms | 141 |
| 100,000 with levels of detail | a hundredth | two | 10.2 ms | 7.2 ms | 98 |
| 100,000 with levels of detail | a tenth | one structure | 27.7 ms | 6.1 ms | 36 |
| | | two | 19.8 ms | 7.5 ms | 51 |
| 20,000 | a tenth | one structure | 8.1 ms | 9.0 ms | 111 |
| | | two | 4.2 ms | 10.9 ms | 92 |

What that says:

- **Building costs the processor half a millisecond where it cost twelve.** With a tenth of
  100,000 moving, the frame goes from 41 a second to 53, and with levels of detail from 36
  to 51.
- **Tracing the second structure costs the card 1 to 2 ms while anything moves.** Where the
  card is what holds the frame back and the processor is not, that is a loss: 20,000 things
  without levels of detail go from 111 a second to 92.
- Where the card is past 16.7 ms, the processor's figure is mostly the wait for it (the two
  rows at 100,000 without levels of detail).

(`moving=0.00001` at 100,000 moves one thing that is 490 m from the camera, beyond the
rays' reach of 400, so it measures the same as nothing moving.)

## Triangles, and levels of detail

What the card spends on that field is triangles, not pixels. With the picture a sixteenth
the size (`size=320x180`) the main pass is still 5.9 ms of its 7.4 and the shadows are
unchanged. Half the things are balls of 960 triangles, most of them a few pixels across.

`lods=on` gives each ball two coarser meshes for further away, through the `Lods` component
any entity can carry (72 triangles beyond 30 m, 24 beyond 90). At 100,000 things:

| Rays | Moving | Lights | On the processor | On the card | Shadows | Main pass | Frames a second |
|---|---|---|---|---|---|---|---|
| off | none | | 5.9 ms | 3.5 ms | 1.0 ms | 2.1 ms | 170 |
| off | a tenth | | 16.5 ms | 3.6 ms | 1.0 ms | 2.2 ms | 61 |
| off | a tenth | 1,000 | 16.2 ms | 4.6 ms | 1.0 ms | 3.2 ms | 62 |
| on | none | | 7.2 ms | 5.8 ms | 1.0 ms | 4.4 ms | 139 |
| on | a tenth | | 27.7 ms | 6.1 ms | 1.1 ms | 4.6 ms | 36 |

So a scene whose distant things are given fewer triangles costs the card a quarter of what
it did, and then it is the processor that holds the frame back once things move. A thing
with levels costs the processor more than one without, since each level is an instance of
its own: 16.5 ms against 13.1 with a tenth moving.

The engine does not make the coarser meshes: they are the scene's to give.

## Things that move, on the processor

With levels of detail the card keeps up, and what a frame costs is the processor's work
for the things that moved. Two parts of it went with how much there was, not how much
moved:

- **Working out where things are** (`src/transform.rs`) walked every entity every frame,
  2.7 ms at 100,000 with nothing moving. An entity with no parent and no children is now
  looked at only when its `Transform` has been written; trees are walked as before.
- **Sending a moved instance** sent all of it, 192 bytes, and its neighbours with it. Where
  an instance is and how it looks are now kept apart on the card
  (`src/render/instances.rs`): a thing that moves sends 48 bytes, and its normals' matrix
  is worked out in the vertex shader from the model's.

At 100,000 things with levels of detail, without ray tracing unless said:

| Moving | | On the processor | On the card | Frames a second |
|---|---|---|---|---|
| none | before | 5.9 ms | 3.5 ms | 170 |
| | now | 3.4 ms | 3.6 ms | 280 |
| a tenth | before | 16.5 ms | 3.6 ms | 61 |
| | now | 11.6 ms | 3.6 ms | 86 |
| a tenth, 1,000 lights | before | 16.2 ms | 4.6 ms | 62 |
| | now | 11.6 ms | 4.6 ms | 87 |
| a tenth, with rays | before | 19.8 ms | 7.5 ms | 51 |
| | now | 14.8 ms | 7.3 ms | 68 |

The tables further up were measured before this, and their processor's figures are that
much too high.

Gathering what changed went through each changed entity whole, its meshes, materials and
batches. One that has only moved, as most have, now has only where its instances are
brought up to date: 3.6 ms became 2.3 for 10,000 of them, and the frame 10.0 ms (100 a
second).

What is left, with a tenth moving: that gathering (Extract, 2.3 ms), and deciding what the
view and the cascades see and sending it (Prepare, 4 ms).
## What is seen, decided on the card

Deciding what the frame's view and each shadow cascade see went through every instance
every frame on the processor, and sent the card the numbers of those seen: 2 ms at 100,000
things with levels of detail, moving or not.

The card now decides it (`src/render/cull.rs`, `cull.wgsl`). It holds where each instance
is already; it is told once what else it needs, the size of the instance's mesh, the
distances it is drawn over, and its batch. A compute pass goes through the instances, lists
what each view sees batch by batch, and writes how many there are into the draw each batch
is then drawn with. The processor sends five numbers a batch for each view, and nothing for
an instance that has not changed.

At 100,000 things with levels of detail, without ray tracing unless said:

| Moving | | On the processor | On the card | Frames a second |
|---|---|---|---|---|
| none | on the processor | 3.6 ms | 3.3 ms | 274 |
| | on the card | 1.4 ms | 3.7 ms | 273 |
| a tenth | on the processor | 10.5 ms | 3.7 ms | 95 |
| | on the card | 9.7 ms | 4.1 ms | 103 |
| a tenth, 1,000 lights | on the processor | 11.8 ms | 4.7 ms | 85 |
| | on the card | 8.9 ms | 5.1 ms | 113 |
| a tenth, with rays | on the processor | 15.1 ms | 7.4 ms | 66 |
| | on the card | 14.3 ms | 7.8 ms | 70 |

The pass itself costs the card 0.04 ms. What else it costs the card, about 0.4 ms, is a draw
for every batch for every view, seen or not.

It wants a card that can begin an indirect draw's instances at a number of our choosing;
where it cannot, or with `MIRA_CULL=cpu`, the processor decides as before. The order of
instances within a batch is then whatever the card's threads came to them in.

What it does not do yet: leave out what is hidden behind other things, and leave out the
draws of batches nothing of which is seen.
## A real scene, and drawing it smaller

The field of things is hard on the processor and on the card's triangles. A scene such as
the Paris square is hard on neither: 1,358 instances, and a card that spends its frame
shading. At 2,560 by 1,440, with ray tracing, the card takes 23.3 ms a frame, 17.8 of them
in the main pass, which is 42 frames a second.

`PostProcess::render_scale` draws the scene smaller than the picture, from a half of its
width and height to the whole. The blend over time then makes the picture at its full size:
each frame's points fall elsewhere among the picture's pixels, by the frame's jitter, so
over a few frames the history holds more than any one frame drew. What is drawn at the
scene's size is the main pass, the light shafts and what moved; the bloom, the tone curve
and anything drawn over the picture stay at the picture's.

Paris, on the card:

| Scale | Main pass | The frame | Frames a second |
|---|---|---|---|
| 1 | 17.8 ms | 23.3 ms | 42 |
| 0.75 | 13.4 ms | 18.7 ms | 53 |
| 0.67 | 10.2 ms | 14.8 ms | 67 |
| 0.5 | 6.7 ms | 10.9 ms | 91 |

At two thirds the picture is a little softer than at full size and otherwise the same. The
default is 1. Without the blend over time (`taa` off) a smaller scene is only stretched.

The instances example takes `scale=`: at 2,560 by 1,440 with levels of detail, 100,000
things at rest go from 11.1 ms on the card to 8.2 at 0.67.
## What is next

On the processor, with things moving, where the time now is:

- Gathering what moved (Extract, 2.4 ms for 10,000 things) and the blend over time's list of
  it (the motion pass, 1.2 ms).
- Meshes in shared buffers, so that a pipeline's draws are one call, which wants a
  benchmark of many different meshes and materials first.

On the card, for a scene that gives no levels of detail: coarser meshes made by the engine.