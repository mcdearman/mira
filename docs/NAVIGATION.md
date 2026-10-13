# Finding the way

## Somewhere to walk

A `NavGrid` is a level as somewhere to walk: square cells laid over the ground, each with
the height of the ground there, or nothing where there is no standing. Insert one as a
resource.

From a level made of colliders, once physics has them:

```rust
let grid = NavGrid::from_physics(&physics, Vec3::new(-40.0, -2.0, -40.0), Vec3::new(40.0, 10.0, 40.0), 0.5, Walker::default());
commands.insert_resource(grid);
```

A ray is sent down the middle of each cell. There is standing where it meets ground no
steeper than the walker stands on (`Walker::slope`), with the walker's `height` clear above
it. Things that move are not ground and are not in the way.

From a world of blocks, a cell to a block:

```rust
let grid = NavGrid::from_voxels(&world, &registry, IVec3::new(-64, 0, -64), IVec3::new(64, 80, 64), 2);
```

In each column the ground is the highest block with two blocks of air above it.

Or by hand: `NavGrid::flat(origin, cell, across, along, height)` and `grid.set(cell, None)`
where a wall is.

## The way

```rust
if let Some(way) = grid.path(from, to) {
    // points to walk through in turn, ending at `to`
}
```

It goes round what is in the way, never up or down a step taller than `Walker::step`
between one cell and the next, never across a corner, and is the shortest there is over the
cells, with the corners that need not be turned left out. `None` where there is no way or
either place is off the grid. A goal with no standing, inside a wall, is taken to mean the
nearest standing to it.

## Walking it

```rust
commands.spawn((Transform::from_xyz(2.0, 0.0, 2.0), Mesh3d(body), Navigator::new(3.5)));

fn chase(player: Query<&Transform, With<Player>>, mut hunters: Query<&mut Navigator>) { /* … */
    hunter.go_to(player_at);
}
```

A `Navigator` finds the way when its goal changes and walks its entity along it at its
`speed`, turning to face the way it goes. An entity with a `CharacterController` is steered,
so it is stopped by walls and carried by the ground as a player is; any other is moved
itself and kept on the grid's ground. `arrived` says when it is there, `stuck` when there is
no way, and `way()` gives the points still ahead.

## What it is and is not

- One height to a cell: ground, slopes, stairs and floors seen from above. Not a bridge and
  the road under it at once, and nothing is walked under: a beam's top is the ground there.
- Made when asked for. A level that changes makes it again, or sets the cells that changed.
- Walkers do not keep out of each other's way.
- Behaviour trees, to decide where to go, are not here yet.