# Lights

## The sun

A `DirectionalLight` is the sun: one direction for the whole scene, with shadows in four
cascades, and the sky that goes with where it stands.

## Lamps, fires and torches

A `PointLight` shines all round from where its entity is, and a `SpotLight` in a cone the way
its entity faces:

```rust
commands.spawn((
    Transform::from_xyz(2.0, 3.0, 0.0),
    PointLight { color: Color::rgb(1.0, 0.6, 0.25), intensity: 40.0, range: 10.0 },
));
commands.spawn((
    Transform::from_xyz(0.0, 4.0, 0.0).looking_at(Vec3::ZERO, Vec3::X),
    SpotLight { intensity: 60.0, range: 15.0, inner: 0.3, outer: 0.45, shadows: true, ..Default::default() },
));
```

| | |
|---|---|
| `color`, `intensity` | how bright a metre away, in the sun's units |
| `range` | how far it reaches, in metres: it weakens with the square of the distance and is brought to nothing there |
| `inner`, `outer` | a spot light's cone, as half angles in radians: full inside the first, none outside the second |
| `shadows` | a spot light's: whether what stands in its light casts a shadow |

Both are components like any other: saved in scenes, set in the inspector, moved by moving
their entity.

They light a surface as the sun does, colour and shine.

## Shadows

A spot light with `shadows: true` is hidden by what stands in its light. It has a shadow
map of its own, 1,024 across, drawn each frame from where the light is with what casts into
its cone. There are eight such maps: of the lights that ask, the eight nearest the camera
have one, and the rest shine through things until the camera comes nearer. Eight of them
over a field of 100,000 things cost the graphics card 0.2 ms a frame.

A point light casts no shadows. Nor does a spot light where the graphics card cannot decide
for itself what each view sees (see [PERFORMANCE.md](PERFORMANCE.md)), since that is how what
casts into a light's map is found.

## Many of them

A scene may hold thousands, because a surface is lit only by the few that reach it. The
view is cut into cells (16 across, 9 down and 24 through its depth, each slice as deep as it
is far away), and every frame each light is put in the cells its sphere touches. A fragment
looks up its own cell and goes through that cell's lights alone. So what a light costs is
what it lights: give a lamp the range it needs and no more.

What that leaves out:

- Lights further than 250 m from the camera light nothing, and a frame takes 4,096 at most.
- They are found by where a surface is in the view, so they do not show in a traced
  reflection of something lit by them, nor in the light probes.

See [PERFORMANCE.md](PERFORMANCE.md) for what a thousand of them cost.
