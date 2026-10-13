# Particles

Smoke, dust, spray, sparks and flashes are particles: soft puffs that face the camera.

A particle is born with a velocity, slows by its `drag`, rises or sinks by its `rise`, is
carried by the wind as it slows, spreads from its `size` to `spread` times that, and thins
away over its `life`. Smoke is lit by the sun and the sky, shadowed and hazed as the scene
is. A particle with `flash` set is light of its own instead, added to the picture: a muzzle
flash, a spark.

## Letting them out

Steadily, from an entity: a chimney, a fire, a fountain.

```rust
let smoke = Particle { velocity: Vec3::Y * 0.4, life: 16.0, size: 0.25, spread: 7.0, rise: 0.9, opacity: 0.35, ..Default::default() };
commands.spawn((Transform::from_xyz(3.0, 9.0, 0.0), ParticleEmitter::new(4.0, smoke).with_scatter(0.2)));
```

`ParticleEmitter::new(rate, particle)` lets out `rate` a second, each like `particle` but
turned and shaped at random, from where the entity is; the particle's velocity is turned as
the entity is, and `scatter` throws each a little aside.

All at once, from a system: a gun's discharge, earth thrown up by a shot.

```rust
fn fire(mut particles: ResMut<Particles>) {
    for _ in 0..12 {
        let aside = Vec3::new(particles.random() - 0.5, particles.random(), particles.random() - 0.5);
        let mut puff = particles.puff(muzzle, forward * 8.0 + aside);
        puff.life = 14.0;
        particles.emit(puff);
    }
}
```

`Particles::puff` makes a pale puff turned and shaped at random, to be changed and let out
with `emit`. `Particles::random` gives a number from nought up to one, for making them
differ. `Particles::wind` is the breeze that carries them off.

## What there is and is not

- Sixteen thousand at once (`MAX_PARTICLES`); those let out past that are lost.
- They are moved on the processor and drawn in one instanced draw, furthest first, after
  everything opaque.
- They cast no shadows, are not in traced reflections, and leave no trace in the blend over
  time of how they moved.
- An emitter is not saved in a scene yet.