# Sound

```sh
cargo run --example sound             # a few notes, and a hum that goes round you
cargo run --example sound -- quiet    # the same with the volume at nothing
```

## A sound

A `Sound` is read from a file or made, and kept in `Assets<Sound>`:

```rust
let shot = sounds.add(Sound::load("res/shot.wav")?);     // WAV, or Ogg Vorbis (.ogg)
let beep = sounds.add(Sound::tone(440.0, 0.2));          // a plain tone, for trying things
```

WAV files of whole numbers (8, 16, 24 or 32 bits) and of 32-bit floats are read, with any
number of channels at any rate. A sound of one channel is heard in both ears; of two, each
ear has its own.

## Playing one

From an entity, for as long as the entity has it, heard from where the entity is:

```rust
commands.spawn((
    Transform::from_xyz(4.0, 1.0, 0.0),
    AudioSource::new(hum).looping().heard_within(12.0).with_volume(0.6),
));
```

| | |
|---|---|
| `volume`, `pitch` | how loud and how fast, 1 as it was recorded; they may be changed while it plays |
| `looping` | whether it begins again when it ends |
| `range` (`heard_within`) | how far off it is heard, in metres: quieter with distance, silent there, louder in the ear on its side. Nought, the default, is heard the same wherever the listener is |

It stops when the `AudioSource` is taken away or the entity despawned. One that does not
loop plays once; `is_finished()` then says so.

At once, for a sound that belongs to nothing, through the `Audio` resource:

```rust
fn click(audio: Res<Audio>, sounds: Res<Assets<Sound>>, ...) {
    if let Some(sound) = sounds.get(beep) {
        let voice = audio.play_with(sound, Playback { volume: 0.5, pan: -0.3, ..Default::default() });
    }
}
```

`play` and `play_with` answer a `VoiceId`, with which the sound may be stopped, paused, and
made louder, quieter, higher or lower while it plays. A sound that has ended is simply no
longer there.

## Who hears

The entity with an `AudioListener`, where it is and the way it faces. Where none has one,
the active camera hears.

## Music

Music is long, and is read from its file as it plays, a second ahead:

```rust
let music = audio.play_music("res/theme.ogg", Playback { looping: true, volume: 0.4, ..Default::default() })?;
```

Ogg Vorbis only. It is changed and stopped as any sound is.

## How loud everything is

`audio.set_master_volume(0.5)`. `audio.set_held(true)` holds every sound where it is, silent,
until it is let go again: the engine does so itself while an app is put away on a phone.

## Where there is no one to hear

`MIRA_AUDIO=0`, or a machine with no sound output, opens nothing: `audio.is_heard()` is
false. Sounds still begin, last as long and end as they would, so a game behaves the same
in a test as it does out loud.

## What there is not yet

- Sounds loaded by name in the background, as models and pictures are, and sources saved in
  scenes.
- Buses (music apart from effects), effects such as echo, and sound that bends round or is
  muffled by what stands in the way.
- Formats other than WAV and Ogg Vorbis.