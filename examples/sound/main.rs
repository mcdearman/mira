//! Sound: a few notes played at once, and a hum that goes round the listener.
//!
//! ```sh
//! cargo run --example sound             # listen
//! cargo run --example sound -- quiet    # the same with the volume at nothing
//! MIRA_HIDDEN=1 cargo run --example sound -- quiet seconds=3   # say how it went, and stop
//! ```

use mira::prelude::*;

/// What the command line asked for.
struct Asked {
    quiet: bool,
    /// How long to run before saying how it went and stopping; none to run on.
    seconds: Option<f32>,
}

/// The thing that hums, and the notes of the tune not yet played, each with when it is due.
#[derive(Component)]
struct Hummer;

struct Tune {
    notes: Vec<(f32, Handle<Sound>)>,
    played: usize,
    ended: usize,
    voices: Vec<mira::audio::VoiceId>,
}

fn setup(mut commands: Commands, mut server: ResMut<AssetServer>, mut meshes: ResMut<Assets<Mesh>>, mut sounds: ResMut<Assets<Sound>>, audio: Res<Audio>, asked: Res<Asked>) {
    if asked.quiet {
        audio.set_master_volume(0.0);
    }
    // A rising scale, a note every quarter of a second.
    let scale = [261.63, 293.66, 329.63, 349.23, 392.0, 440.0, 493.88, 523.25];
    let notes = scale.iter().enumerate().map(|(at, hertz)| (0.5 + at as f32 * 0.25, sounds.add(Sound::tone(*hertz, 0.22)))).collect();
    commands.insert_resource(Tune { notes, played: 0, ended: 0, voices: Vec::new() });

    // A ball that hums, heard from where it is as it goes round.
    let hum = sounds.add(Sound::tone(110.0, 2.0));
    let ball = server.sphere(&mut meshes, 0.3);
    commands.spawn((
        Transform::from_xyz(4.0, 1.0, 0.0),
        Mesh3d(ball),
        Material { color: Color::rgb(0.9, 0.6, 0.2), ..Default::default() },
        AudioSource::new(hum).looping().heard_within(12.0).with_volume(0.6),
        Hummer,
    ));
    let ground = server.plane(&mut meshes, 30.0);
    commands.spawn((Transform::IDENTITY, Mesh3d(ground), Material { color: Color::rgb(0.3, 0.32, 0.35), roughness: 0.9, ..Default::default() }));
    commands.spawn((Transform::from_xyz(0.0, 1.6, 0.0).looking_at(Vec3::new(0.0, 1.0, -4.0), Vec3::Y), Camera::default(), AudioListener));
    commands.spawn((Transform::IDENTITY.looking_at(Vec3::new(-0.5, -1.0, -0.35), Vec3::Y), DirectionalLight::default()));
}

fn play(time: Res<Time>, audio: Res<Audio>, sounds: Res<Assets<Sound>>, mut tune: ResMut<Tune>, mut hummers: Query<&mut Transform, With<Hummer>>) {
    let now = time.elapsed_secs();
    while tune.played < tune.notes.len() && tune.notes[tune.played].0 <= now {
        let note = tune.notes[tune.played].1;
        if let Some(sound) = sounds.get(note) {
            let voice = audio.play_with(sound, Playback { volume: 0.5, ..Default::default() });
            tune.voices.push(voice);
        }
        tune.played += 1;
    }
    tune.ended = tune.voices.iter().filter(|voice| !audio.is_playing(**voice)).count();
    for mut at in &mut hummers {
        at.translation = Vec3::new((now * 0.8).cos() * 4.0, 1.0, (now * 0.8).sin() * 4.0);
    }
}

/// Once the time asked for is up, says how it went and stops.
fn report(time: Res<Time>, audio: Res<Audio>, tune: Res<Tune>, mut asked: ResMut<Asked>, mut exit: EventWriter<AppExit>) {
    let Some(seconds) = asked.seconds else { return };
    if time.elapsed_secs() < seconds {
        return;
    }
    // The app runs a frame more before it stops.
    asked.seconds = None;
    println!("heard by a sound output: {}", audio.is_heard());
    println!("notes begun: {} of {}, ended: {}", tune.played, tune.notes.len(), tune.ended);
    println!("still playing: {}", audio.playing());
    exit.send(AppExit);
}

fn main() -> anyhow::Result<()> {
    let mut asked = Asked { quiet: false, seconds: None };
    for arg in std::env::args().skip(1) {
        match arg.split_once('=') {
            Some(("seconds", value)) => asked.seconds = Some(value.parse().expect("seconds=<how long>")),
            None if arg == "quiet" => asked.quiet = true,
            _ => panic!("not understood: {arg} (quiet, seconds=)"),
        }
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .insert_resource(asked)
        .add_systems(Stage::Startup, setup)
        .add_systems(Stage::Update, (play, report));
    app.run()
}