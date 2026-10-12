//! Lights with a place: lamps, fires, torches.
//!
//! A [`PointLight`] shines all round and a [`SpotLight`] in a cone, each as far as its
//! `range`. Either may cast shadows: a spot light into one shadow map, a point light into
//! six, one for each way it shines.
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
    /// Whether what stands in its light casts a shadow. It takes six of the sixteen shadow
    /// maps there are for lights, where a spot light takes one: see [`SpotLight::shadows`].
    pub shadows: bool,
}

impl crate::ecs::Component for PointLight {}

impl Default for PointLight {
    fn default() -> Self {
        Self { color: Color::WHITE, intensity: 4.0, range: 8.0, shadows: false }
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
    /// Whether what stands in its light casts a shadow. There are sixteen shadow maps for
    /// lights: the lights that ask are given them nearest the camera first, one to a spot
    /// light and six to a point light, and those left without shine through things.
    pub shadows: bool,
}

impl crate::ecs::Component for SpotLight {}

impl Default for SpotLight {
    fn default() -> Self {
        Self { color: Color::WHITE, intensity: 8.0, range: 12.0, inner: 0.35, outer: 0.5, shadows: false }
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
    /// Whether it is to cast shadows, and which of the shadow maps is its own once one has
    /// been given it: see [`shadowed`].
    pub casts: bool,
    pub shadow: Option<u32>,
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
            casts: light.shadows,
            shadow: None,
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
            casts: light.shadows,
            shadow: None,
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
    /// x: which shadow map is its own, or the first of its six, or less than nought where it
    /// has none; y: 1 for a point light, which has six
    shadow: [f32; 4],
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

/// How many shadow maps there are for lights: one for a spot light, six for a point light.
pub(crate) const SHADOWED: usize = 16;
/// The six ways a point light's maps look, in the order the shader takes them in: along x and
/// against it, then y, then z. With each, which way is up in it.
const FACES: [(Vec3, Vec3); 6] = [(Vec3::X, Vec3::Y), (Vec3::NEG_X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::NEG_Y, Vec3::Z), (Vec3::Z, Vec3::Y), (Vec3::NEG_Z, Vec3::Y)];

/// Which of a point light's six maps a direction from it falls in: the one that looks along
/// the axis the direction is longest on. The same sum as in `light_visibility` in `pbr.wgsl`.
#[cfg(test)]
fn face(direction: Vec3) -> usize {
    let size = direction.abs();
    if size.x >= size.y && size.x >= size.z {
        if direction.x >= 0.0 { 0 } else { 1 }
    } else if size.y >= size.z {
        if direction.y >= 0.0 { 2 } else { 3 }
    } else if direction.z >= 0.0 {
        4
    } else {
        5
    }
}
/// How wide and high each of those maps is.
pub(crate) const SHADOW_MAP: u32 = 1024;

/// Gives the shadow maps to the lights that are to cast, those nearest the camera first,
/// and answers what each map is drawn with: the light's own view of what it shines on.
pub(crate) fn shadowed(lights: &mut [LocalLight], camera: Vec3) -> Vec<Mat4> {
    let mut casting: Vec<(f32, usize)> = lights
        .iter()
        .enumerate()
        .take(MOST)
        .filter(|(_, light)| light.casts && light.range > 0.05)
        .map(|(at, light)| (light.position.distance(camera), at))
        .filter(|(away, at)| *away - lights[*at].range < FAR)
        .collect();
    casting.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut maps = Vec::new();
    for (_, at) in casting {
        let light = &mut lights[at];
        let looks = |toward: Vec3, up: Vec3, across: f32| Mat4::perspective_rh(across, 1.0, 0.05, light.range) * Mat4::look_at_rh(light.position, light.position + toward, up);
        // A point light shines every way, and has a map for each of six; a little wider than
        // the quarter turn each has to take in, so that their edges overlap.
        let its: Vec<Mat4> = if light.cos_outer < -1.0 {
            FACES.iter().map(|(toward, up)| looks(*toward, *up, std::f32::consts::FRAC_PI_2 + 0.04)).collect()
        } else {
            // A little wider than the cone, so that its edge is not the map's.
            let across = (2.0 * light.cos_outer.clamp(-1.0, 1.0).acos() + 0.1).min(3.0);
            let up = if light.direction.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
            vec![looks(light.direction, up, across)]
        };
        // One that there is no room left for goes without; a smaller one after it may fit.
        if maps.len() + its.len() <= SHADOWED {
            light.shadow = Some(maps.len() as u32);
            maps.extend(its);
        }
    }
    maps
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
                shadow: [light.shadow.map_or(-1.0, |map| map as f32), if light.cos_outer < -1.0 { 1.0 } else { 0.0 }, 0.0, 0.0],
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
    fn the_nearest_lights_that_ask_are_given_the_shadow_maps() {
        let down = |at: Vec3, shadows: bool| LocalLight::spot(at, Vec3::NEG_Y, &SpotLight { range: 10.0, outer: 0.5, shadows, ..Default::default() });
        // Twenty that ask, further and further off, one that does not ask, and a point light.
        let mut lights: Vec<LocalLight> = (0..20).rev().map(|i| down(Vec3::new(0.0, 5.0, -5.0 * i as f32), true)).collect();
        lights.push(down(Vec3::new(1.0, 5.0, 0.0), false));
        lights.push(lamp(Vec3::ZERO, 5.0));
        let maps = shadowed(&mut lights, Vec3::ZERO);
        assert_eq!(maps.len(), SHADOWED);
        // The nearest has the first map, the furthest have none, nor have those that did
        // not ask.
        assert_eq!(lights[19].shadow, Some(0));
        assert_eq!(lights[18].shadow, Some(1));
        assert!(lights[0].shadow.is_none() && lights[1].shadow.is_none() && lights[20].shadow.is_none() && lights[21].shadow.is_none());
        // Its map takes in what is in its cone, within its reach, and not what is behind it
        // or beyond.
        let seen = |point: Vec3| {
            let clip = maps[0] * point.extend(1.0);
            clip.w > 0.0 && clip.x.abs() <= clip.w && clip.y.abs() <= clip.w && (0.0..=clip.w).contains(&clip.z)
        };
        assert!(seen(Vec3::new(0.0, 0.0, 0.0)) && seen(Vec3::new(2.0, 0.0, 0.0)) && seen(Vec3::new(0.0, 0.0, -2.0)));
        assert!(!seen(Vec3::new(0.0, 8.0, 0.0)) && !seen(Vec3::new(0.0, -6.0, 0.0)) && !seen(Vec3::new(9.0, 0.0, 0.0)));
        // A point light that asks has six, one for each way: whichever way a thing is from it,
        // the map chosen for that way takes the thing in.
        let mut lantern = [LocalLight::point(Vec3::new(1.0, 2.0, 3.0), &PointLight { range: 9.0, shadows: true, ..Default::default() }), down(Vec3::new(0.0, 5.0, -4.0), true)];
        let maps = shadowed(&mut lantern, Vec3::ZERO);
        assert_eq!((maps.len(), lantern[0].shadow, lantern[1].shadow), (7, Some(0), Some(6)));
        for toward in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z, Vec3::new(1.0, 0.98, 0.97), Vec3::new(-0.3, -0.9, 0.89), Vec3::new(0.2, 0.1, -0.21)] {
            let clip = maps[face(toward)] * (lantern[0].position + toward.normalize() * 4.0).extend(1.0);
            assert!(clip.w > 0.0 && clip.x.abs() <= clip.w && clip.y.abs() <= clip.w && (0.0..=clip.w).contains(&clip.z), "toward {toward}");
        }
        // Three of them would want eighteen maps: two are given theirs and the third is not.
        let mut three: Vec<LocalLight> = (0..3).map(|i| LocalLight::point(Vec3::new(0.0, 0.0, -5.0 * i as f32), &PointLight { shadows: true, ..Default::default() })).collect();
        assert_eq!(shadowed(&mut three, Vec3::ZERO).len(), 12);
        assert!(three[2].shadow.is_none());
        // And is told to the shader with the light.
        let grid = LightGrid::build(&lights, &Lookout { camera: Vec3::new(0.0, 2.0, 30.0), ..lookout() });
        assert!(grid.lights.iter().any(|light| light.shadow[0] == 0.0) && grid.lights.iter().any(|light| light.shadow[0] < 0.0));
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
