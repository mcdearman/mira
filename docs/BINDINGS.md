# Bringing a language to mira

A language can write mira plugins once it can build a shared library that exports a few C
functions and calls through the table of function pointers the engine hands it
([`include/mira.h`](../include/mira.h); see [PLUGINS.md](PLUGINS.md)). This is the path
from nothing to a first plugin that works, and the test that says it does.

## The first plugin: the counter

The smallest real plugin is the counter (`COUNTER` in
[`src/plugin/tests.rs`](../src/plugin/tests.rs), in C). Writing it in a new language
exercises nearly everything a binding needs: exported entry points, calls through the
table, a component of its own, engine-kept state, a startup system and two systems with
queries, pointers to the engine's values written in place, and reloading.

[`include/mira_counter.h`](../include/mira_counter.h) is `mira.h` cut down to what the
counter uses: its table keeps every slot in order, as `void *` where the counter does not
call it, so a binding can describe just these and still find them where the engine puts
them. A test keeps it in step with the full header.

What the counter does:

- `mira_plugin_abi_version` returns `MIRA_ABI_VERSION` (1).
- `mira_plugin_load(api, app)` keeps `api`; looks up `mira.Transform` and checks that its
  size and alignment are those of `MiraTransform` (48 and 16), returning -2 if not;
  registers `test.Counter`, one `int32_t`; asks for the state block `test.loads`, one
  `int32_t`, and adds one to it; adds three systems; returns 0.
- `boot`, at `MIRA_STAGE_STARTUP`, no terms: spawns one entity and gives it a `test.Counter`
  of 100. (It has no transform, so the check below does not list it.)
- `adopt`, at `MIRA_STAGE_UPDATE`, terms `mira.Transform` WITH and `test.Counter` WITHOUT:
  gives each entity it visits a `test.Counter` of 0.
- `step`, at `MIRA_STAGE_UPDATE`, terms `mira.Transform` WRITE and `test.Counter` WRITE:
  for each entity, adds `STEP` to `translation[0]`, sets `translation[2]` to the number in
  `test.loads`, and adds one to the counter.

`STEP` is a constant of the build: 1 for the first library, 10 for the second. Systems are
`void` and the engine reads nothing they return: they say what they did through the table.
Every system is called on the one thread that also calls `mira_plugin_load`, one at a time.

## Checking it

`mira-plugin-check` loads a library into an engine with no window and says what it did:

```text
cargo run --bin mira-plugin-check -- counter1.dll --frames 5 --show test.Counter \
    --reload counter10.dll --broken not-a-library.dll
```

The engine starts with two entities with a transform at the origin. A plugin that passes
prints this, the entities' numbers aside (`adopt` gives the counter on the first frame and
`step` counts it on the same frame, since `adopt`'s insert takes effect after it):

```text
--- after 5 frames
plugins: counter1 (load 1)
entity 0: at (5, 0, 1); test.Counter = [5] as integers, ...
entity 1: at (5, 0, 1); test.Counter = [5] as integers, ...
failures: none
reloaded: 1
--- after the reload and 5 frames more
plugins: counter1 (load 2)
entity 0: at (55, 0, 2); test.Counter = [10] as integers, ...
entity 1: at (55, 0, 2); test.Counter = [10] as integers, ...
failures: none
reloaded: 0 (a broken library should not be)
--- after the broken library and 5 frames more
plugins: counter1 (load 2)
entity 0: at (105, 0, 2); test.Counter = [15] as integers, ...
entity 1: at (105, 0, 2); test.Counter = [15] as integers, ...
failures: none
```

So:

| After | X | Z (the loads) | `test.Counter` |
|---|---|---|---|
| 5 frames at a step of 1 | 5 | 1 | 5 |
| a reload at a step of 10, and 5 frames | 55 | 2 | 10 |
| a broken library offered, and 5 frames | 105 | 2 | 15 |

The data lives in the engine, so it carries over a reload untouched; the new code steps by
10 from where the old one left off; `test.loads` counts loads because the engine keeps it;
and a library that cannot be loaded leaves the last one running. The checker exits with 1
if the first load or the reload fails, or the broken one is taken.

The output above is the C counter's, built with clang. A language passes when its counter
prints the same.

## After the counter

The counter passes in Meadow too, written by hand over `Std.Ffi` and again with mira's
Meadow bindings ([`bindings/meadow`](../bindings/meadow)): the second is about forty lines,
and the example games ([`examples/sound`](../examples/sound),
[`examples/sacred_sites`](../examples/sacred_sites), [`examples/voxel_world`](../examples/voxel_world))
are written with the same bindings.

The rest of the table follows the same patterns. [`Mira.hs`](../bindings/haskell/Mira.hs)
is a whole binding, for Haskell, written over the C glue in
[`cbits/mira_hs.c`](../bindings/haskell/cbits/mira_hs.c); its examples are
[`examples/haskell`](../examples/haskell), the same games.
For the engine to build a language's scripts itself, as it does Haskell's
(`App::load_scripts`), it needs one command that turns a source file into a library at a
path it names, with a non-zero exit and readable errors when it cannot.
