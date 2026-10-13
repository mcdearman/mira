# Flat games

mira draws in three dimensions, and a flat game is one seen from where depth does not show.
`examples/tail_lights` is one: a road crossed from above, in pixels.

```sh
cargo run --release --example tail_lights
```

## A flat picture

```rust
// Straight down, the far side to the top of the picture, nothing smaller for being further.
let camera = Camera { orthographic_height: Some(13.0), far: 60.0, ..Default::default() };
commands.spawn((Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z), camera));

app.insert_resource(ViewMode::Unlit)                                  // each surface in its own colour
    .insert_resource(RayTracingSettings { enabled: false, ..Default::default() })
    .insert_resource(PostProcess { taa: false, bloom: 0.0, vignette: 0.0, grain: 0.0, sharpen: 0.0, ..Default::default() });
```

`orthographic_height` is how many metres of the world the picture is tall. With
`ViewMode::Unlit` nothing is lit or shadowed, so a picture's colours are the colours on the
screen.

## Pictures on the ground

A sprite is a picture on a square lying flat: `Mesh::plane(1.0)`, scaled to the size it is
to be, with the picture as its material's colour.

```rust
let square = meshes.add(Mesh::plane(1.0));
let picture = images.add(Image::from_rgba(16, 16, pixels, true).pixelated());
commands.spawn((
    Transform::from_xyz(x, 0.3, z).with_rotation(Quat::from_rotation_y(FRAC_PI_2)).with_scale(Vec3::new(deep, 1.0, wide)),
    Mesh3d(square),
    Material { base_color_texture: Some(picture), alpha_cutoff: Some(0.5), roughness: 1.0, ..Default::default() },
));
```

- **What is over what** is how high each is above the ground: a car at 0.2 is over the road
  at 0 and under a tree at 0.5.
- **See-through pixels** are cut away by `alpha_cutoff`.
- **A quarter turn**: the plane has a picture's top along its own x, so it is turned a
  quarter round to stand the way a picture is looked at from above, and its scale is depth
  then width.
- **Sprites of one picture are one draw**, however many: they are instances like any other.

## Pixel art

`Image::pixelated()` has a picture drawn with its pixels square and sharp however large it
is on the screen, where a picture is otherwise smoothed between them. A sixteen-pixel sprite
is then a sixteen-pixel image.

A picture that changes, a walk or a splat, is a picture for each moment, and the entity's
material is given the one for now.

## The rest is the engine as it is

Keys by name ([INPUT.md](INPUT.md)), sound ([AUDIO.md](AUDIO.md)), saved games
([SAVES.md](SAVES.md)), physics, finding the way ([NAVIGATION.md](NAVIGATION.md)).

## What there is not yet

- Text: `tail_lights` writes its score with a font of its own, a quad a letter.
- Many pictures in one image (a sprite sheet), and tiles drawn as one mesh.
- A flat world's own physics and lighting.