//! Tail Lights: you are a squirrel, and the acorns are across the road.
//!
//! ```sh
//! cargo run --release --example tail_lights
//! cargo run --release --example tail_lights -- quiet    # without the sound
//! cargo run --release --example tail_lights -- watch    # it plays itself, to be looked at
//! ```
//!
//! Seen from straight above. Hop with W A S D or the arrow keys. Reach the far verge and the
//! next road is wider and faster. A car is the end of you, and of your run: back to the
//! first road, with nothing. Tab shows the score; R starts over. The best you have done is
//! kept from one day to the next.
//!
//! Everything seen is drawn a pixel at a time in `art.rs`, and everything heard is made
//! there from nothing: the tune, the hops, and the splat.
//!
//! It is also the engine's newer parts at work: the keys are [`Actions`], bound by name;
//! each car hums from where it is; and what is kept is in the folder [`SaveGames`] has for
//! the game.

mod art;

use mira::prelude::*;

/// How wide and deep a square of the world is, in metres: the squirrel hops one at a time.
const SQUARE: f32 = 1.2;
/// How far across the squirrel may go, in squares either side of the middle.
const SIDE: i32 = 7;
/// Where cars go off one end and come on at the other, in metres from the middle.
const CARS_TURN_BACK: f32 = 15.6;
/// How long a hop takes.
const HOP_SECONDS: f32 = 0.11;
/// How long the squirrel lies there before the run begins again.
const DEAD_SECONDS: f32 = 1.5;
/// How high above the ground each kind of thing is drawn: what is higher is over what is
/// lower.
const SPLAT_UP: f32 = 0.02;
const ACORN_UP: f32 = 0.03;
const CAR_UP: f32 = 0.2;
const SQUIRREL_UP: f32 = 0.3;
const TREE_UP: f32 = 0.5;
const BOARD_UP: f32 = 4.0;

/// A row of the world, counted from the verge the squirrel starts on: rows up to nought are
/// grass, rows 1 to `lanes` are road, and after them is the far verge.
fn row_z(row: i32) -> f32 {
    -(row as f32) * SQUARE
}

/// How the cars in one lane go.
struct Lane {
    /// Metres a second, to the right if more than nought.
    speed: f32,
    /// Metres from the front of one to the front of the next.
    spacing: f32,
    /// Whether they are lorries.
    lorries: bool,
}

/// How hard a road is: the further on, the more lanes, the faster, and the closer together.
fn lanes_of(level: u32) -> Vec<Lane> {
    let count = (2 + level).min(9);
    (0..count)
        .map(|lane| {
            // The same road every time it is played, and no two lanes alike.
            let seed = (level * 31 + lane * 17) as f32;
            let vary = |by: f32| (seed * by).sin() * 0.5 + 0.5;
            let fast = 2.2 + level as f32 * 0.45 + vary(1.7) * (1.5 + level as f32 * 0.25);
            let lorries = vary(2.9) > 0.75;
            let length = if lorries { 4.4 } else { 2.4 };
            Lane {
                speed: fast.min(13.0) * if lane % 2 == 0 { 1.0 } else { -1.0 },
                spacing: (length + 3.4 + vary(4.3) * 5.0 - (level as f32 * 0.18).min(1.6)).max(length + 2.8),
                lorries,
            }
        })
        .collect()
}

/// Where the game has got to.
struct Game {
    level: u32,
    score: u32,
    best_level: u32,
    best_score: u32,
    /// The squirrel's square: how many across from the middle, and its row.
    cell: (i32, i32),
    /// The furthest row reached on this road, which is what scores.
    furthest: i32,
    /// A hop under way: where from, where to, and how far through.
    hop: Option<(Vec3, Vec3, f32)>,
    /// Which way it faces, as a turn about the upright.
    facing: f32,
    /// Seconds it has lain dead, if it is.
    dead: Option<f32>,
    /// The road is to be laid out again, for the level as it now is.
    lay_out: bool,
    lanes: i32,
    /// Whether the score board is up.
    board: bool,
    quiet: bool,
    /// It plays itself.
    watch: bool,
}

impl Game {
    fn place(&self) -> Vec3 {
        Vec3::new(self.cell.0 as f32 * SQUARE, 0.0, row_z(self.cell.1))
    }

    fn back_to_start(&mut self) {
        (self.cell, self.furthest, self.hop, self.facing, self.dead) = ((0, 0), 0, None, 0.0, None);
    }

    /// Notes the best that has been done, and keeps it where it will be found next time.
    fn keep_best(&mut self) {
        if self.score <= self.best_score && self.level <= self.best_level {
            return;
        }
        (self.best_score, self.best_level) = (self.best_score.max(self.score), self.best_level.max(self.level));
        let file = best_file();
        let kept = file.parent().map(std::fs::create_dir_all).and_then(Result::ok).and_then(|()| std::fs::write(&file, format!("{} {}", self.best_level, self.best_score)).ok());
        if kept.is_none() {
            log::warn!("the best run could not be kept in {}", file.display());
        }
    }
}

/// Where the best run is kept: the road reached, and the score.
fn best_file() -> std::path::PathBuf {
    SaveGames::for_game("Tail Lights").folder().join("best.txt")
}

/// The pictures and sounds, made once.
struct Made {
    tile: Handle<Mesh>,
    grass: [Handle<Image>; 3],
    road: [Handle<Image>; 2],
    dashed: Handle<Image>,
    squirrel: [Handle<Image>; 2],
    splat: [Handle<Image>; 4],
    cars: Vec<Handle<Image>>,
    lorries: Vec<Handle<Image>>,
    tree: Handle<Image>,
    acorn: Handle<Image>,
    board: Handle<Image>,
    hop: Handle<Sound>,
    across: Handle<Sound>,
    squash: Handle<Sound>,
    engine: Handle<Sound>,
}

/// A picture lying flat on the ground: what everything in the game is. `size` is how wide
/// and how deep it lies, the top of the picture toward the far verge; `turn` turns it about.
fn flat(made: &Made, picture: Handle<Image>, at: Vec3, size: Vec2, turn: f32) -> (Transform, Mesh3d, Material) {
    (
        // The engine's plane has a picture's top along its own x: a quarter turn stands it
        // the way a picture is looked at from above.
        Transform::from_translation(at).with_rotation(Quat::from_rotation_y(turn + std::f32::consts::FRAC_PI_2)).with_scale(Vec3::new(size.y, 1.0, size.x)),
        Mesh3d(made.tile),
        Material { base_color_texture: Some(picture), alpha_cutoff: Some(0.5), roughness: 1.0, ..Default::default() },
    )
}

#[derive(Component)]
struct Squirrel;

/// A car: how fast it goes (to the right if more than nought) and half its length.
#[derive(Component)]
struct Car {
    speed: f32,
    half_length: f32,
}

/// Something of the road as it is laid out for a level, cleared away for the next.
#[derive(Component)]
struct OfTheLevel;

/// Something of the score board, cleared away when it is put down or written afresh.
#[derive(Component)]
struct OfTheBoard;

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut images: ResMut<Assets<Image>>, mut sounds: ResMut<Assets<Sound>>, mut actions: ResMut<Actions>, audio: Res<Audio>, game: Res<Game>) {
    actions.bind("up", KeyCode::KeyW).bind("up", KeyCode::ArrowUp);
    actions.bind("down", KeyCode::KeyS).bind("down", KeyCode::ArrowDown);
    actions.bind("left", KeyCode::KeyA).bind("left", KeyCode::ArrowLeft);
    actions.bind("right", KeyCode::KeyD).bind("right", KeyCode::ArrowRight);
    actions.bind("restart", KeyCode::KeyR).bind("score", KeyCode::Tab);
    if game.quiet {
        audio.set_master_volume(0.0);
    }
    // The tune goes round for as long as the game is played.
    let tune = art::tune();
    let loudest = |sound: &Sound| sound.samples.iter().fold(0.0f32, |most, sample| most.max(sample.abs()));
    log::info!("the tune is {:.1} s long and {:.2} at its loudest; the splat {:.2} s and {:.2}", tune.seconds(), loudest(&tune), art::splat_sound().seconds(), loudest(&art::splat_sound()));
    audio.play_with(&tune, Playback { volume: 0.5, looping: true, ..Default::default() });

    let (green, tarmac) = ([86, 150, 70], [58, 60, 68]);
    let paints = [[204, 44, 40], [44, 104, 196], [236, 190, 50], [226, 226, 232], [48, 150, 90], [156, 76, 184], [240, 130, 40]];
    let [sitting, hopping] = art::squirrel();
    let made = Made {
        tile: meshes.add(Mesh::plane(1.0)),
        grass: [0, 1, 2].map(|kind| images.add(art::tile(kind, green, [62, 122, 58], [124, 180, 90], false))),
        road: [3, 4].map(|kind| images.add(art::tile(kind, tarmac, [46, 48, 56], [74, 76, 84], false))),
        dashed: images.add(art::tile(5, tarmac, [46, 48, 56], [74, 76, 84], true)),
        squirrel: [images.add(sitting), images.add(hopping)],
        splat: art::splat().map(|frame| images.add(frame)),
        cars: paints.iter().map(|paint| images.add(art::car(*paint))).collect(),
        lorries: paints.iter().map(|paint| images.add(art::lorry(*paint))).collect(),
        tree: images.add(art::tree()),
        acorn: images.add(art::acorn()),
        board: images.add(art::board()),
        hop: sounds.add(art::hop_sound()),
        across: sounds.add(art::across_sound()),
        squash: sounds.add(art::splat_sound()),
        engine: sounds.add(art::engine_sound()),
    };
    commands.spawn((flat(&made, made.squirrel[0], Vec3::Y * SQUIRREL_UP, Vec2::splat(1.5), 0.0), Squirrel));
    commands.insert_resource(made);
    // Straight down, with the far verge to the top of the picture and no lessening with
    // distance: a flat picture of a flat world.
    let camera = Camera { orthographic_height: Some(13.2), far: 60.0, ..Default::default() };
    commands.spawn((Transform::from_xyz(0.0, 20.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z), camera, AudioListener));
}

/// Lays the road out for the level: the verges, the lanes with their markings and their
/// cars, trees along both verges, and an acorn for each road reached.
fn lay_out(mut commands: Commands, mut game: ResMut<Game>, made: Option<Res<Made>>, old: Query<Entity, With<OfTheLevel>>) {
    let Some(made) = made.filter(|_| game.lay_out) else { return };
    game.lay_out = false;
    for entity in &old {
        commands.entity(entity).despawn_recursive();
    }
    let lanes = lanes_of(game.level);
    game.lanes = lanes.len() as i32;
    let far = game.lanes + 1;
    // A number that is always the same for the same square: which tile, where a tree is.
    let chance = |x: i32, row: i32, of: u32| ((x * 7919 + row * 104_729 + of as i32 * 31).rem_euclid(1009)) as f32 / 1009.0;

    // The ground, a tile to a square.
    let wide = (CARS_TURN_BACK / SQUARE).ceil() as i32 + 1;
    for row in -5..=far + 7 {
        for x in -wide..=wide {
            let at = Vec3::new(x as f32 * SQUARE, 0.0, row_z(row));
            let picture = if (1..=game.lanes).contains(&row) {
                // The line between two lanes is along the upper edge of the nearer one's tiles.
                if row < game.lanes && x.rem_euclid(2) == 0 { made.dashed } else { made.road[(chance(x, row, 1) * 2.0) as usize % 2] }
            } else {
                made.grass[(chance(x, row, 2) * 3.0) as usize % 3]
            };
            commands.spawn((flat(&made, picture, at, Vec2::splat(SQUARE), 0.0), OfTheLevel));
        }
    }
    // Trees on both verges, clear of the squares the squirrel begins and ends on.
    for row in (-5..=-2).chain(far + 2..=far + 7) {
        for x in -wide..=wide {
            if chance(x, row, 3) < 0.2 {
                let at = Vec3::new(x as f32 * SQUARE + chance(x, row, 4) * 0.5, TREE_UP, row_z(row) + chance(x, row, 5) * 0.4);
                commands.spawn((flat(&made, made.tree, at, Vec2::splat(1.7 + chance(x, row, 6) * 0.7), 0.0), OfTheLevel));
            }
        }
    }
    // An acorn on the far verge for each road reached.
    let acorns = game.level.min(13) as i32;
    for acorn in 0..acorns {
        let at = Vec3::new((acorn as f32 - (acorns - 1) as f32 * 0.5) * 0.7, ACORN_UP, row_z(far));
        commands.spawn((flat(&made, made.acorn, at, Vec2::splat(0.6), 0.0), OfTheLevel));
    }
    // The cars.
    for (lane, how) in lanes.iter().enumerate() {
        let z = row_z(lane as i32 + 1);
        let length = if how.lorries { 4.4 } else { 2.4 };
        let cars = (CARS_TURN_BACK * 2.0 / how.spacing).floor().max(1.0) as usize;
        for car in 0..cars {
            let x = -CARS_TURN_BACK + car as f32 * how.spacing + (lane as f32 * 2.7).sin().abs() * how.spacing;
            let paint = (lane * 3 + car * 5 + game.level as usize) % made.cars.len();
            let picture = if how.lorries { made.lorries[paint] } else { made.cars[paint] };
            // Drawn driving to the right; turned about to drive to the left.
            let turn = if how.speed > 0.0 { 0.0 } else { std::f32::consts::PI };
            commands.spawn((
                flat(&made, picture, Vec3::new(x, CAR_UP, z), Vec2::new(length, 1.2), turn),
                Car { speed: how.speed, half_length: length * 0.5 },
                // A car hums, heard from where it is: louder as it comes, on the side it is on.
                AudioSource::new(made.engine).looping().heard_within(6.0).with_volume(0.16),
                OfTheLevel,
            ));
        }
    }
    log::info!("road {}: {} lanes", game.level, game.lanes);
}

fn drive(time: Res<Time>, game: Res<Game>, mut cars: Query<(&Car, &mut Transform)>) {
    // The road stands still while the squirrel lies in it.
    if game.dead.is_some() {
        return;
    }
    for (car, mut at) in &mut cars {
        at.translation.x += car.speed * time.delta_secs();
        // Off one end, on at the other.
        if at.translation.x > CARS_TURN_BACK {
            at.translation.x -= CARS_TURN_BACK * 2.0;
        } else if at.translation.x < -CARS_TURN_BACK {
            at.translation.x += CARS_TURN_BACK * 2.0;
        }
    }
}

/// Hops when a key says so, carries a hop through, keeps the score, and notices the far
/// verge.
fn hop(time: Res<Time>, actions: Res<Actions>, audio: Res<Audio>, sounds: Res<Assets<Sound>>, made: Res<Made>, mut game: ResMut<Game>) {
    let play = |sound: Handle<Sound>, volume: f32| {
        if let Some(sound) = sounds.get(sound) {
            audio.play_with(sound, Playback { volume, ..Default::default() });
        }
    };
    if actions.just_pressed("score") {
        game.board = !game.board;
    }
    if actions.just_pressed("restart") {
        (game.level, game.score, game.lay_out) = (1, 0, true);
        game.back_to_start();
        return;
    }
    if let Some(lain) = game.dead {
        game.dead = Some(lain + time.delta_secs());
        if lain >= DEAD_SECONDS {
            // The run is over: the first road again, with nothing.
            (game.level, game.score, game.lay_out) = (1, 0, true);
            game.back_to_start();
        }
        return;
    }
    if let Some((from, to, through)) = game.hop {
        let through = through + time.delta_secs() / HOP_SECONDS;
        game.hop = (through < 1.0).then_some((from, to, through));
        if game.hop.is_some() {
            return;
        }
        // Landed. Ten for every row further than it has been on this road.
        if game.cell.1 > game.furthest {
            game.furthest = game.cell.1;
            game.score += 10;
        }
        // On the far verge the road is crossed: fifty for each road it is, and the next.
        if game.cell.1 > game.lanes {
            game.score += 50 * game.level;
            game.level += 1;
            game.keep_best();
            play(made.across, 0.5);
            game.lay_out = true;
            game.back_to_start();
            return;
        }
        game.keep_best();
    }
    // A key just pressed hops at once; one held hops again as each hop ends.
    let wanted = [("up", (0, 1)), ("down", (0, -1)), ("left", (-1, 0)), ("right", (1, 0))];
    let pressed = wanted.iter().find(|(action, _)| actions.just_pressed(action)).or_else(|| wanted.iter().find(|(action, _)| actions.pressed(action)));
    let Some((_, (across, forward))) = pressed else { return };
    let to = (game.cell.0 + across, game.cell.1 + forward);
    // Not off the sides, and not back past the verge it started on.
    if to.0.abs() > SIDE || to.1 < -1 {
        return;
    }
    let from = game.place();
    game.cell = to;
    game.facing = f32::atan2(-(*across as f32), *forward as f32);
    game.hop = Some((from, game.place(), 0.0));
    play(made.hop, 0.35);
}

/// Puts the squirrel where the game says it is, in the picture that fits what it is doing,
/// and ends it where a car is.
fn squirrel(
    mut game: ResMut<Game>,
    audio: Res<Audio>,
    sounds: Res<Assets<Sound>>,
    made: Res<Made>,
    cars: Query<(&Car, &Transform), Without<Squirrel>>,
    mut squirrels: Query<(&mut Transform, &mut Material), With<Squirrel>>,
) {
    let Some((mut at, mut look)) = squirrels.iter_mut().next() else { return };
    let (place, stretched) = match game.hop {
        Some((from, to, through)) => (from.lerp(to, through), (through * std::f32::consts::PI).sin()),
        None => (game.place(), 0.0),
    };
    // Dead, it is the splat, a moment at a time: spreading, and then lying there.
    let (picture, size, up) = match game.dead {
        Some(lain) => {
            let frame = ((lain / 0.07) as usize).min(made.splat.len() - 1);
            (made.splat[frame], 1.5 + 0.7 * (lain / 0.21).min(1.0), SPLAT_UP)
        }
        // A hop is drawn stretched out, and a little larger at its height, as if nearer.
        None => (made.squirrel[(stretched > 0.2) as usize], 1.5 + 0.22 * stretched, SQUIRREL_UP),
    };
    *at = flat(&made, picture, Vec3::new(place.x, up, place.z), Vec2::splat(size), game.facing).0;
    if look.base_color_texture != Some(picture) {
        look.base_color_texture = Some(picture);
    }
    if game.dead.is_some() {
        return;
    }
    let hit = cars.iter().any(|(car, car_at)| (car_at.translation.x - place.x).abs() < car.half_length + 0.3 && (car_at.translation.z - place.z).abs() < 0.5 + 0.3);
    if hit {
        game.dead = Some(0.0);
        game.hop = None;
        game.keep_best();
        if let Some(sound) = sounds.get(made.squash) {
            audio.play_with(sound, Playback { volume: 0.9, ..Default::default() });
        }
        log::info!("splat on road {}, with {}", game.level, game.score);
    }
}

/// The camera keeps the squirrel a little below the middle of the picture and comes to it
/// smoothly, shakes when it is hit, and the title says how the run is going.
fn follow(time: Res<Time>, game: Res<Game>, window: Option<Res<Window>>, mut cameras: Query<&mut Transform, With<Camera>>, mut shown: Local<(u32, u32, u32)>) {
    let wanted = Vec3::new(0.0, 20.0, game.place().z - 3.4);
    // A jolt as the car goes over, dying away in a third of a second.
    let jolt = game.dead.filter(|lain| *lain < 0.35).map_or(Vec3::ZERO, |lain| {
        let still = 1.0 - lain / 0.35;
        Vec3::new((lain * 71.0).sin(), 0.0, (lain * 93.0).cos()) * 0.22 * still
    });
    for mut at in &mut cameras {
        let ease = 1.0 - (-time.delta_secs() * 6.0).exp();
        let steady = at.translation.lerp(wanted, ease);
        at.translation = Vec3::new(steady.x, 20.0, steady.z) + jolt;
    }
    if *shown != (game.level, game.score, game.best_score) {
        *shown = (game.level, game.score, game.best_score);
        if let Some(window) = window {
            window.set_title(&format!("Tail Lights: road {}, score {}, best {} (Tab for the board)", game.level, game.score, game.best_score));
        }
    }
}

/// What the score board has had made for it: the letters so far, and what it last said.
#[derive(Default)]
struct Written {
    letters: Vec<(char, Handle<Image>)>,
    said: String,
}

/// The score board: put up and down with a key, and written afresh when what it says has
/// changed. It hangs over the middle of the picture, wherever the camera is.
fn board(mut commands: Commands, game: Res<Game>, made: Res<Made>, mut images: ResMut<Assets<Image>>, cameras: Query<&Transform, With<Camera>>, old: Query<Entity, With<OfTheBoard>>, mut kept: Local<Written>) {
    // The letters made so far, and what the board last said.
    let Written { letters, said: written } = &mut *kept;
    let lines = [format!("SCORE {}", game.score), format!("ROAD {}", game.level), format!("BEST {}", game.best_score), format!("BEST ROAD {}", game.best_level)];
    let Some(over) = cameras.iter().next().map(|camera| camera.translation) else { return };
    let saying = if game.board { format!("{}@{:.2},{:.2}", lines.join("/"), over.x, over.z) } else { String::new() };
    if *written == saying {
        return;
    }
    *written = saying;
    for entity in &old {
        commands.entity(entity).despawn_recursive();
    }
    if !game.board {
        return;
    }
    let middle = Vec3::new(over.x, BOARD_UP, over.z);
    commands.spawn((flat(&made, made.board, middle, Vec2::new(9.6, 6.0), 0.0), OfTheBoard));
    // A letter is three pixels by five, with a pixel between one and the next.
    let pixel = 0.16;
    for (row, line) in lines.iter().enumerate() {
        let across = line.chars().count() as f32 * 4.0 * pixel;
        for (place, of) in line.chars().enumerate() {
            let known = letters.iter().find(|(known, _)| *known == of).map(|(_, picture)| *picture);
            let picture = match known.or_else(|| art::letter(of).map(|picture| images.add(picture))) {
                Some(picture) => picture,
                None => continue,
            };
            if known.is_none() {
                letters.push((of, picture));
            }
            let at = middle + Vec3::new(-across * 0.5 + (place as f32 * 4.0 + 1.5) * pixel, 0.05, (row as f32 - 1.5) * 7.5 * pixel);
            commands.spawn((flat(&made, picture, at, Vec2::new(3.0 * pixel, 5.0 * pixel), 0.0), OfTheBoard));
        }
    }
}

/// Plays the game by itself, as a careful squirrel would: it presses the keys a person
/// would, so that everything a person's play goes through is gone through. It hops on when
/// the lane ahead will be clear for long enough, and back when the lane it is in will not.
fn watch_itself(game: Res<Game>, cars: Query<(&Car, &Transform)>, mut keys: ResMut<mira::input::InjectedInput>, mut waited: Local<u32>) {
    if !game.watch || game.dead.is_some() || game.hop.is_some() || game.lay_out {
        return;
    }
    // A moment between hops, as fingers have.
    *waited += 1;
    if *waited < 4 {
        return;
    }
    let across = game.cell.0 as f32 * SQUARE;
    // Whether a car will be over this place in a row within so many seconds.
    let clear = |row: i32, seconds: f32| {
        !cars.iter().any(|(car, at)| {
            let reach = car.half_length + 0.7;
            let (now, then) = (at.translation.x - across, at.translation.x + car.speed * seconds - across);
            (at.translation.z - row_z(row)).abs() < 0.5 && (now.abs() < reach || then.abs() < reach || now.signum() != then.signum())
        })
    };
    let key = if clear(game.cell.1 + 1, 0.55) {
        Some(KeyCode::KeyW)
    } else if !clear(game.cell.1, 0.4) && game.cell.1 > 0 && clear(game.cell.1 - 1, 0.4) {
        Some(KeyCode::KeyS)
    } else {
        None
    };
    if let Some(key) = key {
        keys.tap(key, 1);
        *waited = 0;
    }
}

fn main() -> anyhow::Result<()> {
    let asked = |word: &str| std::env::args().skip(1).any(|arg| arg == word);
    let kept = std::fs::read_to_string(best_file()).unwrap_or_default();
    let mut kept = kept.split_whitespace().map(|number| number.parse().unwrap_or(0));
    let (best_level, best_score) = (kept.next().unwrap_or(1).max(1), kept.next().unwrap_or(0));
    let game = Game {
        level: 1,
        score: 0,
        best_level,
        best_score,
        cell: (0, 0),
        furthest: 0,
        hop: None,
        facing: 0.0,
        dead: None,
        lay_out: true,
        lanes: 0,
        board: asked("board"),
        quiet: asked("quiet"),
        watch: asked("watch"),
    };
    let mut app = App::new();
    app.insert_resource(WindowSettings { title: "Tail Lights".into(), width: 1280, height: 800, ..Default::default() })
        .insert_resource(game)
        // A flat picture: every surface in its own colour, with no light or shadow on it,
        // nothing traced, and nothing smoothed from frame to frame.
        .insert_resource(RayTracingSettings { enabled: false, ..Default::default() })
        .insert_resource(PostProcess { taa: false, bloom: 0.0, vignette: 0.0, grain: 0.0, sharpen: 0.0, ..Default::default() })
        .add_plugins(DefaultPlugins)
        .insert_resource(ViewMode::Unlit)
        .add_systems(Stage::Startup, setup)
        .add_systems(Stage::First, watch_itself)
        .add_systems(Stage::Update, (lay_out, hop, drive, squirrel, follow, board));
    app.run()
}