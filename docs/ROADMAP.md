# mira roadmap

Where mira is going: a general-purpose engine in the class of Unreal, with voxel worlds as the
thing it does better than anyone. This is a long road. The plan is ordered so that every phase
leaves the engine more usable than before, and so that later phases stand on earlier ones
instead of being rewritten.

Phases are sized, not dated: **S** is days, **M** is weeks, **L** is months, **XL** is many
months. Sizes assume one person working with agents and are rough.

Each phase has an **exit test**: something concrete that works at the end and did not before.
A phase is not done until its exit test passes. Tick items off here as they land.

## What it runs on

mira is to be cross-platform, and games made with it are to ship on phones and tablets as
well as on desktops: Windows, macOS and Linux, and iOS and Android. This holds for every
phase, not only the last. A feature is not done while it works on one system only, and what
is added should not close off a phone: no assuming a keyboard and mouse, a large screen, a
desktop graphics card, or files beside the program. Where something cannot be had on a
platform (ray tracing, say), the engine goes without it there and still runs.

The work that is particular to it is listed under Phase 10.

## Where we are (October 2026)

| Area | Have | Missing |
|---|---|---|
| Core | Hand-written sparse-set ECS, function systems, change detection, commands, events, startup access checks, fixed timestep with interpolation; ordering, run conditions and states; systems and single queries run across threads; hooks, observers and relations; reflection and derive macros; signals | Table storage; hooks, observers and relations from plugins |
| Data | glTF and image loading, typed handles; JSON scenes and prefabs; an asset server that loads off the main thread, reloads what changes, unloads what nothing refers to, and compresses pictures to BC7 | ASTC, meshes in a binary form, streamed textures |
| Rendering | Forward HDR + MSAA + TAA with motion vectors, PBR, image-based lighting, cascaded sun shadows, bloom, light shafts, decals, GPU skinning, hand-authored LODs; the frame as a graph of passes; what is seen decided on the card; lights with a place, by the thousand, with shadows; surface shaders; particles; temporal upscaling | Occlusion culling, bindless textures, shading each pixel once, particles moved on the card |
| Lighting | Ray-traced shadows/AO/reflections (Vulkan only), light probes baked once | Dynamic global illumination, anything ray-traced on Metal |
| Voxels | Chunked storage, threaded generation and meshing with ambient occlusion, streaming, raycasts, saving edits | Greedy meshing, chunk LOD, transparent blocks, voxel lighting |
| Simulation | Rigid bodies with sleeping, joints, character controller, fluids; shapes: sphere, box, capsule, plane, static triangle mesh | Convex hulls, continuous collision, ragdolls, vehicles, cloth, destruction |
| Animation | Clips, cross-fades, masks, two-bone IK, procedural gait, skins loaded from glTF | State machines, blend trees, retargeting, compression |
| Runtime | Keyboard, mouse, touch and game controllers, bound to named actions; sound; saved games; finding a way on a grid; text and panels in the game, with a first set of controls | Behaviour trees, a true navigation mesh, networking, a game run on a phone |
| Plugins | Native plugins over a C interface (`include/mira.h`) in any language, hot-reloaded with their data intact; Rust and Haskell bindings; a whole game as a plugin | Animation, joints and much else of the engine from a plugin (see Phase 1A) |
| Tools | The engine app: the game in a window with its entities, an inspector, signals, assets and an agent beside it; a debug connection and an MCP server; screenshots and image-diff tests; CI on Linux and macOS with clippy, Miri and ThreadSanitizer | Play in editor, material editor, a profiler's timeline |

## Principles

1. **Foundations before features.** Reflection comes before serialization, which comes before
   the editor. GPU-driven drawing comes before virtualized geometry.
2. **One renderer.** New techniques replace old paths rather than sitting beside them. The
   forward path is retired when the GPU-driven one matches it.
3. **Everything runs on Metal.** Hardware ray tracing stays an optional upgrade. Any feature
   that needs rays gets a software path first.
4. **Voxels are first-class.** Each rendering phase names what it does for voxel terrain, not
   only for meshes.
5. **Sound by construction.** The ECS's unsafe code stays behind declared access, and stays
   clean under Miri.

## Phase 0: Footing (M)

Make the project safe to change quickly.

- [x] The ECS as a crate of its own (`crates/mira_ecs`), which the engine re-exports as
      `mira::ecs`; it depended on nothing else, and Miri now builds only it
- [ ] The rest of the split: `mira_core`, `mira_render`, `mira_voxel`, `mira_physics`. These
      lean on each other (the app, reflection, assets and rendering all meet), so each needs
      its seams found first
- [x] CI on macOS and Linux: build, test, clippy, Miri on the ECS (written; not yet seen to
      pass on GitHub)
- [x] Image-diff tests: a fixed scene is stepped to a set frame, drawn, and compared with a
      stored frame (`tests/frames.rs`, `render::frame_diff`); run where there is a graphics
      card with `MIRA_FRAME_TESTS=1`
- [ ] Image-diff tests of the bigger demos (voxel world, Paris, Napoleonic), which load in
      the background and so are not yet at the same moment every run; run in CI (the runners
      have no graphics card set up)
- [x] CPU frame timings: per frame, per stage and per system, from the running game
      (`FrameStats`, `mira-debug profile`)
- [x] GPU timings: what the graphics card takes over each pass, read back from its own
      clock (`mira-debug passes`, `card_millis` in `mira-debug profile`; see
      [PERFORMANCE.md](PERFORMANCE.md))
- [ ] A timeline of a frame across threads; the timings shown on screen
- [x] A benchmark of a frame as the number of things in it grows (`examples/instances`;
      what it shows is in [PERFORMANCE.md](PERFORMANCE.md))
- [ ] Benchmarks for ECS iteration and chunk meshing, and any of them run in CI

**Exit test:** a change that breaks rendering or halves ECS speed fails CI.

## Phase 1A: Plugins and hot reload (L)

Pulled forward from Phase 9 at Chris's request (October 2026): hot reload and plugins are a
priority, and plugins may be written in any language that can speak a C interface. The
contract is `include/mira.h`; see [PLUGINS.md](PLUGINS.md).

- [x] Components defined at runtime by name, size and alignment, checked by the same access
      rules as Rust components
- [x] The C interface, version 1: define and find components, persistent state, systems with
      one query each, deferred spawn/despawn/insert/remove, time, logging
- [x] Loading shared libraries, with interface-version checking
- [x] Hot reload: a rebuilt plugin replaces itself in the running app, keeping component data,
      state and system order; a broken rebuild leaves the old version running
- [x] Rust bindings (`crates/mira_plugin`) and example plugins in Rust and C
- [x] Haskell as the default plugin language: typed bindings (`bindings/haskell`), an example
      plugin, and hot reload that survives garbage collection. Meadow is meant to take over
      this role when it is ready.
- [x] Input (keys, mouse), meshes (shapes and custom), mesh and material on an entity
- [x] More than one query per system, checked against each other like typed queries
- [x] A generic host (`examples/host`) and a whole small game as a Haskell plugin
      (`plugins/chase`)
- [x] Events between plugins and from the engine, with each reader's place kept across
      reloads; cameras, lights and the window title; colliders, bodies, impulses, raycasts and
      contact events. The example game now needs nothing from its host.
- [x] Plugins describe their components' fields, so scenes and inspectors handle them; plugins
      load images and spawn models by name
- [x] Hierarchy from plugins: `set_parent` and `despawn_tree`
- [ ] Reach more of the engine from a plugin: animation, joints and character controllers,
      voxel terrain, text and UI, sound
- [x] Ordering a plugin's systems (`system_order`); orthographic cameras
      (`set_camera_orthographic`)
- [ ] Optional terms and change filters in queries; hooks from plugins
- [ ] Removing a plugin at runtime, with its components and systems
- [ ] A generic script host for languages that can't build a shared library themselves
- [x] Meadow bindings, once Meadow can export C functions (`bindings/meadow`)
- [ ] Bindings generated from the header for at least one more compiled language (Zig)
- [x] Hot reload for shaders and textures (see [HOT_RELOAD.md](HOT_RELOAD.md))
- [ ] Hot reload for meshes, models and scenes (shared with Phase 1), and for the shaders that
      only run on Vulkan
- [ ] Hot reload for the engine's own Rust gameplay code through the same mechanism

**Exit test:** the Phase 4 sample game's rules live entirely in plugins, in two languages, and
can be rewritten while the game is being played.

## Phase 1B: A live program (L)

Added at Chris's request (October 2026), after Bret Victor's talks and Jack Rusher's "Stop
Writing Dead Programs": in debug builds the game is a program you change, question and repair
while it runs, not one you restart. Hot reload (Phase 1A) is half of this; the other half is
being able to see inside.

- [x] A failure stops the game where it is instead of ending it: a system that panics is
      caught, its stack and message kept, and the game paused with the world intact; fix the
      code (a plugin reloads itself), resume; see [LIVE.md](LIVE.md)
- [x] Time under control: pause, step frames, slow motion
- [x] Every system's name, stage, order, constraints and run times, from the running app
- [x] What each system reads and writes, from the running app
- [x] A debug connection (a local socket speaking JSON) to a running game: list and search
      entities, read and change any registered component or resource by name, spawn and
      despawn, save the scene, pause and step, see failures with their stacks, reload
      plugins, read and rewire the signal graph
- [x] A command-line client for it (`mira-debug`)
- [ ] The same protocol under the editor (Phase 5)
- [ ] Hot reload of the host's own Rust systems, not only plugins (the engine as a library the
      game reloads)
- [x] Rewind: snapshots of the world every few frames (reflection makes them); step back to
      one, change code or state, play forward again
- [ ] Snapshots of only what changed; scrubbing both ways; restoring what signals have timed
- [x] Failures across the plugin boundary: a plugin system that throws (Haskell), panics
      (Rust) or calls `system_fail` (C) pauses the game with its message, and reloading the
      fixed plugin resumes it
- [ ] Haskell call stacks with the exception; catching crashes in native plugin code

**Exit test:** make a plugin system divide by zero while the sample game runs; the game
freezes on that frame and shows the stack; fix the line, save, and play carries on from the
same frame without the window ever closing.

## Phase 1C: Agents at the controls (M)

Added at Chris's request (October 2026): an AI agent should be able to drive the engine for
development as fully as a person at an editor: see the scene, know everything about the
game's state, change it, and control time and code. The means is the Model Context Protocol
(MCP), on top of the debug connection of Phase 1B ([LIVE.md](LIVE.md)).

- [x] An MCP server (`mira-mcp`, JSON-RPC over stdio) that connects to a running game by its
      debug address, with a tool for every command of the debug connection: entities,
      components and resources by name, spawn and despawn, systems, failures with stacks,
      pause, step, rewind, time scale, signals, plugin reload, scene save; see
      [MCP.md](MCP.md)
- [x] Seeing the scene: a screenshot tool that returns the next frame as an image, at a
      chosen size
- [ ] Screenshots from a chosen camera or a free viewpoint, with debug overlays (entity ids,
      bounds, colliders, the signal graph); loading a scene
- [x] The scene in words: where the camera is and what there is, on screen first and nearest
      first (`describe`), for when an image is more than is needed
- [ ] What changed since last asked; sizes and what hides what
- [x] The schema of every registered type, so an agent knows what it may read and write and
      in what shape
- [x] A report of state that is not reflected, and so invisible (`unregistered`)
- [ ] Total knowledge: registering what that report lists in the engine itself (the
      renderer's and physics' resources, `GlobalTransform`, animation)
- [x] Signal rules as text, and the text and the graph kept the same: a file saved changes
      the running rules, and a rule changed in the graph rewrites its line and no other
      (see [SIGNALS.md](SIGNALS.md#rules-as-text)). The first step toward the node editor below
- [ ] A node editor for scripts, as Blueprint is, that is a view of the code: an edit to
      either changes the other at once. For Meadow, whose tools must give a syntax tree
      that loses nothing, types for the wires, and names for definitions that outlast an edit
- [ ] The rules' text beside their graph in the panel; where boxes sit kept with the text
- [x] Events pushed, not polled, over the debug connection: failures, signal changes, pauses
      (`watch`, `mira-debug events`)
- [ ] The same events through MCP; log lines and plugin reloads as events
- [x] Running to a condition: step frames and wait for them, or run until a signal is true
- [x] Input from the agent: keys and mouse injected as if played, so an agent can play-test
      what it built
- [x] Launching and owning a game from MCP: start it by its command line, read its log, shut
      it down
- [x] Running with the window hidden (`MIRA_HIDDEN=1`), rendering off screen; a game an
      agent launches is hidden by default
- [ ] Rendering on a machine with no display; seeding; gamepad and text input
- [ ] Editing through the same door: write a prefab or a scene, define signals, build and
      reload a plugin, and see the result, without leaving the conversation
- [ ] The same tools from inside the editor (Phase 5), so a person and an agent can work on
      one running game

**Exit test:** an agent with only the MCP tools is asked to "make the red team win faster in
the sacred-site game and show me": it finds the rule in the signal graph, changes it, steps
the game until `red.wins`, and returns a screenshot of the moment.

## Phase 1: Data layer (L)

The layer Unreal's editor, saves, networking and Blueprints all stand on.

- [x] Proc-macro crate: `#[derive(Component)]`, `#[derive(Reflect)]`
- [x] Type registry: look up a type by name, list and edit its fields at runtime
- [x] Serialization of any reflected value; a text scene format (JSON); see
      [SCENES.md](SCENES.md)
- [x] Components defined by plugins are reflected once the plugin describes their fields
- [x] Prefabs: a scene by name, instanced under an entity, rebuilt when its file changes,
      with per-instance overrides; plugins can spawn them
- [x] Reflecting the physics components (bodies, colliders with every shape, joints,
      character controllers), `Lods`, `Interpolate`, `ChunkViewer`
- [x] Scenes capture registered resources (a level's fog and ambient light)
- [x] Saving edited voxel chunks: an edited chunk keeps its blocks when it streams out, and
      the edited chunks of a world save to one file and load back. [VOXELS.md](VOXELS.md)
- [x] Saving skeletal animation state: `Playing` holds the clip, the moment and the speed as
      data beside a `Model`, and the animator takes them up when the model arrives. What
      inverse kinematics and procedural walking remember is not saved
- [x] Hierarchy as a real feature: `Children` kept from `Parent`, recursive despawn, and
      propagation that walks down from the roots (one multiply per entity at any depth)
- [x] Asset server: assets by name (files, parts of model files, shapes), images decoded on
      worker threads and reloaded when the file changes, scenes saving assets by name; see
      [ASSETS.md](ASSETS.md)
- [x] Unloading assets nothing refers to, by tracing reflected components (handles stay
      plain ids): `AssetServer::unload_unused`
- [x] Model files read on a worker and watched: a `Model` component names the file, its
      parts appear under the entity when read and again when the file is saved
- [x] Dependencies between assets: what each needs is kept by name (a model and its parts,
      a prefab and what it uses), with load states, `is_ready`, and unloading that follows it
- [x] Asset processing, first part: picture files to BC7 with their smaller levels, on
      workers, cached on disk by content hash, with a limit on the cache
- [x] The textures inside model files, compressed where the file is read
- [ ] ASTC for the cards that want it; meshes to a binary format
- [x] Skins and animations from glTF, in code (`GltfScene::spawn_animated`) and as data (a
      `Model` whose file has a skeleton gets an `Animator` and skinned parts)

**Exit test:** save the Paris scene to a file and load it back identical; edit a texture on disk
and see it change in the running scene.

## Phase 2: Parallel core (L)

- [x] System ordering: before/after, system sets, chains, run conditions, application states
      with enter and exit systems; see [SCHEDULING.md](SCHEDULING.md)
- [x] Groundwork for running systems in parallel: components must be `Send + Sync` (they all
      were), systems and conditions report what they touch, and each stage is planned into
      batches of systems that could run at the same moment (shown by the debug connection)
- [x] Thread safety settled in the types: resources read through `Res`/`ResMut` must be
      `Send + Sync`, system state and commands `Send`; a resource can be pinned to the main
      thread (the window is), and each system is marked if it must run there (pinned
      resource, whole world, or a plugin's code)
- [x] The parallel executor: a pool of worker threads runs each batch, pinned systems stay on
      the main thread, commands are applied between batches in order; results identical to
      running in turn; clean under Miri with worker threads
- [x] Splitting one query's work across threads: `Query::par_for_each` and
      `par_for_each_mut`, on the systems' own pool, whose waiting threads now do queued work
- [x] A ThreadSanitizer run of the ECS's tests in CI
- [x] Relaxing "commands end a batch" where a later system needn't see them:
      `.commands_can_wait()`
- [x] A faster walk through the storage there is: a query guesses that an entity sits at the
      same place in each component it reads as in the one it walks, which is so for
      components spawned together. Measured with `examples/query_bench` (200,000 entities,
      one thread): one component 3.8 to 1.6 ns an entity; three components spawned together
      6.4 to 5.0; three added in different orders unchanged at 7.4
- [ ] Table storage as an option beside sparse sets, chosen per component. By the same
      measurement it would win back the gap between "spawned together" and "added in
      different orders" (about a third) and little else, for a rewrite of storage, queries
      and everything that reaches components by name; worth doing when a game shows that gap
- [x] Hooks: run code when a component is added or removed (`on_add`, `on_remove`)
- [x] Observers: `on_change` for a component that was written, told when the stage has run;
      events aimed at an entity (`trigger`, `observe`, `observe_entity`), heard at once
- [ ] The same from plugins;
      hooks from plugins
- [x] Relations: any component that names another entity, with the way back kept by the
      engine (`Relation`, `Related<R>`); `Parent` and `Children` are one of them. See
      [RELATIONS.md](RELATIONS.md)
- [x] Relations with many targets of one kind (`each_target`), and what becomes of a source
      when its target is despawned (`WhenTargetGoes`: keep, unlink or despawn)
- [ ] Relations from plugins
- [x] One job system for systems, asset loading and voxel work: one pool with a queue for
      the frame's work, taken first, and one for background work, which leaves a worker free

**Exit test:** the Napoleonic demo's update time drops in proportion to cores used, with Miri
and a thread sanitizer clean.

## Phase 2B: Signals (M)

Added at Chris's request (October 2026). Game rules written as handlers that set and clear
shared flags get out of step with each other. His example is a bug in Age of Empires 4:
standing on a sacred site pauses the win timer; step onto a second site and the units on the
first die and the timer starts again. The rule should be stated once, as a function of the
world: the timer is paused while anyone on your team stands on any sacred site the other
team holds. That is a signal, in the sense of functional reactive programming: a value
derived from other values, which the engine keeps true.

- [x] Signals: a named value computed from queries over the world and from other signals,
      read by systems through the `Signals` resource; see [SIGNALS.md](SIGNALS.md)
- [x] Signals as run conditions, and edges (became true, became false) as conditions and
      events, so "when the timer un-pauses" is written once
- [x] Signals over time: held for, and a timer that runs only while a signal is true (the
      sacred-site clock itself)
- [ ] Sources skipped when what they read hasn't changed
- [ ] Dependencies known to the engine: which signals read which, shown in the debug
      connection, with the current value of each (Phase 1B)
- [x] Signals from plugins, through the C interface (set, get, define), with Rust and Haskell
      bindings
- [x] A Haskell layer in which a rule reads as an expression (`defineRule`, `sig`, `.&&.`,
      `notS`, `timer`, …)
- [x] The sacred-site rule as a worked example and a test
- [x] Signals as data, not closures: a graph of named nodes (sources read from the world;
      operations such as and, or, not, count, compare, held-for) joined by connections, which
      can be listed with their values and rewired, forced and redefined while the game runs
- [x] Saving and loading a game's rules (`Signals::save_rules`, `load_rules`)
- [ ] Signals carrying entities
- [x] The signal graph over the debug connection: read every node and its value, set
      constants, connect inputs, force outputs, define and remove nodes, while the game runs
- [x] A signal graph viewer in the terminal (`mira-debug watch`): which signals are active as
      you play, what feeds what, what just changed; edited live from a second terminal
- [x] An orthographic camera (`Camera::orthographic(height)`), with shadows, sky and fog
      working under it
- [x] `examples/sacred_sites` as a 3D game under an orthographic camera, drawn from its
      signals (the graph is watched from a terminal)
- [x] The signal graph shown live inside that game (`crates/mira_ui`, example
      `sacred_sites_graph`); see [UI.md](UI.md)
- [x] The in-game view as a circuit: boxes, ports and wires lit when true; rewiring by
      dragging, forcing, and changing constants, on the running game
- [x] In the panel: adding and removing inputs by dragging wires, changing a box's
      operation, adding constants, removing signals
- [x] The panel's layout is kept with the rules and saved with them
- [x] Naming signals, timers and comparisons from the panel; typing numbers (click a box's
      name or its lower line; `Signals::rename`, `signal_rename`)
- [ ] The graphical editor of the graph (Chris, October 2026): signals and connections laid
      out and edited by hand while the game runs; in the game's panel and in the editor
      (Phase 5)

**Exit test:** the sacred-site game: two teams, several sites, units walking on and off and
dying in any order; the timer is right in every case because no code ever sets it.

## Phase 3: GPU-driven renderer (XL)

- [x] Render graph: passes declare what they read and write; the graph orders them and
      allocates targets (see [RENDER_GRAPH.md](RENDER_GRAPH.md)). The frame's own passes run
      through it
- [x] The rest of the frame's targets made by the graph (`hdr`, `depth`), and the debug
      lines and overlays as passes. What outlasts a frame (the history the blend over time
      keeps) stays with its pass
- [x] Shaders made of modules: imports by name, and parts kept by what is defined, so one
      source gives its variants (see [SHADERS.md](SHADERS.md)). The engine's lighting is a
      module its shaders import
- [x] Materials with a shader of their own: an entity's `Surface` names a module that has the
      last word on what is lit, its `Material` gives the values, and every entity naming
      one surface shares its pipelines
- [ ] Further for materials: values of a surface's own beyond the standard ones, and the
      shadow, the traced reflection and the vertices in a surface shader's reach
- [x] Instances kept on the card: one for each mesh an entity draws, written only when its
      entity changes; each view sends the numbers of those it sees, and each shadow cascade
      of those that cast into it (see [PERFORMANCE.md](PERFORMANCE.md))
- [x] The traced scene kept from frame to frame, in two structures: what stands still, and
      what moves, which is the only one built again when something does (see
      [PERFORMANCE.md](PERFORMANCE.md))
- [ ] The rest of the scene on the GPU: shared vertex and index buffers, bindless textures
- [x] What each view and each shadow cascade sees decided on the card, in a compute pass,
      and drawn with indirect draws (`src/render/cull.rs`; see
      [PERFORMANCE.md](PERFORMANCE.md))
- [ ] Culling against a depth pyramid as well, so that what is hidden behind other things is
      not drawn; a pipeline's draws as one multi-draw
- [x] Multisampling made a choice (`Multisampling`), for scenes whose edges the blend over
      time smooths well enough (see [PERFORMANCE.md](PERFORMANCE.md))
- [ ] Visibility buffer or deferred shading, so that each pixel is shaded once
- [x] Point and spot lights, clustered: sorted each frame into cells of the view, so a
      surface goes through only the lights that reach it (see [LIGHTS.md](LIGHTS.md))
- [x] Shadows for lights with a place: sixteen maps, one to a spot light and six to a point
      light, given to those nearest the camera that ask
- [ ] Shadows for more lights at once, in an atlas with maps sized by how near the light is;
      the lights in traced reflections and the probes
- [x] Temporal upscaling: the scene drawn smaller than the picture and made up to its size
      as frames are blended over time (`PostProcess::render_scale`; see
      [PERFORMANCE.md](PERFORMANCE.md))
- [x] Particles as an engine feature: lit, soft puffs let out by an emitter or by a system,
      moved on the processor (see [PARTICLES.md](PARTICLES.md)); the demos' smoke is these
- [ ] Particles moved on the card, for many more of them; emitters saved in scenes; sorted
      or order-independent transparency
- [x] Voxels: faces that lie side by side joined into large quads (greedy meshing)
- [ ] Voxels: a pass for blocks that are seen through, and chunk draws through the same
      indirect path as meshes

**Exit test:** 100,000 mesh instances and 1,000 shadow-less lights at 60 fps on the M2 Pro.

## Phase 4: Runtime for real games (XL)

- [x] Text and panels in the game: Armature underneath, a thin mira layer on top
      (`crates/mira_ui`: input shared with the game, drawn over the frame); see [UI.md](UI.md)
- [x] A look and a first set of controls for games (`mira_ui::kit`: theme, panel, button,
      toggle, slider, bar, anchoring)
- [x] A text field (`kit::field`), and typing played in from outside (`mira_input` `text`)
- [x] Tabs, a choice between options, lists and scrolling (`kit::tabs`, `choice`, `list`,
      `scroll`)
- [ ] Dropdowns and other pop-ups; interface in the world; gamepad focus
- [x] Flat games: pictures drawn with their pixels sharp (`Image::pixelated`), and how a
      game seen from above is made of what there is (see [FLAT.md](FLAT.md))
- [ ] Flat games: sprite sheets, tiles as one mesh, and text that a game can write without
      a font of its own
- [x] Audio: sounds mixed, heard from where they are, and music read as it plays (see
      [AUDIO.md](AUDIO.md))
- [ ] Audio: sounds loaded by name and sources saved in scenes; buses and effects
- [x] Input: named actions and axes, rebinding, and bindings kept in a file (see
      [INPUT.md](INPUT.md))
- [x] Input: touch. Fingers, taps and swipes, bound to the same actions as keys (see
      [INPUT.md](INPUT.md)); not yet tried on a phone
- [x] Input: game controllers. Buttons bound to actions and sticks to axes, beside keys
      (see [INPUT.md](INPUT.md)); not yet tried with one plugged in
- [ ] Input: two-finger gestures, a stick and buttons drawn on the screen; rumble
- [ ] Animation graph: state machines, blend trees, retargeting, root motion
- [ ] Physics: convex hulls, continuous collision, ragdolls; decide here whether to keep the
      custom solver or adopt Rapier
- [x] Navigation: a grid of where there is standing, made from a level's colliders or a
      world's blocks; the way over it; and a component that walks it (see
      [NAVIGATION.md](NAVIGATION.md))
- [ ] Navigation: more than one height to a cell (a true navmesh), walkers keeping out of
      each other's way; behaviour trees
- [x] Save games, built on Phase 1: the world saved under a name and loaded in any run (see
      [SAVES.md](SAVES.md))
- [ ] Voxels: save edited chunks to region files; player collision against blocks

**Exit test:** ship a small complete game (menu, saves, sound, enemies that find their way)
using only the engine.

## Phase 5: Editor (XL)

Built with [Neo](https://github.com/mcdearman/neo), Chris's GUI toolkit, on top of the
engine's reflection. Before starting, work out with the Neo project what the
editor needs from it.

- [x] Agree with Neo on what it must provide: a wgpu viewport inside a Neo window, dockable
      panels, tree and property views, drag and drop, a transcript for the agent window; see
      [EDITOR.md](EDITOR.md). Neo is public, so the app can be built in this workspace
- [x] The game run by a host: on a device it is given, with no window or loop of its own,
      its frames a texture for the host to show (`App::host`, `render::frame_texture`)
- [x] The first app: a Neo window with the game in a viewport, and a bar to pause, resume
      and step it
- [x] The app is the engine's own program: bare `cargo run` opens it, on a starter scene or
      on the plugins named; the app is a package of its own, `crates/mira_app`, which the
      workspace runs by default, and the engine does not depend on Neo; the voxel world
      that used to be `cargo run` is `--example voxel_world`
- [x] Panels in a dock, arranged by dragging and kept between runs; the game's entities and
      signals listed live beside it

- [x] Entity tree (select, open and shut, drag to reparent) and a read-only inspector
      generated from reflection
- [x] Editing in the inspector: numbers, vectors and switches change the running game
- [x] Colours picked as colours, and entity references set by dragging from the tree
- [ ] In the inspector still: text and asset references, and
      one undo step per edit; renaming (mira has no name component yet); transform gizmos
- [x] An Assets panel listing the project's models, pictures, scenes and prefabs; placing a
      model or a saved scene; saving the chosen entity as a prefab
- [ ] Thumbnails and drag to place; prefab copies that follow their file
- [x] Neo's look: the desktop's appearance, a glass window by default with a switch in
      Settings, and a tool bar of mira's own
- [x] Choosing an entity by clicking it in the picture, and moving it over the ground by
      dragging, or along one direction by a handle; a frame round the chosen one
- [x] Undo and redo of what is changed from the app; saving the scene and opening one;
      names for entities (`Name`), given in the tree
- [ ] Play in editor: the scene as edited kept apart from the game as it runs, so that
      stopping puts it back
- [ ] Material editor; voxel sculpting and painting tools
- [x] Panels for what the engine already knows: Log (the engine's log is now kept as well as
      printed), Console, Profiler, Systems, World settings, History, Time, Failures,
      Plugins, Statistics; a Window menu to open and shut them
- [x] The signal graph as a panel (the circuit from `mira_ui`, which now draws from its own
      corner so that it sits in a panel as well as over a whole game)
- [x] A Place panel: shapes, a sun, a camera and an empty entity put in the scene; delete and
      duplicate with everything under the entity; all in the undo history, which now
      covers entities made and taken away as well as components changed
- [x] A scene camera of the app's own, apart from the game's: turn, fly, move in and out
- [x] Panels that run git and cargo in the project (Changes, Tests, Build) and show what
      they say; a References panel of what the scene uses by name
- [x] The app arranged after Unreal: game, entities over inspector, and drawers (Assets,
      Agent, Log, Console) shut down to a strip along the bottom; flat tabs parted by lines
- [ ] A render-graph viewer
- [x] The agent window's first form: a conversation panel under the game, with Claude Code
      run behind it and given the game's own tools (see [EDITOR.md](EDITOR.md#the-agent))
- [ ] The agent window in full: a panel of the engine app in which to talk to an AI agent that is
      working on the game in front of you, so that no second app need be open. Asked for by
      Chris (October 2026), as the other half of Phase 1C: there the agent reaches the engine
      from outside; here it sits inside it.
  - The conversation, with what the agent does shown as it does it: each tool call and its
    answer, screenshots inline, the signal graph and entities it names as links into the
    other panels
  - The agent drives this very engine through the same tools as from outside (`mira_*`),
    served in-process, with no socket to set up
  - The agent itself is a program the window runs and talks to, not something built into
    the engine: any agent that speaks a common protocol can be put there. To be settled
    before building: which protocol (the Agent Client Protocol that editors use for this is
    the likely one) and how the agent is told about the engine's tools
  - Saying yes or no to what an agent asks to do; stopping it; and the game's own pause,
    step and rewind beside the conversation, since they are how its work is checked
  - What you have selected and are looking at (the entity, the signal, the camera's view)
    given to the agent as context without being typed

**Exit test:** rebuild the Paris square without writing Rust for placement.

## Phase 6: Dynamic global illumination (XL)

mira's answer to Lumen. The voxel grid is the advantage: rays can step through it on any GPU.

- [ ] Block occupancy as a 3D texture clipmap, with a distance field for fast stepping
- [ ] Meshes join the same scene through per-mesh distance fields
- [ ] Probe grid updated every frame on a budget, replacing the baked probes
- [ ] Cached surface lighting so bounces accumulate (per block face for voxels, cards for meshes)
- [ ] Screen-space probes for the final gather; traced reflections for smooth surfaces
- [ ] Hardware ray tracing as a quality tier of the same pipeline

**Exit test:** knock a hole in a cave roof and watch daylight spread and bounce inside within a
second, on Metal.

## Phase 7: Virtualized geometry and textures (XL)

mira's answer to Nanite.

- [ ] Meshlets and a cluster hierarchy built at import (meshoptimizer)
- [ ] Per-cluster LOD selection and two-pass occlusion culling on the GPU
- [ ] Compute rasterizer for pixel-sized triangles
- [ ] Cluster streaming; virtual shadow maps; virtual texturing
- [ ] Voxels: chunk LOD out to the horizon with seamless transitions

**Exit test:** a scene with a billion source triangles holds 60 fps, with frame time that
depends on resolution rather than scene size.

## Phase 8: World scale (L)

- [ ] Streaming for non-voxel content in cells; large coordinates without jitter
- [ ] Foliage, water, weather and time of day as engine features (lifted from the demos)
- [ ] Destruction: meshes that fracture, voxel structures that collapse

**Exit test:** fly 100 km in a straight line with no hitch and no precision artefacts.

## Phase 9: Networking and scripting (XL)

- [ ] Replication driven by reflection; client prediction and rollback from world snapshots
- [ ] A sandboxed modding layer for untrusted code (WebAssembly is the candidate). Trusted
      native plugins and hot reload are Phase 1A.

**Exit test:** a 16-player session of the Phase 4 game, with a mod loaded at runtime.

## Phase 10: Shipping (L, ongoing)

- [ ] Packaged builds for Windows, Linux and macOS; a web build on WebGPU
- [x] Windows in CI beside Linux and macOS: built, tested and linted on every change
- [x] Native plugins on Windows: C and Rust plugins are built, loaded, reloaded and tested
      there (the tests compile C with clang)
- [x] Haskell plugins on Windows: built by the same script, run from Git's `sh`; each
      library carries its own GHC runtime there, as GHC has none to share on Windows
- [x] Meadow scripts: mira's bindings for Meadow (`bindings/meadow`), `.mw` files built by
      the engine with MeadowBoot, and the sound example in Meadow (`examples/meadow/sound`)
- [x] Meadow: a system that fails is reported and the game goes on; the sacred sites and
      the voxel world in Meadow, played by the same tests as in Haskell
- [x] Meadow scripts with the whole interface (physics, events, images, models, system
      order), and MeadowBoot installed in CI (Meadow's prerelease, on Linux and Windows)
- [x] Meadow the language the examples and the docs lead with, Haskell beside it: the games
      are Meadow scripts (`examples/sound`, `sacred_sites`, `voxel_world`), each also in
      Haskell (`examples/haskell`), which the editor's tests and the hosted game use; CI runs
      the Meadow scripts on Linux, macOS and Windows (MeadowBoot pre.4)
- [x] Scripts built by the engine itself, so that no shell is needed on any system: a game
      is a folder of scripts, built, loaded, and built again when one is saved
      (`mira-play <folder>`; see [PLUGINS.md](PLUGINS.md))
- [ ] Every example a folder of scripts: the Rust examples ported, and what the scripts'
      interface lacks for them added (sound, named actions, touch, particles, lights, voxels,
      finding a way, saved games, the interface, flat pictures)
- [ ] A game of scripts packaged to give to someone: built once, with no compiler needed
      where it is played
- [x] The engine is built for Android on every change (see [ANDROID.md](ANDROID.md)); a
      game there is told its activity, and keeps its saved games among the app's own files
- [x] A game packaged for Android: Tail Lights as an `.apk`, built by a workflow
- [x] A game seen to run on real Android phones: Tail Lights on a Pixel 8 and a Galaxy S26
      (see [ANDROID.md](ANDROID.md#on-real-phones))
- [ ] What is seen decided on the graphics card on phones too: on a Galaxy S26 it drew
      nothing, so phones decide it on the processor for now
- [ ] Mobile, so that a game can be made for phones and tablets: builds and packaging for iOS; the app's life on
      a phone (put away, brought back, the screen turned,
      the surface lost and made again); touch as input, through the same actions as keys;
      an interface that fits a small screen and its cut-outs; sound and saved games in the
      places a phone allows; a renderer that holds its frame rate on a phone's graphics
      (no ray tracing, compressed textures a phone reads, a render scale chosen for it)
- [ ] Game controllers on phones (on Windows, macOS and Linux they are heard from)
- [ ] Pipeline caching, crash reports, localization, accessibility settings
- [ ] Documentation, a book, and sample projects

**Exit test:** someone who has never seen mira builds and ships a game with it from the docs.

## What can run alongside

Phases 0 to 2 are strictly ordered, with 1A alongside them. After that, three tracks can
advance independently:

- **Rendering:** 3, then 6, then 7
- **Game runtime and tools:** 4, then 5, then 9
- **World:** the voxel items in each phase, then 8

## Not planned

Things Unreal has that only make sense with a team or licences: console support, a
MetaHuman-style character pipeline, film-grade cinematics tools, a marketplace. Revisit if the
project grows.

## Next three steps

1. Phase 3, the GPU-driven renderer: the frame as a graph of passes, culling and drawing
   driven from the GPU, clustered lights, and materials as data.
2. What Phases 1 and 2 leave: hooks, observers and relations from plugins; meshes in a
   binary form and ASTC; table storage if a game shows the need. The Paris demo saved to a
   scene and loaded back is still Phase 1's exit test, and waits on its assets going
   through the asset server.
3. In the app: the first run of the agent against Claude Code, made by hand, and the panels
   that wait on engine systems (materials, animation, particles, audio, navigation) as
   those systems arrive.
