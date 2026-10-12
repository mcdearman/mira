//! Many things at once: a field of cubes and balls, some of them moving, to measure what a
//! frame costs as their number grows. The roadmap's test for the renderer is 100,000 of them
//! at sixty frames a second.
//!
//! ```sh
//! cargo run --release --example instances                        # 100,000, a tenth moving
//! cargo run --release --example instances -- count=20000 moving=0.5 rays=off lights=1000
//! cargo run --release --example instances -- lods=on size=1920x1080
//! MIRA_HIDDEN=1 cargo run --release --example instances -- frames=300   # measure, say, and stop
//! ```
//!
//! With `frames=N` it runs that many frames, prints where the time of a frame went (the
//! stages, and the passes that draw it) and stops. Two times are told apart: the engine's
//! own on the processor, and the graphics card's drawing what it was handed. The slower of
//! the two is what the frame rate can reach.
//! Without it the field is there to look at: `mira-debug profile` and `mira-debug passes` say
//! the same of it while it runs.

use std::time::Duration;

use mira::prelude::*;
use mira::render::RenderGraph;

/// What the command line asked for.
struct Asked {
    count: usize,
    /// The part of them that moves each frame, 0 to 1.
    moving: f32,
    /// How many frames to run before saying what they cost and stopping; none to run on.
    frames: Option<u64>,
    /// Whether rays are traced where the card can. A card that cannot does not pay for it.
    rays: bool,
    /// Whether the sun casts shadows: without them, what the shadows cost is what is saved.
    shadows: bool,
    /// How many lamps stand over the field.
    lights: usize,
    /// The size of the picture: what the card takes grows with it where it is shading, and
    /// does not where it is the triangles that cost.
    size: (u32, u32),
    /// Whether the balls are drawn with fewer triangles the further away they are.
    lods: bool,
    /// How many spot lights shine down on the middle of the field, casting shadows.
    spots: usize,
    /// How many lanterns stand among the things in the middle of the field, shining every
    /// way and casting shadows.
    lanterns: usize,
}

/// A thing that bobs up and down about where it was put.
#[derive(Component)]
struct Bob {
    rest: f32,
    phase: f32,
}

fn asked() -> Asked {
    let mut asked = Asked { count: 100_000, moving: 0.1, frames: None, rays: true, shadows: true, lights: 0, size: (1280, 720), lods: false, spots: 0, lanterns: 0 };
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("count", value)) => asked.count = value.parse().expect("count=<how many>"),
            Some(("moving", value)) => asked.moving = value.parse::<f32>().expect("moving=<0 to 1>").clamp(0.0, 1.0),
            Some(("frames", value)) => asked.frames = Some(value.parse().expect("frames=<how many>")),
            Some(("rays", value)) => asked.rays = value != "off",
            Some(("shadows", value)) => asked.shadows = value != "off",
            Some(("lights", value)) => asked.lights = value.parse().expect("lights=<how many>"),
            Some(("lods", value)) => asked.lods = value != "off",
            Some(("spots", value)) => asked.spots = value.parse().expect("spots=<how many>"),
            Some(("lanterns", value)) => asked.lanterns = value.parse().expect("lanterns=<how many>"),
            Some(("size", value)) => {
                let (wide, high) = value.split_once('x').expect("size=<wide>x<high>");
                asked.size = (wide.parse().expect("size=<wide>x<high>"), high.parse().expect("size=<wide>x<high>"));
            }
            _ => panic!("not understood: {arg} (count=, moving=, frames=, rays=off, shadows=off, lights=, size=, lods=on, spots=, lanterns=)"),
        }
    }
    asked
}

/// Lays the things out in a square field, a few shapes in a few colours so that they fall
/// into a few dozen batches, as a scene built from a kit of parts does.
fn field(mut commands: Commands, mut server: ResMut<AssetServer>, mut meshes: ResMut<Assets<Mesh>>, asked: Res<Asked>) {
    let shapes = [
        server.cube(&mut meshes, 0.6),
        server.sphere(&mut meshes, 0.35),
        server.cube(&mut meshes, 0.4),
        server.sphere(&mut meshes, 0.25),
    ];
    let colours = [
        Color::rgb(0.85, 0.25, 0.2),
        Color::rgb(0.2, 0.45, 0.85),
        Color::rgb(0.9, 0.7, 0.2),
        Color::rgb(0.3, 0.7, 0.35),
        Color::rgb(0.75, 0.75, 0.78),
        Color::rgb(0.55, 0.3, 0.7),
    ];
    // A ball is 960 triangles close to, and at a distance is a few pixels across: with
    // `lods=on` it is drawn with fewer the further away it is.
    let coarser = [0.35f32, 0.25].map(|radius| [(12, 6), (6, 4)].map(|(sectors, stacks)| meshes.add(Mesh::uv_sphere(radius, sectors, stacks))));
    let side = (asked.count as f32).sqrt().ceil() as usize;
    let spacing = 1.2;
    let half = side as f32 * spacing * 0.5;
    // Every so many is one that moves, spread evenly through the field.
    let every = (asked.moving > 0.0).then(|| (1.0 / asked.moving).round().max(1.0) as usize);
    for i in 0..asked.count {
        let (column, row) = (i % side, i / side);
        let at = Vec3::new(column as f32 * spacing - half, 0.4, row as f32 * spacing - half);
        let material = Material { color: colours[(column + row * 3) % colours.len()], roughness: 0.6, ..Default::default() };
        let shape = (column * 7 + row) % shapes.len();
        let thing = if asked.lods && shape % 2 == 1 {
            let [middle, far] = coarser[shape / 2];
            let level = |max_distance, mesh| LodLevel { max_distance, mesh, material };
            commands.spawn((Transform::from_translation(at), Lods(vec![level(30.0, shapes[shape]), level(90.0, middle), level(f32::MAX, far)]))).id()
        } else {
            commands.spawn((Transform::from_translation(at), Mesh3d(shapes[shape]), material)).id()
        };
        if every.is_some_and(|every| i % every == 0) {
            commands.entity(thing).insert(Bob { rest: at.y, phase: i as f32 * 0.37 });
        }
    }
    // Lamps spread evenly over the field, each lighting the few things about it.
    let lamps_across = (asked.lights as f32).sqrt().ceil().max(1.0) as usize;
    let between = half * 2.0 / lamps_across as f32;
    for i in 0..asked.lights {
        let (column, row) = (i % lamps_across, i / lamps_across);
        let at = Vec3::new((column as f32 + 0.5) * between - half, 1.6, (row as f32 + 0.5) * between - half);
        let lamp = PointLight { color: colours[(column * 5 + row) % colours.len()], intensity: 12.0, range: (between * 0.75).clamp(3.0, 12.0), shadows: false };
        commands.spawn((Transform::from_translation(at), lamp));
    }
    // Spot lights in a ring over the middle of the field, each leaning outward a little, so
    // that the things under them throw shadows away from it.
    for i in 0..asked.spots {
        let turn = i as f32 / asked.spots as f32 * std::f32::consts::TAU;
        let out = Vec3::new(turn.cos(), 0.0, turn.sin());
        let at = out * 4.0 + Vec3::new(0.0, 7.0, 0.0);
        let lamp = SpotLight { color: colours[i % colours.len()], intensity: 400.0, range: 22.0, inner: 0.5, outer: 0.75, shadows: true };
        commands.spawn((Transform::from_translation(at).looking_at(out * 10.0, Vec3::Y), lamp));
    }
    // Lanterns between the things, a little above them, in a row across the middle.
    for i in 0..asked.lanterns {
        let across = (i as f32 - (asked.lanterns as f32 - 1.0) * 0.5) * 4.8 + 0.6;
        let lamp = PointLight { color: colours[(i + 2) % colours.len()], intensity: 30.0, range: 9.0, shadows: true };
        commands.spawn((Transform::from_xyz(across, 1.3, 0.6), lamp));
    }
    let ground = server.plane(&mut meshes, half * 2.0 + 20.0);
    commands.spawn((Transform::IDENTITY, Mesh3d(ground), Material { color: Color::rgb(0.3, 0.32, 0.35), roughness: 0.9, ..Default::default() }));
    // High enough, and far enough back, that most of the field is in sight.
    commands.spawn((
        Transform::from_xyz(0.0, half * 0.55 + 6.0, half * 1.25 + 8.0).looking_at(Vec3::new(0.0, 0.0, -half * 0.2), Vec3::Y),
        Camera::default(),
    ));
    commands.spawn((Transform::IDENTITY.looking_at(Vec3::new(-0.5, -1.0, -0.35), Vec3::Y), DirectionalLight { shadows: asked.shadows, ..Default::default() }));
    log::info!("{} things in a field {side} across, {} of them moving", asked.count, every.map_or(0, |every| asked.count.div_ceil(every)));
}

fn bob(time: Res<Time>, mut things: Query<(&Bob, &mut Transform)>) {
    let now = time.elapsed_secs();
    for (bob, mut at) in &mut things {
        at.translation.y = bob.rest + 0.3 * (now * 2.0 + bob.phase).sin();
    }
}

/// Once the frames asked for have run, says what a frame cost and stops the app. It runs
/// after the frame is drawn, when every stage before the drawing has been timed.
fn report(world: &mut World) {
    let Some(frames) = world.resource::<Asked>().frames else { return };
    if world.resource::<Time>().frame_count() < frames {
        return;
    }
    // The app runs a frame more before it stops.
    world.resource_mut::<Asked>().frames = None;
    let millis = |time: Duration| time.as_secs_f64() * 1e3;
    let stats = world.resource::<FrameStats>();
    let (mean, worst) = (stats.mean(), stats.worst());
    println!("\n{} things, over the last {} frames:", world.resource::<Asked>().count, stats.frames().count());
    let on_card = world.resource::<RenderGraph>().on_card();
    let slower = mean.max(on_card.unwrap_or_default());
    println!("  on the processor {:8.3} ms mean, {:8.3} ms worst", millis(mean), millis(worst));
    match on_card {
        Some(on_card) => println!("  on the card      {:8.3} ms", millis(on_card)),
        None => println!("  on the card      not told: this card does not time its passes"),
    }
    println!("  so {:.0} frames a second could be reached", 1.0 / slower.as_secs_f64().max(1e-9));
    let stats = world.resource::<FrameStats>();
    println!("  by stage, this frame:");
    let mut counted = Duration::ZERO;
    for (stage, took) in stats.stages() {
        counted += *took;
        if millis(*took) >= 0.01 {
            println!("    {:<12} {:8.3} ms", format!("{stage:?}"), millis(*took));
        }
    }
    // The drawing stage is still running, since this is in it.
    println!("    {:<12} {:8.3} ms  (the mean frame, less the stages above)", "Render", millis(mean.saturating_sub(counted)));
    if let Ok(passes) = world.resource_mut::<RenderGraph>().describe() {
        println!("  by pass, recording its work and then the card doing it:");
        for pass in passes {
            let (took, on_card) = (pass.took.unwrap_or_default(), pass.on_card.unwrap_or_default());
            if millis(took) >= 0.01 || millis(on_card) >= 0.01 {
                println!("    {:<16} {:8.3} ms {:8.3} ms", pass.name, millis(took), millis(on_card));
            }
        }
    }
    world.resource_mut::<Events<AppExit>>().send(AppExit);
}

fn main() -> anyhow::Result<()> {
    let asked = asked();
    let mut app = App::new();
    app.insert_resource(WindowSettings { title: "instances".into(), width: asked.size.0, height: asked.size.1, ..Default::default() })
        .insert_resource(RayTracingSettings { enabled: asked.rays, ..Default::default() })
        .add_plugins(DefaultPlugins)
        .insert_resource(asked)
        .add_systems(Stage::Startup, field)
        .add_systems(Stage::Update, bob)
        .add_systems(Stage::Render, report);
    app.run()
}
