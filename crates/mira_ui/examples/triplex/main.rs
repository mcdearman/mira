//! Triplex acies: a legion of ten cohorts in three lines, four, three and three, against a
//! Gallic host. Seen from above at a slant, orthographically, with every man a sprite.
//!
//! ```sh
//! cargo run --release -p mira_ui --example triplex
//! cargo run --release -p mira_ui --example triplex -- watch     # it commands itself
//! ```
//!
//! The front line tires as it fights. Relieve it (R, or the trumpet button): it falls back
//! through the gaps of the second line while the second comes forward through the gaps of the
//! first, and fresh men meet the Gauls. Caesar steadies the cohorts near him; ride him to where
//! the line is wavering. Orders to cohorts beyond his voice go by rider, and take time to
//! arrive; trumpet calls reach everyone at once.
//!
//! - Left click a cohort, or drag a box round several, to choose them (Shift adds). 1, 2, 3
//!   choose a line; C chooses Caesar.
//! - Right click to send them there; drag with the right button to say which way they face.
//! - F advances the chosen cohorts, H halts them, R sounds the relief of the line.
//! - W A S D or the arrows move the view, the wheel zooms; Space pauses; + and − change speed.
//!
//! For looking at it without playing: `watch` (it commands itself), `follow` (the view keeps
//! Caesar in the middle), `speed=4`, `zoom=60` (metres of the field the view is tall),
//! `look=x,z`, `size=1280x720`, `shot=picture.png@95` (a screenshot at 95 seconds of battle;
//! as many as wanted) and `quit=100`.
//!
//! The battle is in `battle.rs`, as plain data stepped twenty times a second: nothing in it
//! draws or reads input. The sprites are rendered by `tools/romans.py` (Blender) from the
//! Rocketbox figure the Paris demo uses, and are drawn from one sheet per kind of man with
//! `Material::texture_region`.

mod battle;

use std::f32::consts::{FRAC_PI_4, PI};

use battle::{facing_vector, right_of, Battle, Command, Outcome, Side, DT, STEPS_A_SECOND};
use mira::prelude::*;
use mira::render::{PointerSight, Screenshot};
use mira_ui::{
    armature::{App as Interface, Element, Scheme, Style},
    kit::{anchored, bar, button, caption, column, heading, panel, row, stack, Anchor, Theme},
    UiHost, UiPlugin,
};

const SPRITES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../res/triplex/sprites");

/// As the sprites were rendered (see `res/triplex/sprites/sprites.json`): the camera's angle
/// below the horizon, and metres of the world to a pixel of a sprite.
const PITCH: f32 = 35.0 * PI / 180.0;
const PX: f32 = 0.0185;
/// How far each sprite is brought toward the camera: enough that the parts of the picture
/// below a man's feet (a foot stepping toward us, a horse's forelegs) stay above the ground.
const LIFT: f32 = 2.5;

fn view_dir() -> Vec3 {
    Vec3::new(0.0, -PITCH.sin(), -PITCH.cos())
}

fn view_up() -> Vec3 {
    Vec3::new(0.0, PITCH.cos(), -PITCH.sin())
}

// ---- Sprite sheets ---------------------------------------------------------------------------

/// A sheet of one kind of man: a row for each of eight directions, a column for each frame.
#[derive(Clone, Copy)]
struct Sheet {
    image: Handle<Image>,
    mesh: Handle<Mesh>,
    cell: (u32, u32),
    size: (u32, u32),
}

impl Sheet {
    fn region(&self, column: u32, row: u32) -> Vec4 {
        let (w, h) = (self.cell.0 as f32, self.cell.1 as f32);
        let (sw, sh) = (self.size.0 as f32, self.size.1 as f32);
        Vec4::new((column as f32 * w + 0.5) / sw, (row as f32 * h + 0.5) / sh, (w - 1.0) / sw, (h - 1.0) / sh)
    }

    fn material(&self, column: u32, row: u32) -> Material {
        Material {
            base_color_texture: Some(self.image),
            alpha_cutoff: Some(0.5),
            roughness: 1.0,
            texture_region: Some(self.region(column, row)),
            ..Default::default()
        }
    }
}

struct Sheets {
    legionary: Sheet,
    gaul: Sheet,
    legate: Sheet,
    commander: Sheet,
}

/// A square standing up to face the camera, as big as a sprite's cell, with the man's feet
/// at the origin.
fn sprite_mesh(cell: (u32, u32), foot: u32) -> Mesh {
    let (w, h) = (cell.0 as f32 * PX, cell.1 as f32 * PX);
    let up = view_up();
    let centre = -view_dir() * LIFT + up * (h * 0.5 - foot as f32 * PX);
    let mut mesh = Mesh::default();
    mesh.push_quad(centre, -view_dir(), Vec3::X * (w * 0.5), up * (h * 0.5));
    mesh
}

fn load_sheet(images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, name: &str, cell: (u32, u32), foot: u32) -> anyhow::Result<Sheet> {
    let path = format!("{SPRITES}/{name}.png");
    let bytes = std::fs::read(&path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let image = Image::from_bytes(&bytes, true)?;
    let size = (image.width, image.height);
    Ok(Sheet { image: images.add(image), mesh: meshes.add(sprite_mesh(cell, foot)), cell, size })
}

/// Which of the eight rows of a sheet shows a man facing this way.
fn direction(facing: f32) -> u32 {
    ((facing / FRAC_PI_4).round() as i32).rem_euclid(8) as u32
}

const WALK: u32 = 1;
const FIGHT: u32 = 9;
const DEAD: u32 = 15;
/// Metres walked between one frame of the walk and the next.
const STRIDE: f32 = 0.16;

/// A man's picture: which man, and what he shows now.
struct Sprite {
    man: usize,
    shown: (u32, u32),
    at: Vec2,
}

impl Component for Sprite {}

struct RiderSprite {
    rider: usize,
    shown: (u32, u32),
}

impl Component for RiderSprite {}

// ---- The battle as a resource ------------------------------------------------------------------

/// The battle, and how fast time passes in it.
struct Field {
    battle: Battle,
    /// Seconds not yet stepped.
    owed: f32,
    speed: f32,
    paused: bool,
    /// It commands itself.
    watch: bool,
    /// When it last relieved its line by itself.
    watch_relieved: f32,
}

#[derive(Default)]
struct Chosen {
    units: Vec<usize>,
    riders: Vec<usize>,
    /// Where the left button went down, on the screen and the ground.
    drag: Option<(Vec2, Vec2)>,
    /// Where the right button went down, on the ground.
    aim: Option<Vec2>,
}

struct View {
    focus: Vec2,
    height: f32,
    /// Keep Caesar in the middle of the view.
    follow: bool,
}

/// Screenshots to take, at seconds of battle, and when to stop.
#[derive(Default)]
struct Shots {
    at: Vec<(f32, String)>,
    quit_after: Option<f32>,
}

// ---- Setup ------------------------------------------------------------------------------------

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut meshes: ResMut<Assets<Mesh>>, field: Res<Field>) {
    let load = |images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, name: &str, cell, foot| {
        load_sheet(images, meshes, name, cell, foot).unwrap_or_else(|e| panic!("couldn't load the sprites: {e:#} (render them with tools/romans.py)"))
    };
    let sheets = Sheets {
        legionary: load(&mut images, &mut meshes, "legionary", (112, 144), 22),
        gaul: load(&mut images, &mut meshes, "gaul", (112, 144), 22),
        legate: load(&mut images, &mut meshes, "legate", (208, 192), 50),
        commander: load(&mut images, &mut meshes, "commander", (208, 192), 50),
    };
    let b = &field.battle;
    for (i, man) in b.men.iter().enumerate() {
        let sheet = if b.units[man.unit].side == Side::Rome { sheets.legionary } else { sheets.gaul };
        let row = direction(man.facing);
        commands.spawn((
            Transform::from_xyz(man.pos.x, 0.0, man.pos.y),
            Mesh3d(sheet.mesh),
            sheet.material(0, row),
            NotShadowCaster,
            Sprite { man: i, shown: (0, row), at: man.pos },
        ));
    }
    for (i, r) in b.riders.iter().enumerate() {
        let sheet = if r.commander { sheets.commander } else { sheets.legate };
        let row = direction(r.facing);
        commands.spawn((Transform::from_xyz(r.pos.x, 0.0, r.pos.y), Mesh3d(sheet.mesh), sheet.material(0, row), NotShadowCaster, RiderSprite { rider: i, shown: (0, row) }));
    }
    // The field: grass, with a darker band where a stream once ran.
    let ground = images.add(grass(1024));
    let mut mesh = Mesh::default();
    mesh.push_quad(Vec3::ZERO, Vec3::Y, Vec3::Z * 1500.0, Vec3::X * 1500.0);
    for v in &mut mesh.vertices {
        v.uv = [v.uv[0] * 3000.0 / 48.0, v.uv[1] * 3000.0 / 48.0];
    }
    commands.spawn((Transform::default(), Mesh3d(meshes.add(mesh)), Material { base_color_texture: Some(ground), roughness: 1.0, ..Default::default() }, NotShadowCaster));
    commands.spawn((Transform::default(), Camera { orthographic_height: Some(150.0), far: 4000.0, near: 0.1, ..Default::default() }));
    commands.insert_resource(sheets);
}

/// A tile of grass that repeats without a seam: noise over a lattice that wraps.
fn grass(size: u32) -> Image {
    let hash = |x: i32, y: i32, period: i32, seed: u32| {
        let (x, y) = (x.rem_euclid(period) as u32, y.rem_euclid(period) as u32);
        let mut h = x.wrapping_mul(0x8da6_b343).wrapping_add(y.wrapping_mul(0xd816_3841)).wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
        h ^= h >> 15;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 16;
        h as f32 / u32::MAX as f32
    };
    let noise = |u: f32, v: f32, period: i32, seed: u32| {
        let (x, y) = (u * period as f32, v * period as f32);
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let a = hash(x0, y0, period, seed) + (hash(x0 + 1, y0, period, seed) - hash(x0, y0, period, seed)) * sx;
        let b = hash(x0, y0 + 1, period, seed) + (hash(x0 + 1, y0 + 1, period, seed) - hash(x0, y0 + 1, period, seed)) * sx;
        a + (b - a) * sy
    };
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32, y as f32 / size as f32);
            let broad = noise(u, v, 3, 1) * 0.5 + noise(u, v, 7, 2) * 0.3 + noise(u, v, 17, 3) * 0.2;
            let fine = noise(u, v, 64, 4) * 0.5 + noise(u, v, 160, 5) * 0.5;
            let dry = (noise(u, v, 5, 6) - 0.55).max(0.0) * 1.2;
            let shade = 0.88 + 0.18 * (broad - 0.5) + 0.12 * (fine - 0.5);
            let (r, g, b) = (92.0 + 30.0 * dry, 114.0 - 2.0 * dry, 62.0 + 4.0 * dry);
            data.extend([(r * shade).min(255.0) as u8, (g * shade).min(255.0) as u8, (b * shade).min(255.0) as u8, 255]);
        }
    }
    Image::from_rgba(size, size, data, true)
}

// ---- Stepping the battle ------------------------------------------------------------------------

fn step(time: Res<Time>, mut field: ResMut<Field>) {
    if field.paused {
        return;
    }
    let speed = field.speed;
    field.owed = (field.owed + time.delta_secs() * speed).min(0.5 * speed.max(1.0));
    while field.owed >= DT {
        field.owed -= DT;
        if field.watch {
            command_itself(&mut field);
        }
        field.battle.step();
    }
}

/// What a careful general does, for `watch`: hold until the Gauls close, relieve the line when
/// the front is tired, and ride to wherever the line is shakiest.
fn command_itself(field: &mut Field) {
    let b = &mut field.battle;
    let front = b.romans_in_line(1);
    if front.is_empty() {
        return;
    }
    let tired = front.iter().map(|&u| b.units[u].fatigue).sum::<f32>() / front.len() as f32;
    if tired > 0.5 && b.time - field.watch_relieved > 30.0 && !b.romans_in_line(2).is_empty() {
        field.watch_relieved = b.time;
        b.relieve_the_line();
    }
    if b.steps.is_multiple_of(40) {
        let shakiest = front.iter().copied().filter(|&u| b.units[u].fighting()).min_by(|&a, &c| b.units[a].cohesion.total_cmp(&b.units[c].cohesion));
        if let Some(u) = shakiest {
            let behind = b.units[u].pos - b.units[u].forward() * (b.units[u].depth() + 12.0);
            b.riders[0].target = Some(behind);
        }
    }
}

// ---- Showing it ---------------------------------------------------------------------------------

fn show_men(field: Res<Field>, sheets: Res<Sheets>, mut sprites: Query<(&mut Sprite, &mut Transform, &mut Material)>) {
    let b = &field.battle;
    let alpha = if field.paused { 1.0 } else { (field.owed / DT).clamp(0.0, 1.0) };
    for (mut sprite, mut at, mut material) in &mut sprites {
        let man = &b.men[sprite.man];
        let pos = if man.alive { man.was.lerp(man.pos, alpha) } else { man.pos };
        if pos != sprite.at {
            sprite.at = pos;
            at.translation = Vec3::new(pos.x, 0.0, pos.y);
        }
        let column = if !man.alive {
            DEAD
        } else if man.fighting {
            FIGHT + ((man.swing * 6.0) as u32).min(5)
        } else if man.moving {
            WALK + ((man.walked / STRIDE) as u32 % 8)
        } else {
            0
        };
        let shown = (column, direction(man.facing));
        if shown != sprite.shown {
            sprite.shown = shown;
            let sheet = if b.units[man.unit].side == Side::Rome { sheets.legionary } else { sheets.gaul };
            material.texture_region = Some(sheet.region(shown.0, shown.1));
        }
    }
}

fn show_riders(field: Res<Field>, sheets: Res<Sheets>, mut sprites: Query<(&mut RiderSprite, &mut Transform, &mut Material)>) {
    let b = &field.battle;
    let alpha = if field.paused { 1.0 } else { (field.owed / DT).clamp(0.0, 1.0) };
    for (mut sprite, mut at, mut material) in &mut sprites {
        let r = &b.riders[sprite.rider];
        let pos = r.was.lerp(r.pos, alpha);
        at.translation = Vec3::new(pos.x, 0.0, pos.y);
        let column = if r.moving { 1 + ((r.walked / 0.45) as u32 % 8) } else { 0 };
        let shown = (column, direction(r.facing));
        if shown != sprite.shown {
            sprite.shown = shown;
            let sheet = if r.commander { sheets.commander } else { sheets.legate };
            material.texture_region = Some(sheet.region(shown.0, shown.1));
        }
    }
}

fn ground(p: Vec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y, p.y)
}

/// Marks on the field: what is chosen and where it is going, Caesar's reach, riders with
/// orders, and pila in the air.
fn marks(field: Res<Field>, chosen: Res<Chosen>, pointer: Res<PointerSight>, mut lines: ResMut<DebugLines>) {
    let b = &field.battle;
    lines.width = 2.0;
    for &u in &chosen.units {
        let unit = &b.units[u];
        let f = unit.forward();
        let r = right_of(f) * (unit.width() * 0.5 + 1.0);
        let back = f * (unit.depth() + 0.5);
        let front = unit.pos + f * 1.0;
        let corners = [front - r, front + r, front + r - back, front - r - back].map(|p| ground(p, 0.3));
        lines.path(&corners, true, Color::srgb(1.0, 0.85, 0.3));
        if let battle::Order::MoveTo { to, facing } = unit.order {
            lines.arrow(ground(unit.pos, 0.3), ground(to, 0.3), Color::srgb(0.9, 0.9, 0.9));
            lines.arrow(ground(to, 0.3), ground(to + facing_vector(facing) * 8.0, 0.3), Color::srgb(0.9, 0.9, 0.9));
        }
    }
    for &r in &chosen.riders {
        let rider = &b.riders[r];
        lines.circle(ground(rider.pos, 0.3), Vec3::Y, 4.0, Color::srgb(1.0, 0.85, 0.3));
        if let Some(t) = rider.target {
            lines.arrow(ground(rider.pos, 0.3), ground(t, 0.3), Color::srgb(0.9, 0.9, 0.9));
        }
    }
    for rider in &b.riders {
        let colour = if rider.commander { Color::srgb(0.95, 0.35, 0.25) } else { Color::srgb(0.75, 0.55, 0.45) };
        lines.width = 1.0;
        lines.circle(ground(rider.pos, 0.2), Vec3::Y, rider.reach(), colour);
    }
    lines.width = 2.0;
    for m in &b.messages {
        if let Some(at) = m.rider {
            let to = b.units[m.unit].pos;
            lines.line(ground(at, 0.4), ground(to, 0.4), Color::srgb(0.85, 0.85, 1.0));
            lines.circle(ground(at, 0.4), Vec3::Y, 2.0, Color::srgb(0.85, 0.85, 1.0));
        }
    }
    lines.width = 1.2;
    for v in &b.volleys {
        let t = ((b.time - v.thrown) / 1.1).clamp(0.0, 1.0);
        let d = v.to - v.from;
        let r = right_of(d.normalize_or_zero());
        for k in 0..12 {
            let across = (k as f32 / 11.0 - 0.5) * v.width;
            let a = v.from + r * across;
            let c = v.to + r * across * 0.8;
            let at = |s: f32| {
                let p = a.lerp(c, s);
                Vec3::new(p.x, 1.8 + (s * PI).sin() * 7.0, p.y)
            };
            let s = (t + k as f32 * 0.013).min(1.0);
            lines.line(at((s - 0.06).max(0.0)), at(s), Color::srgb(0.3, 0.25, 0.2));
        }
    }
    if let (Some((_, start)), Some(now)) = (chosen.drag, pointer.on_level(0.0)) {
        let now = Vec2::new(now.x, now.z);
        let corners = [start, Vec2::new(now.x, start.y), now, Vec2::new(start.x, now.y)].map(|p| ground(p, 0.3));
        lines.path(&corners, true, Color::srgb(1.0, 1.0, 1.0));
    }
}

fn place_camera(mut view: ResMut<View>, field: Res<Field>, mut cameras: Query<(&mut Transform, &mut Camera)>) {
    if view.follow {
        view.focus = field.battle.riders[0].pos - Vec2::new(0.0, view.height * 0.15);
    }
    for (mut at, mut camera) in &mut cameras {
        let focus = Vec3::new(view.focus.x, 0.0, view.focus.y);
        *at = Transform::from_translation(focus - view_dir() * 1500.0).looking_at(focus, Vec3::Y);
        camera.orthographic_height = Some(view.height);
    }
}

// ---- Input --------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn control(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mouse: Res<Mouse>,
    time: Res<Time>,
    pointer: Res<PointerSight>,
    window: Option<Res<Window>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut view: ResMut<View>,
    mut field: ResMut<Field>,
    mut chosen: ResMut<Chosen>,
) {
    // The view.
    let pan = view.height * 0.9 * time.delta_secs();
    let held = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
    let mut d = Vec2::ZERO;
    if held(KeyCode::KeyW, KeyCode::ArrowUp) {
        d.y -= 1.0;
    }
    if held(KeyCode::KeyS, KeyCode::ArrowDown) {
        d.y += 1.0;
    }
    if held(KeyCode::KeyA, KeyCode::ArrowLeft) {
        d.x -= 1.0;
    }
    if held(KeyCode::KeyD, KeyCode::ArrowRight) {
        d.x += 1.0;
    }
    if d != Vec2::ZERO {
        view.follow = false;
    }
    view.focus += d * pan;
    if mouse.scroll.y != 0.0 {
        view.height = (view.height * 0.88f32.powf(mouse.scroll.y.signum() * mouse.scroll.y.abs().min(3.0))).clamp(18.0, 600.0);
    }
    // Time.
    if keys.just_pressed(KeyCode::Space) {
        field.paused = !field.paused;
    }
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        field.speed = (field.speed * 2.0).min(8.0);
    }
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        field.speed = (field.speed * 0.5).max(0.25);
    }
    // Orders by key.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    for (key, line) in [(KeyCode::Digit1, 1), (KeyCode::Digit2, 2), (KeyCode::Digit3, 3)] {
        if keys.just_pressed(key) {
            if !shift {
                chosen.units.clear();
                chosen.riders.clear();
            }
            chosen.units.extend(field.battle.romans_in_line(line));
        }
    }
    if keys.just_pressed(KeyCode::KeyC) {
        chosen.units.clear();
        chosen.riders = vec![0];
        view.focus = field.battle.riders[0].pos;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        field.battle.relieve_the_line();
    }
    if keys.just_pressed(KeyCode::KeyF) {
        advance(&mut field.battle, &chosen.units);
    }
    if keys.just_pressed(KeyCode::KeyH) {
        halt(&mut field.battle, &chosen.units);
    }
    // The pointer.
    let Some(here) = pointer.on_level(0.0).map(|p| Vec2::new(p.x, p.z)) else {
        return;
    };
    let screen = mouse.position.unwrap_or_default();
    if buttons.just_pressed(MouseButton::Left) {
        chosen.drag = Some((screen, here));
    }
    if buttons.just_released(MouseButton::Left) {
        if let Some((from_screen, from)) = chosen.drag.take() {
            if !shift {
                chosen.units.clear();
                chosen.riders.clear();
            }
            let b = &field.battle;
            if from_screen.distance(screen) < 6.0 {
                // A click: the general under it, or else the cohort.
                if let Some(r) = (0..b.riders.len()).find(|&r| b.riders[r].pos.distance(here) < 5.0) {
                    chosen.riders.push(r);
                } else if let Some(u) = (0..b.units.len()).find(|&u| b.units[u].side == Side::Rome && b.units[u].alive() && b.units[u].covers(here, 2.0)) {
                    if !chosen.units.contains(&u) {
                        chosen.units.push(u);
                    }
                }
            } else if let Some(project) = projector(&cameras, window.as_deref()) {
                // A box: everything Roman whose middle is inside it on the screen.
                let (lo, hi) = (from_screen.min(screen), from_screen.max(screen));
                let inside = |p: Vec2| {
                    let s = project(ground(p, 0.0));
                    s.x >= lo.x && s.x <= hi.x && s.y >= lo.y && s.y <= hi.y
                };
                let _ = from;
                for u in 0..b.units.len() {
                    let unit = &b.units[u];
                    if unit.side == Side::Rome && unit.alive() && inside(unit.pos - unit.forward() * unit.depth() * 0.5) && !chosen.units.contains(&u) {
                        chosen.units.push(u);
                    }
                }
                for r in 0..b.riders.len() {
                    if inside(b.riders[r].pos) && !chosen.riders.contains(&r) {
                        chosen.riders.push(r);
                    }
                }
            }
        }
    }
    if buttons.just_pressed(MouseButton::Right) {
        chosen.aim = Some(here);
    }
    if buttons.just_released(MouseButton::Right) {
        if let Some(to) = chosen.aim.take() {
            let facing = ((here - to).length() > 4.0).then(|| (here - to).x.atan2((here - to).y));
            send(&mut field.battle, &chosen, to, facing);
        }
    }
    let _ = time;
}

/// Where a point of the world is on the screen, in the pointer's pixels.
fn projector(cameras: &Query<(&Camera, &GlobalTransform)>, window: Option<&Window>) -> Option<impl Fn(Vec3) -> Vec2> {
    let size = window?.size().as_vec2();
    let (camera, placed) = cameras.iter().find(|(camera, _)| camera.active)?;
    let view_proj = camera.projection(size.x / size.y) * placed.0.inverse();
    Some(move |p: Vec3| {
        let c = view_proj.project_point3(p);
        Vec2::new((c.x * 0.5 + 0.5) * size.x, (0.5 - c.y * 0.5) * size.y)
    })
}

/// Sends what is chosen to a place, keeping the cohorts where they stand to one another, and
/// turned if a facing was given.
fn send(b: &mut Battle, chosen: &Chosen, to: Vec2, facing: Option<f32>) {
    for &r in &chosen.riders {
        b.riders[r].target = Some(to);
    }
    if chosen.units.is_empty() {
        return;
    }
    let middle = chosen.units.iter().map(|&u| b.units[u].pos).sum::<Vec2>() / chosen.units.len() as f32;
    let now = chosen.units.iter().map(|&u| b.units[u].facing).sum::<f32>() / chosen.units.len() as f32;
    let turn = facing.map_or(0.0, |f| f - now);
    let (s, c) = turn.sin_cos();
    for &u in &chosen.units {
        let off = b.units[u].pos - middle;
        // Turning by `turn` about the up axis, in this ground's (x, z).
        let off = Vec2::new(off.x * c + off.y * s, -off.x * s + off.y * c);
        let facing = facing.unwrap_or(b.units[u].facing);
        b.order(u, Command::Move { to: to + off, facing });
    }
}

fn advance(b: &mut Battle, units: &[usize]) {
    let all: Vec<usize> = if units.is_empty() { b.romans_in_line(1) } else { units.to_vec() };
    for u in all {
        b.order(u, Command::Advance);
    }
}

fn halt(b: &mut Battle, units: &[usize]) {
    for &u in units {
        b.order(u, Command::Halt);
    }
}

// ---- The panels ---------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Ask {
    Advance,
    Halt,
    Relieve,
    Line(u8),
    Caesar,
    Pause,
    Faster,
    Slower,
}

#[derive(Default)]
struct Hud {
    time: f32,
    romans: usize,
    gauls: usize,
    outcome: String,
    news: Vec<String>,
    chosen: Vec<(String, String, f32, f32, f32)>,
    generals: Vec<(String, bool)>,
    paused: bool,
    speed: f32,
    riders_out: usize,
    asked: Vec<Ask>,
}

impl Interface for Hud {
    type Message = Ask;

    fn update(&mut self, ask: Ask) {
        self.asked.push(ask);
    }

    fn style(&self, _: Scheme) -> Style {
        Theme::default().style()
    }

    fn view(&self) -> Element<Ask> {
        let minutes = (self.time / 60.0) as u32;
        let mut news = column().spacing(4.0);
        for line in &self.news {
            news = news.push(caption(line.clone()));
        }
        let state = anchored(
            Anchor::TopLeft,
            panel(
                column()
                    .spacing(6.0)
                    .push(heading("Triplex acies"))
                    .push(caption(format!(
                        "{minutes}:{:02}   Romans {}   Gauls {}{}",
                        self.time as u32 % 60,
                        self.romans,
                        self.gauls,
                        if self.paused { "   (paused)".to_owned() } else if self.speed != 1.0 { format!("   ({}x)", self.speed) } else { String::new() }
                    )))
                    .push(caption(self.outcome.clone()))
                    .push(news),
            ),
        );
        let mut chosen = column().spacing(6.0);
        if self.chosen.is_empty() && self.generals.is_empty() {
            chosen = chosen.push(caption("Click a cohort, or drag a box; 1 2 3 choose a line, C Caesar."));
        }
        for (name, commander) in &self.generals {
            let what = if *commander { "Right click to ride. Cohorts within his circle tire more slowly and hold longer; orders beyond his voice go by rider." } else { "A legate: the cohorts near him are steadied, a little." };
            chosen = chosen.push(heading(name.clone())).push(caption(what));
        }
        for (name, what, men, fatigue, heart) in self.chosen.iter().take(4) {
            chosen = chosen
                .push(row().spacing(10.0).push(heading(name.clone())).push(caption(what.clone())))
                .push(row().spacing(6.0).push(caption("Men")).push(bar(*men)).push(caption("Breath")).push(bar(1.0 - fatigue)).push(caption("Heart")).push(bar(*heart)));
        }
        if self.chosen.len() > 4 {
            chosen = chosen.push(caption(format!("and {} more", self.chosen.len() - 4)));
        }
        if self.riders_out > 0 {
            chosen = chosen.push(caption(format!("{} rider{} carrying orders", self.riders_out, if self.riders_out == 1 { "" } else { "s" })));
        }
        let orders = row()
            .spacing(8.0)
            .push(button("Advance (F)", Ask::Advance))
            .push(button("Halt (H)", Ask::Halt))
            .push(button("Relieve the line (R)", Ask::Relieve));
        let choose = row()
            .spacing(8.0)
            .push(button("Line 1", Ask::Line(1)))
            .push(button("Line 2", Ask::Line(2)))
            .push(button("Line 3", Ask::Line(3)))
            .push(button("Caesar", Ask::Caesar))
            .push(button(if self.paused { "Go on" } else { "Pause" }, Ask::Pause))
            .push(button("-", Ask::Slower))
            .push(button("+", Ask::Faster));
        let orders = anchored(Anchor::BottomLeft, panel(column().spacing(8.0).push(chosen).push(orders).push(choose)));
        stack().push(state).push(orders).into()
    }
}

fn read_out(world: &World, hud: &mut Hud) {
    let field = world.resource::<Field>();
    let chosen = world.resource::<Chosen>();
    let b = &field.battle;
    hud.time = b.time;
    hud.romans = b.standing(Side::Rome);
    hud.gauls = b.standing(Side::Gaul);
    hud.paused = field.paused;
    hud.speed = field.speed;
    hud.outcome = match b.outcome {
        Outcome::Fighting if b.time < b.gauls_go => "The Gauls are forming up.".to_owned(),
        Outcome::Fighting => String::new(),
        Outcome::Won => "Victory: the Gauls are routed.".to_owned(),
        Outcome::Lost => "Defeat: the legion is broken.".to_owned(),
    };
    hud.news = b.news.iter().rev().take(4).map(|(t, s)| format!("{}:{:02}  {s}", (*t / 60.0) as u32, *t as u32 % 60)).collect();
    hud.chosen = chosen
        .units
        .iter()
        .map(|&u| {
            let unit = &b.units[u];
            let what = if unit.routed {
                "routed".to_owned()
            } else if unit.fighting() {
                format!("line {}, fighting", unit.line)
            } else {
                match unit.order {
                    battle::Order::Hold => format!("line {}, holding", unit.line),
                    battle::Order::Advance => format!("line {}, advancing", unit.line),
                    battle::Order::MoveTo { .. } => format!("line {}, marching", unit.line),
                }
            };
            (unit.name.clone(), what, unit.strength(), unit.fatigue, unit.cohesion.max(0.0))
        })
        .collect();
    hud.generals = chosen.riders.iter().map(|&r| (b.riders[r].name.clone(), b.riders[r].commander)).collect();
    hud.riders_out = b.messages.iter().filter(|m| m.rider.is_some()).count();
}

fn carry_out(world: &mut World) {
    let Some(host) = world.get_resource_mut::<UiHost<Hud>>() else {
        return;
    };
    let asked = std::mem::take(&mut host.app().asked);
    for ask in asked {
        let units = world.resource::<Chosen>().units.clone();
        match ask {
            Ask::Advance => advance(&mut world.resource_mut::<Field>().battle, &units),
            Ask::Halt => halt(&mut world.resource_mut::<Field>().battle, &units),
            Ask::Relieve => world.resource_mut::<Field>().battle.relieve_the_line(),
            Ask::Line(line) => {
                let in_line = world.resource::<Field>().battle.romans_in_line(line);
                let chosen = world.resource_mut::<Chosen>();
                chosen.units = in_line;
                chosen.riders.clear();
            }
            Ask::Caesar => {
                let at = world.resource::<Field>().battle.riders[0].pos;
                let chosen = world.resource_mut::<Chosen>();
                chosen.units.clear();
                chosen.riders = vec![0];
                world.resource_mut::<View>().focus = at;
            }
            Ask::Pause => {
                let field = world.resource_mut::<Field>();
                field.paused = !field.paused;
            }
            Ask::Faster => {
                let field = world.resource_mut::<Field>();
                field.speed = (field.speed * 2.0).min(8.0);
            }
            Ask::Slower => {
                let field = world.resource_mut::<Field>();
                field.speed = (field.speed * 0.5).max(0.25);
            }
        }
    }
}

// ---- Screenshots, for looking at it without a person -------------------------------------------

/// For `poke`: plays a person's mouse into the window, as a check that choosing and ordering
/// work: a box dragged round the right of the field, then a right click ahead.
fn poke(mut played: ResMut<mira::input::InjectedInput>) {
    use mira::input::Played;
    let (from, to, there) = (Vec2::new(820.0, 380.0), Vec2::new(1560.0, 880.0), Vec2::new(1100.0, 300.0));
    played.play_after(20, Played::Cursor(from));
    played.play_after(22, Played::Button(MouseButton::Left, true));
    for k in 1..=6 {
        played.play_after(22 + k, Played::Cursor(from.lerp(to, k as f32 / 6.0)));
    }
    played.play_after(30, Played::Button(MouseButton::Left, false));
    played.play_after(34, Played::Cursor(there));
    played.play_after(36, Played::Button(MouseButton::Right, true));
    played.play_after(38, Played::Button(MouseButton::Right, false));
}

fn take_shots(world: &mut World) {
    let time = world.resource::<Field>().battle.time;
    let due: Vec<String> = {
        let shots = world.resource_mut::<Shots>();
        let due = shots.at.iter().filter(|(t, _)| *t <= time).map(|(_, p)| p.clone()).collect();
        shots.at.retain(|(t, _)| *t > time);
        due
    };
    if let Some(path) = due.into_iter().next() {
        world.resource_mut::<Screenshot>().request(path);
    }
    let quit = world.resource::<Shots>().quit_after.is_some_and(|q| time >= q);
    if quit {
        world.resource_mut::<Events<AppExit>>().send(AppExit);
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |key: &str| args.iter().find_map(|a| a.strip_prefix(&format!("{key}="))).map(str::to_owned);
    let watch = args.iter().any(|a| a == "watch");
    let cohort: usize = value("cohort").and_then(|v| v.parse().ok()).unwrap_or(360);
    let band: usize = value("band").and_then(|v| v.parse().ok()).unwrap_or(800);
    let mut shots = Shots::default();
    for a in args.iter().filter_map(|a| a.strip_prefix("shot=")) {
        if let Some((path, at)) = a.rsplit_once('@') {
            shots.at.push((at.parse().unwrap_or(0.0), path.to_owned()));
        }
    }
    shots.quit_after = value("quit").and_then(|v| v.parse().ok());
    let speed = value("speed").and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let height = value("zoom").and_then(|v| v.parse().ok()).unwrap_or(230.0);
    let focus = value("look")
        .and_then(|v| v.split_once(',').and_then(|(x, z)| Some(Vec2::new(x.parse().ok()?, z.parse().ok()?))))
        .unwrap_or(Vec2::new(0.0, -50.0));
    let (width, tall) = value("size").and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))).unwrap_or((1600, 900));

    let mut app = App::new();
    app.insert_resource(WindowSettings { title: "Triplex acies".into(), width, height: tall, ..Default::default() })
        .insert_resource(RayTracingSettings { enabled: false, ..Default::default() })
        .insert_resource(PostProcess { taa: false, bloom: 0.0, vignette: 0.0, grain: 0.0, sharpen: 0.0, ..Default::default() })
        .add_plugins(DefaultPlugins)
        // Each sprite in the colours it was rendered with, lit as it was in Blender.
        .insert_resource(ViewMode::Unlit)
        .insert_resource(Field { battle: Battle::new(49, cohort, band), owed: 0.0, speed, paused: false, watch, watch_relieved: -100.0 })
        .insert_resource(Chosen::default())
        .insert_resource(View { focus, height, follow: args.iter().any(|a| a == "follow") })
        .insert_resource(shots)
        .add_plugins(UiPlugin::new(Hud::default).sync(read_out))
        .add_systems(Stage::Startup, setup)
        .add_systems(Stage::First, carry_out)
        .add_systems(Stage::Update, (control, step, show_men, show_riders, marks, place_camera).chain())
        .add_systems(Stage::Last, take_shots);
    if args.iter().any(|a| a == "poke") {
        app.add_systems(Stage::Startup, poke);
    }
    let _ = STEPS_A_SECOND;
    app.run()
}
