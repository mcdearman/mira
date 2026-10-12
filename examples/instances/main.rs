//! Many things at once: a field of cubes and balls, some of them moving, to measure what a
//! frame costs as their number grows. The roadmap's test for the renderer is 100,000 of them
//! at sixty frames a second.
//!
//! ```sh
//! cargo run --release --example instances                        # 100,000, a tenth moving
//! cargo run --release --example instances -- count=20000 moving=0.5 rays=off
//! MIRA_HIDDEN=1 cargo run --release --example instances -- frames=300   # measure, say, and stop
//! ```
//!
//! With `frames=N` it runs that many frames, prints where the time of a frame went (the
//! stages, the costliest systems, and the passes that draw it) and stops. The times are the
//! engine's own on the processor, not the graphics card's and not the wait for the display.
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
}

/// A thing that bobs up and down about where it was put.
#[derive(Component)]
struct Bob {
    rest: f32,
    phase: f32,
}

fn asked() -> Asked {
    let mut asked = Asked { count: 100_000, moving: 0.1, frames: None, rays: true, shadows: true };
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("count", value)) => asked.count = value.parse().expect("count=<how many>"),
            Some(("moving", value)) => asked.moving = value.parse::<f32>().expect("moving=<0 to 1>").clamp(0.0, 1.0),
            Some(("frames", value)) => asked.frames = Some(value.parse().expect("frames=<how many>")),
            Some(("rays", value)) => asked.rays = value != "off",
            Some(("shadows", value)) => asked.shadows = value != "off",
            _ => panic!("not understood: {arg} (count=, moving=, frames=, rays=off, shadows=off)"),
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
    let side = (asked.count as f32).sqrt().ceil() as usize;
    let spacing = 1.2;
    let half = side as f32 * spacing * 0.5;
    // Every so many is one that moves, spread evenly through the field.
    let every = if asked.moving > 0.0 { (1.0 / asked.moving).round().max(1.0) as usize } else { usize::MAX };
    for i in 0..asked.count {
        let (column, row) = (i % side, i / side);
        let at = Vec3::new(column as f32 * spacing - half, 0.4, row as f32 * spacing - half);
        let material = Material { color: colours[(column + row * 3) % colours.len()], roughness: 0.6, ..Default::default() };
        let thing = commands.spawn((Transform::from_translation(at), Mesh3d(shapes[(column * 7 + row) % shapes.len()]), material)).id();
        if i % every == 0 {
            commands.entity(thing).insert(Bob { rest: at.y, phase: i as f32 * 0.37 });
        }
    }
    let ground = server.plane(&mut meshes, half * 2.0 + 20.0);
    commands.spawn((Transform::IDENTITY, Mesh3d(ground), Material { color: Color::rgb(0.3, 0.32, 0.35), roughness: 0.9, ..Default::default() }));
    // High enough, and far enough back, that most of the field is in sight.
    commands.spawn((
        Transform::from_xyz(0.0, half * 0.55 + 6.0, half * 1.25 + 8.0).looking_at(Vec3::new(0.0, 0.0, -half * 0.2), Vec3::Y),
        Camera::default(),
    ));
    commands.spawn((Transform::IDENTITY.looking_at(Vec3::new(-0.5, -1.0, -0.35), Vec3::Y), DirectionalLight { shadows: asked.shadows, ..Default::default() }));
    log::info!("{} things in a field {side} across, {} of them moving", asked.count, asked.count.div_ceil(every.min(asked.count.max(1))));
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
    println!("  a frame   {:8.3} ms mean, {:8.3} ms worst  ({:.0} a second could be reached)", millis(mean), millis(worst), 1.0 / mean.as_secs_f64().max(1e-9));
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
        println!("  by pass, recording its work:");
        for pass in passes {
            if let Some(took) = pass.took.filter(|took| millis(*took) >= 0.01) {
                println!("    {:<16} {:8.3} ms", pass.name, millis(took));
            }
        }
    }
    world.resource_mut::<Events<AppExit>>().send(AppExit);
}

fn main() -> anyhow::Result<()> {
    let asked = asked();
    let mut app = App::new();
    app.insert_resource(RayTracingSettings { enabled: asked.rays, ..Default::default() })
        .add_plugins(DefaultPlugins)
        .insert_resource(asked)
        .add_systems(Stage::Startup, field)
        .add_systems(Stage::Update, bob)
        .add_systems(Stage::Render, report);
    app.run()
}
