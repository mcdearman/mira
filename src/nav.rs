//! Finding the way.
//!
//! A [`NavGrid`] is a level as somewhere to walk: square cells laid over the ground, each
//! with the height of the ground there, or nothing where there is no standing. It is made
//! from what a level is made of, its colliders ([`NavGrid::from_physics`]) or its blocks
//! ([`NavGrid::from_voxels`]), or cell by cell.
//!
//! [`NavGrid::path`] gives the way from one place to another over it: a few points to walk
//! through in turn, round what is in the way and never up a step too high. A [`Navigator`]
//! on an entity walks it there.
//!
//! The grid is one height to a cell, so it is ground, slopes, stairs and the floors of
//! buildings seen from above: not a bridge and the road under it at once.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::{IVec3, Vec2, Vec3};

use crate::{
    ecs::{Component, Query, Res},
    physics::{CharacterController, PhysicsWorld},
    time::Time,
    transform::Transform,
    voxel::{BlockRegistry, VoxelWorld},
};

/// Who is to walk: how much room they need, and what they can climb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walker {
    /// How tall, in metres: there must be that much clear above the ground.
    pub height: f32,
    /// The tallest step up or down between one cell and the next.
    pub step: f32,
    /// The steepest ground stood on, in radians from level.
    pub slope: f32,
}

impl Default for Walker {
    fn default() -> Self {
        Self { height: 1.8, step: 0.45, slope: 0.8 }
    }
}

/// A level as somewhere to walk.
#[derive(Clone, Debug)]
pub struct NavGrid {
    /// The corner of the first cell, across (x) and along (z).
    origin: Vec2,
    /// How wide a cell is, in metres.
    cell: f32,
    across: usize,
    along: usize,
    /// The height of the ground in each cell, or not a number where there is no standing.
    heights: Vec<f32>,
    /// The tallest step between one cell and the next.
    step: f32,
}

/// A cell of the grid: how many across and how many along.
pub type Cell = (usize, usize);

impl NavGrid {
    /// A grid with nowhere to stand yet. `origin` is the corner of the first cell.
    pub fn new(origin: Vec2, cell: f32, across: usize, along: usize) -> Self {
        Self { origin, cell: cell.max(0.01), across, along, heights: vec![f32::NAN; across * along], step: Walker::default().step }
    }

    /// Level ground everywhere, at a height: to cut holes in, or for a game on a plain.
    pub fn flat(origin: Vec2, cell: f32, across: usize, along: usize, height: f32) -> Self {
        Self { heights: vec![height; across * along], ..Self::new(origin, cell, across, along) }
    }

    /// The tallest step between one cell and the next that a path will take.
    pub fn with_step(mut self, step: f32) -> Self {
        self.step = step.max(0.0);
        self
    }

    pub fn size(&self) -> Cell {
        (self.across, self.along)
    }

    pub fn cell_size(&self) -> f32 {
        self.cell
    }

    /// Says what the ground is in a cell: its height, or none where there is no standing.
    pub fn set(&mut self, (x, z): Cell, height: Option<f32>) {
        if x < self.across && z < self.along {
            self.heights[z * self.across + x] = height.unwrap_or(f32::NAN);
        }
    }

    /// The height of the ground in a cell, if there is standing there.
    pub fn height(&self, (x, z): Cell) -> Option<f32> {
        (x < self.across && z < self.along).then(|| self.heights[z * self.across + x]).filter(|height| !height.is_nan())
    }

    /// The cell a place is over, if it is over the grid.
    pub fn cell_at(&self, place: Vec3) -> Option<Cell> {
        let at = (Vec2::new(place.x, place.z) - self.origin) / self.cell;
        (at.x >= 0.0 && at.y >= 0.0 && (at.x as usize) < self.across && (at.y as usize) < self.along).then_some((at.x as usize, at.y as usize))
    }

    /// The middle of a cell, on its ground; at nought where there is none.
    pub fn middle(&self, (x, z): Cell) -> Vec3 {
        let at = self.origin + (Vec2::new(x as f32, z as f32) + 0.5) * self.cell;
        Vec3::new(at.x, self.height((x, z)).unwrap_or(0.0), at.y)
    }

    /// The ground of a level made of colliders, between two corners: a ray is sent down the
    /// middle of each cell from the upper corner's height, and there is standing where it
    /// meets ground no steeper than the walker stands on, with the walker's height clear
    /// above it. Things that move are not ground.
    pub fn from_physics(physics: &PhysicsWorld, low: Vec3, high: Vec3, cell: f32, walker: Walker) -> Self {
        let cell = cell.max(0.01);
        let (across, along) = ((((high.x - low.x) / cell).ceil().max(0.0)) as usize, (((high.z - low.z) / cell).ceil().max(0.0)) as usize);
        let mut grid = Self::new(Vec2::new(low.x, low.z), cell, across, along).with_step(walker.step);
        let scenery = |_, moving: bool| !moving;
        for z in 0..along {
            for x in 0..across {
                let over = grid.middle((x, z));
                let Some(ground) = physics.raycast_where(Vec3::new(over.x, high.y, over.z), Vec3::NEG_Y, high.y - low.y, scenery) else { continue };
                if ground.normal.y < walker.slope.cos() {
                    continue;
                }
                // Room to stand: nothing between a little above the ground and head height.
                let clear = physics.raycast_where(ground.point + Vec3::Y * 0.1, Vec3::Y, walker.height - 0.1, scenery).is_none();
                if clear {
                    grid.set((x, z), Some(ground.point.y));
                }
            }
        }
        grid
    }

    /// The ground of a world of blocks, between two corners in blocks, a cell to a block:
    /// in each column, the highest block that is solid with `clear` blocks of air above it
    /// (two, for a walker two blocks tall). A step is one block.
    pub fn from_voxels(world: &VoxelWorld, registry: &BlockRegistry, low: IVec3, high: IVec3, clear: i32) -> Self {
        let (across, along) = ((high.x - low.x + 1).max(0) as usize, (high.z - low.z + 1).max(0) as usize);
        let mut grid = Self::new(Vec2::new(low.x as f32, low.z as f32), 1.0, across, along).with_step(1.05);
        for z in 0..along {
            for x in 0..across {
                let column = IVec3::new(low.x + x as i32, 0, low.z + z as i32);
                let standing = (low.y..=high.y).rev().find(|y| {
                    let at = column.with_y(*y);
                    world.is_solid(registry, at) && (1..=clear).all(|up| !world.is_solid(registry, at + IVec3::Y * up))
                });
                if let Some(y) = standing {
                    grid.set((x, z), Some(y as f32 + 1.0));
                }
            }
        }
        grid
    }

    /// Whether one may go from a cell straight to one beside it, or cornerwise: both must be
    /// ground, with no more than a step between them, and going cornerwise the two cells it
    /// passes between must be ground as well, so that a corner is not cut.
    fn passable(&self, from: Cell, to: Cell) -> bool {
        let (Some(a), Some(b)) = (self.height(from), self.height(to)) else { return false };
        if (a - b).abs() > self.step {
            return false;
        }
        if from.0 != to.0 && from.1 != to.1 {
            let reachable = |beside: Cell| self.height(beside).is_some_and(|h| (h - a).abs() <= self.step && (h - b).abs() <= self.step);
            return reachable((to.0, from.1)) && reachable((from.0, to.1));
        }
        true
    }

    fn neighbours(&self, (x, z): Cell) -> impl Iterator<Item = (Cell, f32)> + '_ {
        const AROUND: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
        AROUND.iter().filter_map(move |(dx, dz)| {
            let (nx, nz) = (x as i32 + dx, z as i32 + dz);
            let to = (nx >= 0 && nz >= 0 && (nx as usize) < self.across && (nz as usize) < self.along).then_some((nx as usize, nz as usize))?;
            self.passable((x, z), to).then_some((to, if *dx != 0 && *dz != 0 { std::f32::consts::SQRT_2 } else { 1.0 }))
        })
    }

    /// The nearest cell with standing to one that may have none, looking as far as `reach`
    /// cells off. A goal inside a wall is taken to mean beside it.
    fn standing_near(&self, (x, z): Cell, reach: i32) -> Option<Cell> {
        (0..=reach)
            .flat_map(|ring| (-ring..=ring).flat_map(move |dz| (-ring..=ring).filter(move |dx| dx.abs().max(dz.abs()) == ring).map(move |dx| (dx, dz))))
            .map(|(dx, dz)| (x as i32 + dx, z as i32 + dz))
            .filter(|(nx, nz)| *nx >= 0 && *nz >= 0)
            .map(|(nx, nz)| (nx as usize, nz as usize))
            .find(|cell| self.height(*cell).is_some())
    }

    /// Whether one may walk straight from the middle of one cell to the middle of another:
    /// every cell the line passes over is gone through in turn.
    fn straight(&self, from: Cell, to: Cell) -> bool {
        let (a, b) = (Vec2::new(from.0 as f32, from.1 as f32), Vec2::new(to.0 as f32, to.1 as f32));
        let steps = ((b - a).abs().max_element() * 3.0).ceil().max(1.0) as usize;
        let mut last = from;
        for step in 1..=steps {
            let at = a + (b - a) * (step as f32 / steps as f32) + 0.5;
            let now = (at.x as usize, at.y as usize);
            if now != last {
                if !self.passable(last, now) {
                    return false;
                }
                last = now;
            }
        }
        true
    }

    /// The cells to go through from one to another, each beside the last: the shortest way
    /// there is, or none where there is no way.
    pub fn cells(&self, from: Cell, to: Cell) -> Option<Vec<Cell>> {
        self.height(from)?;
        self.height(to)?;
        #[derive(PartialEq)]
        struct Reached {
            guess: f32,
            cell: Cell,
        }
        impl Eq for Reached {}
        impl Ord for Reached {
            // The least guess first out of a heap that gives the greatest.
            fn cmp(&self, other: &Self) -> Ordering {
                other.guess.total_cmp(&self.guess).then_with(|| self.cell.cmp(&other.cell))
            }
        }
        impl PartialOrd for Reached {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
        // How far it is at the least from a cell to the goal: straight and cornerwise steps.
        let at_least = |(x, z): Cell| {
            let (dx, dz) = (x.abs_diff(to.0) as f32, z.abs_diff(to.1) as f32);
            dx.max(dz) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dz)
        };
        let index = |(x, z): Cell| z * self.across + x;
        let mut cost = vec![f32::INFINITY; self.heights.len()];
        let mut came: Vec<Option<Cell>> = vec![None; self.heights.len()];
        let mut open = BinaryHeap::new();
        cost[index(from)] = 0.0;
        open.push(Reached { guess: at_least(from), cell: from });
        while let Some(Reached { guess, cell }) = open.pop() {
            if cell == to {
                let mut way = vec![to];
                while let Some(before) = came[index(*way.last().unwrap())] {
                    way.push(before);
                }
                way.reverse();
                return Some(way);
            }
            // Reached again by a shorter way since this was put in: out of date.
            if guess > cost[index(cell)] + at_least(cell) + 1e-4 {
                continue;
            }
            for (next, length) in self.neighbours(cell) {
                let reached = cost[index(cell)] + length;
                if reached < cost[index(next)] {
                    cost[index(next)] = reached;
                    came[index(next)] = Some(cell);
                    open.push(Reached { guess: reached + at_least(next), cell: next });
                }
            }
        }
        None
    }

    /// The way from one place to another: the points to walk through in turn, ending at the
    /// goal, with the corners that need not be turned left out. None where there is no way,
    /// or where either place is off the grid. A place with no standing is taken to mean the
    /// nearest standing to it.
    pub fn path(&self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let start = self.standing_near(self.cell_at(from)?, 2)?;
        let goal_cell = self.cell_at(to)?;
        let goal = self.standing_near(goal_cell, 4)?;
        let cells = self.cells(start, goal)?;
        // From each point kept, straight to the furthest cell that can be walked straight to.
        let mut kept = vec![cells[0]];
        let mut at = 0;
        while at + 1 < cells.len() {
            let mut furthest = at + 1;
            for ahead in (at + 2..cells.len()).rev() {
                if self.straight(cells[at], cells[ahead]) {
                    furthest = ahead;
                    break;
                }
            }
            kept.push(cells[furthest]);
            at = furthest;
        }
        let mut way: Vec<Vec3> = kept.into_iter().skip(1).map(|cell| self.middle(cell)).collect();
        // The goal itself, and not the middle of its cell, where it is on the ground.
        if goal == goal_cell {
            let ground = self.height(goal).unwrap_or(to.y);
            match way.last_mut() {
                Some(last) => *last = Vec3::new(to.x, ground, to.z),
                None => way.push(Vec3::new(to.x, ground, to.z)),
            }
        }
        Some(way)
    }
}

/// Walks its entity to where it is told, over the [`NavGrid`] there is (a resource).
///
/// Set `goal`; the way is found, and each frame the entity is moved along it at `speed`. An
/// entity with a [`CharacterController`] is steered, and so is stopped by walls and carried
/// by the ground as it is when a player moves it; any other is moved itself and kept on the
/// grid's ground. `arrived` says when it is there, and `stuck` when there is no way.
#[derive(Clone, Debug)]
pub struct Navigator {
    /// Where to go. None is nowhere: it stands.
    pub goal: Option<Vec3>,
    /// Metres a second.
    pub speed: f32,
    /// How near a point of the way is near enough.
    pub near: f32,
    pub arrived: bool,
    /// There is no way to the goal, or it is off the grid.
    pub stuck: bool,
    /// Whether to turn to face the way it goes.
    pub turns: bool,
    way: Vec<Vec3>,
    next: usize,
    found_for: Option<Vec3>,
}

impl Component for Navigator {}

impl Navigator {
    pub fn new(speed: f32) -> Self {
        Self { goal: None, speed, near: 0.25, arrived: false, stuck: false, turns: true, way: Vec::new(), next: 0, found_for: None }
    }

    /// Sends it somewhere.
    pub fn go_to(&mut self, goal: Vec3) {
        self.goal = Some(goal);
    }

    /// Stops it where it is.
    pub fn stop(&mut self) {
        self.goal = None;
    }

    /// The points still ahead of it.
    pub fn way(&self) -> &[Vec3] {
        &self.way[self.next.min(self.way.len())..]
    }
}

/// Walks every [`Navigator`] along its way, once there is a [`NavGrid`]. Part of
/// `DefaultPlugins`.
pub struct NavPlugin;

impl crate::app::Plugin for NavPlugin {
    fn build(&self, app: &mut crate::app::App) {
        app.add_systems(crate::app::Stage::Update, navigate);
    }
}

/// Finds the way for each navigator whose goal has changed, and moves every one along its
/// way.
pub(crate) fn navigate(time: Res<Time>, grid: Option<Res<NavGrid>>, mut walkers: Query<(&mut Navigator, &mut Transform, Option<&mut CharacterController>)>) {
    let Some(grid) = grid else { return };
    let dt = time.delta_secs();
    for (mut nav, mut at, mut controller) in &mut walkers {
        if nav.found_for != nav.goal {
            nav.found_for = nav.goal;
            let way = nav.goal.and_then(|goal| grid.path(at.translation, goal));
            (nav.stuck, nav.arrived, nav.next) = (nav.goal.is_some() && way.is_none(), false, 0);
            nav.way = way.unwrap_or_default();
        }
        let mut toward = Vec3::ZERO;
        if nav.goal.is_some() && !nav.stuck {
            // Past each point it has come near enough to.
            let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
            while nav.next < nav.way.len() && flat(nav.way[nav.next], at.translation) <= nav.near {
                nav.next += 1;
            }
            match nav.way.get(nav.next) {
                Some(point) => toward = Vec3::new(point.x - at.translation.x, 0.0, point.z - at.translation.z).normalize_or_zero(),
                None => nav.arrived = true,
            }
        }
        match controller.as_deref_mut() {
            Some(controller) => controller.desired_velocity = toward * nav.speed,
            None if toward != Vec3::ZERO => {
                // No further than the point it is making for, so that it does not overshoot.
                let point = nav.way[nav.next];
                let left = Vec2::new(point.x - at.translation.x, point.z - at.translation.z).length();
                at.translation += toward * (nav.speed * dt).min(left);
                if let Some(ground) = grid.cell_at(at.translation).and_then(|cell| grid.height(cell)) {
                    at.translation.y = ground;
                }
            }
            None => {}
        }
        if nav.turns && toward != Vec3::ZERO {
            at.rotation = glam::Quat::from_rotation_y(f32::atan2(-toward.x, -toward.z));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain twelve cells square, a metre a cell, with a wall across it at z = 6 from the
    /// left edge to x = 8: the way round is by the right.
    fn walled() -> NavGrid {
        let mut grid = NavGrid::flat(Vec2::ZERO, 1.0, 12, 12, 0.0);
        for x in 0..9 {
            grid.set((x, 6), None);
        }
        grid
    }

    fn length(way: &[Vec3], from: Vec3) -> f32 {
        std::iter::once(&from).chain(way).zip(way).map(|(a, b)| a.distance(*b)).sum()
    }

    #[test]
    fn the_way_goes_round_what_is_in_it_and_is_no_longer_than_it_need_be() {
        let grid = walled();
        let (from, to) = (Vec3::new(2.5, 0.0, 2.5), Vec3::new(2.5, 0.0, 10.5));
        let way = grid.path(from, to).unwrap();
        assert_eq!(*way.last().unwrap(), to, "it ends at the goal itself");
        // Round the end of the wall, in a few straight legs and not a cell at a time.
        assert!(way.len() <= 4, "{way:?}");
        assert!(way.iter().any(|point| point.x > 9.0), "{way:?}");
        // Every leg can be walked: no point of it is over the wall.
        let mut at = from;
        for point in &way {
            for step in 0..=20 {
                let on = at.lerp(*point, step as f32 / 20.0);
                assert!(grid.cell_at(on).and_then(|cell| grid.height(cell)).is_some(), "over the wall at {on}");
            }
            at = *point;
        }
        // And it is near the shortest there is: to the wall's end and back.
        let shortest = from.distance(Vec3::new(9.5, 0.0, 6.0)) + Vec3::new(9.5, 0.0, 6.0).distance(to);
        assert!(length(&way, from) < shortest * 1.15, "{} against {shortest}", length(&way, from));
        // Across open ground the way is the one straight leg.
        assert_eq!(grid.path(from, Vec3::new(10.5, 0.0, 3.5)).unwrap().len(), 1);
    }

    #[test]
    fn where_there_is_no_way_there_is_none_and_a_goal_in_a_wall_means_beside_it() {
        let mut grid = walled();
        for x in 9..12 {
            grid.set((x, 6), None);
        }
        let (from, beyond) = (Vec3::new(2.5, 0.0, 2.5), Vec3::new(2.5, 0.0, 10.5));
        assert!(grid.path(from, beyond).is_none(), "the wall goes right across");
        assert!(grid.path(from, Vec3::new(50.0, 0.0, 2.0)).is_none(), "off the grid");
        // Into the wall itself: as near as there is standing, on this side.
        let way = grid.path(from, Vec3::new(4.5, 0.0, 6.5)).unwrap();
        let end = *way.last().unwrap();
        assert!(end.z < 6.0 && (end.x - 4.5).abs() <= 1.0, "{end}");
        // Standing where one is: there, with nothing to walk.
        assert!(grid.path(from, from).unwrap().iter().all(|point| point.distance(from) < 1e-4));
    }

    #[test]
    fn a_step_too_high_is_not_taken_and_stairs_are() {
        // A ledge two metres up along the far side, with stairs up to it at the right.
        let mut grid = NavGrid::flat(Vec2::ZERO, 1.0, 8, 8, 0.0).with_step(0.5);
        for x in 0..8 {
            for z in 5..8 {
                grid.set((x, z), Some(2.0));
            }
        }
        for (z, height) in [(1, 0.4), (2, 0.8), (3, 1.2), (4, 1.6)] {
            grid.set((7, z), Some(height));
        }
        let way = grid.path(Vec3::new(1.5, 0.0, 3.5), Vec3::new(1.5, 2.0, 6.5)).unwrap();
        assert!(way.iter().any(|point| point.x > 7.0), "up the stairs, not up the face: {way:?}");
        assert!((way.last().unwrap().y - 2.0).abs() < 1e-5);
        // Without the stairs there is no way up.
        grid.set((7, 3), None);
        assert!(grid.path(Vec3::new(1.5, 0.0, 3.5), Vec3::new(1.5, 2.0, 6.5)).is_none());
        // A corner is not cut: between two cells that touch only at a corner there is no way.
        let mut corner = NavGrid::new(Vec2::ZERO, 1.0, 2, 2);
        corner.set((0, 0), Some(0.0));
        corner.set((1, 1), Some(0.0));
        assert!(corner.cells((0, 0), (1, 1)).is_none());
    }

    #[test]
    fn a_grid_is_made_from_a_world_of_blocks() {
        use crate::voxel::{Block, BlockId, ChunkData};
        let mut registry = BlockRegistry::default();
        let stone = registry.register(Block::uniform("stone", 0));
        let mut world = VoxelWorld::default();
        world.insert_chunk(IVec3::ZERO, ChunkData::default());
        // A floor at y = 0, a pillar on it, a low roof over one corner, and a hole.
        for x in 0..8 {
            for z in 0..8 {
                world.set_block(IVec3::new(x, 0, z), stone);
            }
        }
        world.set_block(IVec3::new(3, 1, 3), stone);
        world.set_block(IVec3::new(3, 2, 3), stone);
        world.set_block(IVec3::new(3, 3, 3), stone);
        world.set_block(IVec3::new(6, 2, 6), stone);
        world.set_block(IVec3::new(1, 0, 6), BlockId::AIR);
        let grid = NavGrid::from_voxels(&world, &registry, IVec3::new(0, 0, 0), IVec3::new(7, 6, 7), 2);
        assert_eq!(grid.size(), (8, 8));
        assert_eq!(grid.height((0, 0)), Some(1.0), "on top of the floor");
        assert_eq!(grid.height((3, 3)), Some(4.0), "on top of the pillar, which is too high to step onto");
        assert_eq!(grid.height((6, 6)), Some(3.0), "on the roof: under it there is not room to stand");
        assert_eq!(grid.height((1, 6)), None, "nothing to stand on over the hole");
        let way = grid.path(Vec3::new(1.5, 1.0, 3.5), Vec3::new(5.5, 1.0, 3.5)).unwrap();
        assert!(way.iter().all(|point| (point.y - 1.0).abs() < 1e-5) && way.len() >= 2, "round the pillar: {way:?}");
    }

    #[test]
    fn a_navigator_walks_its_entity_there_and_says_when_it_cannot() {
        use crate::app::{App, Stage};
        let mut app = App::new();
        app.add_plugins(crate::time::TimePlugin).add_plugins(crate::transform::TransformPlugin);
        app.world.insert_resource(walled());
        app.add_systems(Stage::Update, navigate);
        app.world.resource_mut::<Time>().set_fixed_step(Some(std::time::Duration::from_millis(50)));
        let walker = app.world.spawn((Transform::from_xyz(2.5, 0.0, 2.5), Navigator::new(4.0)));
        let goal = Vec3::new(2.5, 0.0, 10.5);
        app.world.get_mut::<Navigator>(walker).unwrap().go_to(goal);
        let mut frames = 0;
        while !app.world.get::<Navigator>(walker).unwrap().arrived && frames < 400 {
            app.update();
            frames += 1;
            let at = app.world.get::<Transform>(walker).unwrap().translation;
            assert!(app.world.resource::<NavGrid>().cell_at(at).and_then(|cell| app.world.resource::<NavGrid>().height(cell)).is_some(), "walked into the wall at {at}");
        }
        let at = app.world.get::<Transform>(walker).unwrap().translation;
        assert!(at.distance(goal) < 0.3, "ended at {at} after {frames} frames");
        // About as long as the way is at its speed, and not a frame at a cell.
        assert!(frames < 120, "{frames} frames");
        // Sent where there is no way, it stands and says so.
        let nav = app.world.get_mut::<Navigator>(walker).unwrap();
        nav.go_to(Vec3::new(500.0, 0.0, 0.0));
        app.update();
        let nav = app.world.get::<Navigator>(walker).unwrap();
        assert!(nav.stuck && !nav.arrived && nav.way().is_empty());
        assert!(app.world.get::<Transform>(walker).unwrap().translation.distance(at) < 1e-4);
    }
}