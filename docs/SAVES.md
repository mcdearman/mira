# Saved games

A saved game is the whole world as a [scene](SCENES.md), with a few words about it, kept
under a name: a slot.

```rust
let saves = SaveGames::for_game("Sacred Sites");

saves.save(&app.world, "before the bridge", "Chapter 2, 14 gold")?;

let (info, spawned) = saves.load(&mut app.world, "before the bridge")?;
for problem in &spawned.skipped {
    log::warn!("{problem}");
}
```

## What is saved

What a scene holds: the registered components of every entity, and the registered
resources. A game's own state is in its saves once it is registered:

```rust
app.register_type::<Health>().register_resource_type::<Score>();
```

An entity marked `NotSaved` is left out, and left alone when a save is loaded: for an
interface, a camera the program owns, anything that is the program's and not the game's.

## Loading

Loading makes the world what was saved and nothing else. Everything a scene would capture
of the world as it is now is despawned, and what was saved is put in its place, its
resources with it. A save is read in any run of the game, not only the one that made it.

A component of a type the game no longer has, or whose value no longer fits, is left out
and told of in `Spawned::skipped`; the rest of the save loads. So does a save from before a
field was added, where the type is marked `#[reflect(default)]`.

A file that is not a save, or is a save of a version this engine does not read, is refused
with why, and the world is left as it was.

## Where they are kept

`SaveGames::for_game(name)` keeps them where the system keeps a program's own files for the
person using it: `%APPDATA%\<name>\saves` on Windows, `~/Library/Application
Support/<name>/saves` on a Mac, `$XDG_DATA_HOME/<name>/saves` or
`~/.local/share/<name>/saves` elsewhere. `SaveGames::in_folder(path)` keeps them where it
is told.

A slot's name is letters, digits, spaces, `-` and `_`. Each is one file,
`<slot>.save.json`. It is written beside where it goes and then put in place whole, so a
save that is cut short leaves the one before it as it was.

## A list of them

```rust
for save in saves.list() {            // the latest first
    println!("{}: {} ({:?} played)", save.slot, save.note, save.played);
}
saves.info("slot 1")?;                // one of them, without loading it
saves.exists("slot 1");
saves.delete("slot 1")?;
```

`SaveInfo` has the slot, the note, when it was saved, how long the game had been played by
its own clock, and how many entities it holds.

## What there is not yet

- A picture of the moment with each save.
- Saves made smaller: they are the scene's JSON as it is.
- The edits to a voxel world, which are kept apart (see [VOXELS.md](VOXELS.md)).