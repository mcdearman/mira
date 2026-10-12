//! Lights with a place: lamps, fires, torches.
//!
//! A [`PointLight`] shines all round and a [`SpotLight`] in a cone, each as far as its
//! `range`. They cast no shadows.
//!
//! A scene may hold a great many, since a surface is lit only by the few that reach it. The
//! view is cut into cells, a grid across the picture and slices through its depth, and each
//! frame every light is put in the cells its sphere touches. A fragment looks up its own cell
//! and goes through that cell's lights alone, however many there are elsewhere.

use glam::{Mat4, Vec3};

use crate::reflect::Reflect;

use super::Color;

/// A light that shines all round from where its entity is.
#[derive(Clone, Copy, Debug, Reflect)]
#[reflect(name = "mira.PointLight", default)]
pub struct PointLight {
    pub color: Color,
    /// How bright, in the sun's units, a metre away.
    pub intensity: f32,
    /// How far it reaches, in metres: it fades to nothing there.
    pub range: f32,
}

impl crate::ecs::Component for PointLight {}

impl Default for PointLight {
    fn default() -> Self {
        Self { color: Color::WHITE, intensity: 4.0, range: 8.0 }
    }
}

/// A light that shines in a cone, the way its entity faces.
#[derive(Clone, Copy, Debug, Reflect)]
#[reflect(name = "mira.SpotLight", default)]
pub struct SpotLight {
    pub color: Color,
    /// How bright, in the sun's units, a metre away along its middle.
    pub intensity: f32,
    pub range: f32,
    /// The cone's half angle, in radians: full brightness inside `inner`, none outside
    /// `outer`.
    pub inner: f32,
    pub outer: f32,
}

impl crate::ecs::Component for SpotLight {}

impl Default for SpotLight {
    fn default() -> Self {
        Self { color: Color::WHITE, intensity: 8.0, range: 12.0, inner: 0.35, outer: 0.5 }
    }
}

/// A light as a frame gathers it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalLight {
    pub position: Vec3,
    pub range: f32,
    /// Colour times brightness.
    pub light: Vec3,
    /// The way a spot light faces, and the cosines of its cone's angles. A point light has
    /// cosines below -1, which every direction is inside.
    pub direction: Vec3,
    pub cos_inner: f32,
    pub cos_outer: f32,
}

impl LocalLight {
    pub fn point(position: Vec3, light: &PointLight) -> Self {
        Self {
            position,
            range: light.range.max(0.0),
            light: light.color.to_vec3() * light.intensity,
            direction: Vec3::NEG_Z,
            cos_inner: -2.0,
            cos_outer: -3.0,
        }
    }

    pub fn spot(position: Vec3, direction: Vec3, light: &SpotLight) -> Self {
        let outer = light.outer.clamp(0.01, 1.55);
        let inner = light.inner.clamp(0.0, outer - 0.005);
        Self {
            position,
            range: light.range.max(0.0),
            light: light.color.to_vec3() * light.intensity,
            direction: direction.normalize_or(Vec3::NEG_Z),
            cos_inner: inner.cos(),
            cos_outer: outer.cos(),
        }
    }
}

/// A light as the shader reads it. Must match `Light` in `pbr.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LightRaw {
    /// xyz: where it is, w: how far it reaches
    position: [f32; 4],
    /// rgb: colour times brightness, w: the cosine of the cone's outer angle
    light: [f32; 4],
    /// xyz: the way it faces, w: the cosine of the cone's inner angle
    direction: [f32; 4],
}

/// How the view is cut into cells: across, down, and through its depth. Must match
/// `LIGHT_CELLS` in `pbr.wgsl`.
pub(crate) const CELLS: (u32, u32, u32) = (16, 9, 24);
/// Where the slices through the view's depth begin and end, in metres from the camera. A
/// light further off than the far end lights nothing. Each slice is as deep as it is far
/// away, in proportion, so cells are near enough cubes at any distance.
pub(crate) const NEAR: f32 = 0.5;
pub(crate) const FAR: f32 = 250.0;
/// The most lights a frame takes: those past it, in the order they were gathered, are left
/// out.
pub(crate) const MOST: usize = 4096;

/// The slice of the view's depth that a distance from the camera falls in. The same sum as
/// `light_cell` in `pbr.wgsl`.
pub(crate) fn slice(depth: f32) -> u32 {
    let through = (depth.max(NEAR) / NEAR).ln() / (FAR / NEAR).ln();
    ((through * CELLS.2 as f32) as u32).min(CELLS.2 - 1)
}

/// What a view is seen from, as far as the lights need it.
pub(crate) struct Lookout {
    /// The matrix the shaders find a fragment's place across the picture with.
    pub view_proj: Mat4,
    pub camera: Vec3,
    pub forward: Vec3,
}

/// The lights of a frame, sorted into the view's cells.
#[derive(Default)]
pub(crate) struct LightGrid {
    pub lights: Vec<LightRaw>,
    /// For each cell, where its lights begin in `indices` and how many there are.
    pub cells: Vec<[u32; 2]>,
    pub indices: Vec<u32>,
}

impl LightGrid {
    /// The cells and then the numbers, end to end, as the shader reads them: one buffer,
    /// since a shader may read only so many.
    fn together(&self) -> Vec<u32> {
        self.cells.iter().flatten().chain(&self.indices).copied().collect()
    }
}

impl LightGrid {
    /// Sorts the lights into the cells their spheres touch.
    pub fn build(lights: &[LocalLight], from: &Lookout) -> Self {
        let (across, down, deep) = CELLS;
        let count = (across * down * deep) as usize;
        let mut kept: Vec<LightRaw> = Vec::new();
        // A light's number, for each cell it is in.
        let mut placed: Vec<(u32, u32)> = Vec::new();
        for light in lights.iter().take(MOST) {
            let depth = (light.position - from.camera).dot(from.forward);
            let reach = light.range;
            if reach <= 0.0 || depth + reach < NEAR || depth - reach > FAR {
                continue;
            }
            let number = kept.len() as u32;
            kept.push(LightRaw {
                position: light.position.extend(light.range).to_array(),
                light: light.light.extend(light.cos_outer).to_array(),
                direction: light.direction.extend(light.cos_inner).to_array(),
            });
            let slices = slice(depth - reach)..=slice(depth + reach);
            // Across and down: the box about the sphere, as the picture shows it. A sphere the
            // camera stands in, or beside, may be anywhere in the picture.
            let (mut low, mut high) = ((0, 0), (across - 1, down - 1));
            if depth - reach > 1e-3 {
                let (mut least, mut most) = (glam::Vec2::splat(f32::MAX), glam::Vec2::splat(f32::MIN));
                let mut behind = false;
                for corner in 0..8 {
                    let offset = Vec3::new(
                        if corner & 1 == 0 { -reach } else { reach },
                        if corner & 2 == 0 { -reach } else { reach },
                        if corner & 4 == 0 { -reach } else { reach },
                    );
                    let clip = from.view_proj * (light.position + offset).extend(1.0);
                    if clip.w <= 1e-6 {
                        behind = true;
                        break;
                    }
                    let place = glam::Vec2::new(clip.x, clip.y) / clip.w * 0.5 + 0.5;
                    least = least.min(place);
                    most = most.max(place);
                }
                if !behind {
                    if most.x < 0.0 || most.y < 0.0 || least.x >= 1.0 || least.y >= 1.0 {
                        // Wholly off to one side: in no cell, though it stays in the list.
                        continue;
                    }
                    let cell = |place: f32, cells: u32| ((place.clamp(0.0, 0.999_999) * cells as f32) as u32).min(cells - 1);
                    low = (cell(least.x, across), cell(least.y, down));
                    high = (cell(most.x, across), cell(most.y, down));
                }
            }
            for z in slices {
                for y in low.1..=high.1 {
                    for x in low.0..=high.0 {
                        placed.push(((z * down + y) * across + x, number));
                    }
                }
            }
        }
        // Each cell's lights end to end, cell after cell.
        let mut cells = vec![[0u32; 2]; count];
        for (cell, _) in &placed {
            cells[*cell as usize][1] += 1;
        }
        let mut next = 0;
        for cell in &mut cells {
            cell[0] = next;
            next += cell[1];
            cell[1] = 0;
        }
        let mut indices = vec![0u32; placed.len()];
        for (cell, number) in placed {
            let cell = &mut cells[cell as usize];
            indices[(cell[0] + cell[1]) as usize] = number;
            cell[1] += 1;
        }
        Self { lights: kept, cells, indices }
    }

    /// The numbers of the lights in the cell a point of the world falls in: what a fragment
    /// there goes through. The same sum as `light_cell` in `pbr.wgsl`.
    #[cfg(test)]
    fn at(&self, point: Vec3, from: &Lookout) -> &[u32] {
        let (across, down, _) = CELLS;
        let clip = from.view_proj * point.extend(1.0);
        let place = (glam::Vec2::new(clip.x, clip.y) / clip.w.max(1e-6) * 0.5 + 0.5).clamp(glam::Vec2::ZERO, glam::Vec2::splat(0.999_999));
        let (x, y) = ((place.x * across as f32) as u32, (place.y * down as f32) as u32);
        let z = slice((point - from.camera).dot(from.forward));
        let [first, count] = self.cells[((z * down + y) * across + x) as usize];
        &self.indices[first as usize..(first + count) as usize]
    }
}

/// The lights and their cells on the card.
pub(crate) struct LightBuffers {
    pub lights: wgpu::Buffer,
    /// Each cell's place in the list and count, then the list of light numbers.
    pub grid: wgpu::Buffer,
    /// How many lights and how many numbers there is room for.
    room: (usize, usize),
}

impl LightBuffers {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            lights: Self::make(device, "lights", 64 * std::mem::size_of::<LightRaw>() as u64),
            grid: Self::make(device, "lights by cell", Self::grid_size(1024)),
            room: (64, 1024),
        }
    }

    /// The size of the grid's buffer with room for this many light numbers.
    fn grid_size(numbers: usize) -> u64 {
        ((CELLS.0 * CELLS.1 * CELLS.2) as u64 * 2 + numbers as u64) * 4
    }

    fn make(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Sends a frame's lights. Answers whether a buffer is a new one, as it is when the old
    /// had no room, and what was bound to it must be bound again.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, grid: &LightGrid) -> bool {
        let mut fresh = false;
        if grid.lights.len() > self.room.0 {
            self.room.0 = grid.lights.len().next_power_of_two();
            self.lights = Self::make(device, "lights", (self.room.0 * std::mem::size_of::<LightRaw>()) as u64);
            fresh = true;
        }
        if grid.indices.len() > self.room.1 {
            self.room.1 = grid.indices.len().next_power_of_two();
            self.grid = Self::make(device, "lights by cell", Self::grid_size(self.room.1));
            fresh = true;
        }
        queue.write_buffer(&self.lights, 0, bytemuck::cast_slice(&grid.lights));
        queue.write_buffer(&self.grid, 0, bytemuck::cast_slice(&grid.together()));
        fresh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At the origin, looking down -z.
    fn lookout() -> Lookout {
        Lookout { view_proj: Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 0.1), camera: Vec3::ZERO, forward: Vec3::NEG_Z }
    }

    fn lamp(position: Vec3, range: f32) -> LocalLight {
        LocalLight::point(position, &PointLight { range, ..Default::default() })
    }

    #[test]
    fn a_light_is_found_where_it_shines_and_not_far_from_it() {
        let from = lookout();
        let grid = LightGrid::build(&[lamp(Vec3::new(0.0, 0.0, -20.0), 3.0), lamp(Vec3::new(30.0, 0.0, -60.0), 4.0)], &from);
        assert_eq!(grid.lights.len(), 2);
        // Anywhere inside a light's sphere, a fragment finds it.
        for offset in [Vec3::ZERO, Vec3::new(2.9, 0.0, 0.0), Vec3::new(0.0, -2.9, 0.0), Vec3::new(0.0, 0.0, 2.9), Vec3::new(1.6, 1.6, -1.6)] {
            assert!(grid.at(Vec3::new(0.0, 0.0, -20.0) + offset, &from).contains(&0), "at {offset}");
            assert!(grid.at(Vec3::new(30.0, 0.0, -60.0) + offset, &from).contains(&1), "at {offset}");
        }
        // Well away from both, it finds neither.
        assert!(grid.at(Vec3::new(-25.0, 0.0, -40.0), &from).is_empty());
        assert!(grid.at(Vec3::new(0.0, 0.0, -150.0), &from).is_empty());
        // And most cells hold nothing: the lights are small in a wide view.
        let empty = grid.cells.iter().filter(|cell| cell[1] == 0).count();
        assert!(empty * 10 > grid.cells.len() * 9, "{empty} of {} cells are empty", grid.cells.len());
    }

    #[test]
    fn lights_that_cannot_be_seen_are_left_out() {
        let from = lookout();
        let behind = lamp(Vec3::new(0.0, 0.0, 40.0), 5.0);
        let too_far = lamp(Vec3::new(0.0, 0.0, -400.0), 5.0);
        let dark = lamp(Vec3::new(0.0, 0.0, -10.0), 0.0);
        let aside = lamp(Vec3::new(500.0, 0.0, -20.0), 5.0);
        let grid = LightGrid::build(&[behind, too_far, dark, aside], &from);
        // The one off to the side is kept in the list but is in no cell.
        assert_eq!(grid.lights.len(), 1);
        assert!(grid.indices.is_empty());
    }

    #[test]
    fn a_light_the_camera_stands_in_is_everywhere_near() {
        let from = lookout();
        let grid = LightGrid::build(&[lamp(Vec3::new(0.0, 0.0, 1.0), 6.0)], &from);
        // Every cell across and down, in the slices it reaches.
        for point in [Vec3::new(0.0, 0.0, -2.0), Vec3::new(-3.0, 1.5, -2.0), Vec3::new(3.0, -1.5, -2.0)] {
            assert_eq!(grid.at(point, &from), [0]);
        }
        assert!(grid.at(Vec3::new(0.0, 0.0, -30.0), &from).is_empty());
    }

    #[test]
    fn slices_grow_with_distance_and_cover_the_whole_depth() {
        assert_eq!(slice(0.0), 0);
        assert_eq!(slice(NEAR), 0);
        assert_eq!(slice(FAR * 2.0), CELLS.2 - 1);
        let mut last = 0;
        for metres in 1..250 {
            let now = slice(metres as f32);
            assert!(now >= last);
            last = now;
        }
        assert_eq!(last, CELLS.2 - 1);
    }

    #[test]
    fn a_spot_lights_cone_is_kept_in_order() {
        let spot = LocalLight::spot(Vec3::ZERO, Vec3::new(0.0, -2.0, 0.0), &SpotLight { inner: 0.9, outer: 0.4, ..Default::default() });
        assert!(spot.cos_inner > spot.cos_outer, "the inner angle is the smaller");
        assert!((spot.direction.length() - 1.0).abs() < 1e-6);
        let point = lamp(Vec3::ZERO, 1.0);
        assert!(point.cos_outer < -1.0, "every direction is inside a point light");
    }
}
