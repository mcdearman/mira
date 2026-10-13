//! Sound.
//!
//! A [`Sound`] is read from a file, or made, and kept in `Assets<Sound>`. It is played by
//! giving an entity an [`AudioSource`], which plays for as long as the entity has it and is
//! heard from where the entity is; or at once, through [`Audio`], for a sound that belongs
//! to nothing: a click in a menu, a shot. Music, which is long, is read from its file as it
//! plays ([`Audio::play_music`]).
//!
//! What is heard is heard by the [`AudioListener`], or by the camera where there is none: a
//! source with a `range` is quieter the further off it is, silent at its range, and louder
//! in the ear it is on the side of.
//!
//! Everything playing is mixed in one place (`mixer`), which the machine's sound output asks
//! for a few milliseconds at a time. Where there is no output, or `MIRA_AUDIO=0`, the mixer
//! is run from the frame instead, to no one: sounds still begin, last and end as they would.

mod device;
mod mixer;
mod sound;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use glam::Vec3;

pub use mixer::VoiceId;
pub use sound::Sound;

use crate::{
    app::{App, Plugin, Stage},
    assets::{Assets, Handle},
    ecs::{Component, Entity, Query, Res, ResMut, With, World},
    render::Camera,
    time::Time,
    transform::GlobalTransform,
};
use mixer::{Mixer, Source};

/// How a sound is played.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playback {
    /// How loud: 1 as it was recorded.
    pub volume: f32,
    /// How fast, and so how high: 1 as it was recorded, 2 twice as fast and an octave up.
    pub pitch: f32,
    /// Whether it begins again when it ends, until it is stopped.
    pub looping: bool,
    /// Where between the ears: -1 the left, 0 both alike, 1 the right.
    pub pan: f32,
}

impl Default for Playback {
    fn default() -> Self {
        Self { volume: 1.0, pitch: 1.0, looping: false, pan: 0.0 }
    }
}

/// How loud a sound is in each ear, from how loud it is and where between them: as much
/// sound in all wherever it is.
fn ears(volume: f32, pan: f32) -> [f32; 2] {
    let across = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    // Both ears alike is each at 0.707 of the whole: brought back up so that a sound not
    // placed anywhere is as loud as it was recorded.
    [volume * across.cos() * std::f32::consts::SQRT_2, volume * across.sin() * std::f32::consts::SQRT_2]
}

/// Music being read from its file as it plays.
struct Music {
    voice: VoiceId,
    reader: lewton::inside_ogg::OggStreamReader<std::io::BufReader<std::fs::File>>,
    looping: bool,
    done: bool,
}

/// Plays sounds and says how loud everything is.
pub struct Audio {
    mixer: Arc<Mutex<Mixer>>,
    /// Whether the machine's sound output is playing the mixer. Where it is not, the frame
    /// runs it.
    heard: bool,
    music: Vec<Music>,
    /// Where what the mixer makes goes when no output is asking for it.
    scratch: Vec<f32>,
}

impl Default for Audio {
    fn default() -> Self {
        Self { mixer: Arc::new(Mutex::new(Mixer::new(48_000))), heard: false, music: Vec::new(), scratch: Vec::new() }
    }
}

impl Audio {
    fn mixer(&self) -> MutexGuard<'_, Mixer> {
        self.mixer.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether anyone can hear: the machine has a sound output and it is open.
    pub fn is_heard(&self) -> bool {
        self.heard
    }

    /// Plays a sound once, as it was recorded.
    pub fn play(&self, sound: &Sound) -> VoiceId {
        self.play_with(sound, Playback::default())
    }

    pub fn play_with(&self, sound: &Sound, how: Playback) -> VoiceId {
        let source = Source::Memory { samples: sound.samples.clone(), channels: sound.channels as usize, rate: sound.rate };
        self.mixer().play(source, ears(how.volume, how.pan), how.pitch, how.looping)
    }

    /// Plays music from an Ogg Vorbis file, reading it as it goes. It may be changed and
    /// stopped as any sound may.
    pub fn play_music(&mut self, path: impl AsRef<std::path::Path>, how: Playback) -> anyhow::Result<VoiceId> {
        use anyhow::Context;
        let path = path.as_ref();
        let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let reader = lewton::inside_ogg::OggStreamReader::new(std::io::BufReader::new(file)).with_context(|| format!("{} is not Ogg Vorbis", path.display()))?;
        let source = Source::Stream {
            queue: VecDeque::new(),
            channels: reader.ident_hdr.audio_channels as usize,
            rate: reader.ident_hdr.audio_sample_rate,
            ended: false,
        };
        let voice = self.mixer().play(source, ears(how.volume, how.pan), how.pitch, false);
        self.music.push(Music { voice, reader, looping: how.looping, done: false });
        self.feed();
        Ok(voice)
    }

    /// Stops a sound. One that has ended already is not there to stop.
    pub fn stop(&mut self, voice: VoiceId) {
        self.mixer().stop(voice);
        self.music.retain(|music| music.voice != voice);
    }

    pub fn is_playing(&self, voice: VoiceId) -> bool {
        self.mixer().is_playing(voice)
    }

    /// How many sounds are playing.
    pub fn playing(&self) -> usize {
        self.mixer().playing()
    }

    pub fn set_volume(&self, voice: VoiceId, volume: f32, pan: f32) {
        self.mixer().set_gain(voice, ears(volume, pan));
    }

    pub fn set_pitch(&self, voice: VoiceId, pitch: f32) {
        self.mixer().set_pitch(voice, pitch);
    }

    pub fn set_paused(&self, voice: VoiceId, paused: bool) {
        self.mixer().set_paused(voice, paused);
    }

    /// How loud everything is: 1 as it is, 0 silent.
    pub fn set_master_volume(&self, volume: f32) {
        self.mixer().volume = volume.max(0.0);
    }

    pub fn master_volume(&self) -> f32 {
        self.mixer().volume
    }

    /// Holds every sound where it is, silent, or lets them go on: what is done while an app
    /// is put away on a phone, and brought back. The engine does this itself then.
    pub fn set_held(&self, held: bool) {
        self.mixer().held = held;
    }

    pub fn is_held(&self) -> bool {
        self.mixer().held
    }

    /// Reads more of each piece of music, so that a second of it is always ready to play.
    fn feed(&mut self) {
        let mixer = self.mixer.clone();
        let mut mixer = mixer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.music.retain_mut(|music| {
            let rate = music.reader.ident_hdr.audio_sample_rate as usize;
            while !music.done && mixer.queued(music.voice).is_some_and(|queued| queued < rate) {
                match music.reader.read_dec_packet_itl() {
                    Ok(Some(packet)) => {
                        let samples: Vec<f32> = packet.into_iter().map(|sample| sample as f32 / 32_768.0).collect();
                        mixer.feed(music.voice, &samples, false);
                    }
                    // The end: round again from the beginning, or no more.
                    Ok(None) if music.looping && music.reader.seek_absgp_pg(0).is_ok() => {}
                    Ok(None) => music.done = true,
                    Err(err) => {
                        log::error!("music stopped: {err}");
                        music.done = true;
                    }
                }
            }
            if music.done {
                mixer.feed(music.voice, &[], true);
            }
            mixer.is_playing(music.voice)
        });
    }

    /// Runs the mixer for a frame where no output is asking it to, so that sounds last as
    /// long as they would.
    fn run_unheard(&mut self, seconds: f32) {
        if self.heard {
            return;
        }
        let frames = (seconds.clamp(0.0, 0.25) * self.mixer().rate as f32) as usize;
        self.scratch.resize(frames * 2, 0.0);
        let mixer = self.mixer.clone();
        mixer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mix(&mut self.scratch, 2);
    }
}

/// Plays a sound for as long as its entity has it, heard from where the entity is.
#[derive(Clone, Copy, Debug)]
pub struct AudioSource {
    pub sound: Handle<Sound>,
    pub volume: f32,
    pub pitch: f32,
    pub looping: bool,
    /// How far off it can be heard, in metres: quieter with distance, silent there, and
    /// louder in the ear on its side. Nought is a sound heard the same wherever the listener
    /// is, as music is.
    pub range: f32,
    voice: Option<VoiceId>,
    begun: bool,
}

impl Component for AudioSource {}

impl AudioSource {
    /// The sound, once, heard the same everywhere.
    pub fn new(sound: Handle<Sound>) -> Self {
        Self { sound, volume: 1.0, pitch: 1.0, looping: false, range: 0.0, voice: None, begun: false }
    }

    /// Heard from where the entity is, as far off as `range` metres.
    pub fn heard_within(mut self, range: f32) -> Self {
        self.range = range;
        self
    }

    pub fn looping(mut self) -> Self {
        self.looping = true;
        self
    }

    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    /// Whether it has played to its end. A looping sound never has.
    pub fn is_finished(&self) -> bool {
        self.begun && self.voice.is_none()
    }
}

/// Where sound is heard from, and which way the hearer faces. Where no entity has one, the
/// active camera hears.
#[derive(Clone, Copy, Debug, Default)]
pub struct AudioListener;

impl Component for AudioListener {}

/// How loud a source at `at` is in each ear of a listener at `from` who has `right` on the
/// right hand.
fn heard_from(source: &AudioSource, at: Vec3, from: Vec3, right: Vec3) -> [f32; 2] {
    if source.range <= 0.0 {
        return ears(source.volume, 0.0);
    }
    let to = at - from;
    let away = to.length();
    // Quieter with distance, and brought to nothing at its range.
    let near = (1.0 - away / source.range).clamp(0.0, 1.0);
    // Right beside the listener it is in both ears; further off, in the one on its side.
    let side = if away > 1e-4 { (to / away).dot(right) * (away / 0.5).min(1.0) } else { 0.0 };
    ears(source.volume * near * near, side * 0.8)
}

/// Begins the sources that have not begun, places every one that is heard from somewhere,
/// and notes those that have ended.
fn play_sources(
    audio: Res<Audio>,
    sounds: Res<Assets<Sound>>,
    listeners: Query<&GlobalTransform, With<AudioListener>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mut sources: Query<(&mut AudioSource, Option<&GlobalTransform>)>,
) {
    let listener = listeners.iter().next().or_else(|| cameras.iter().find(|(camera, _)| camera.active).map(|(_, at)| at));
    let (from, right) = listener.map_or((Vec3::ZERO, Vec3::X), |at| (at.translation(), at.0.transform_vector3(Vec3::X).normalize_or(Vec3::X)));
    let mut mixer = audio.mixer();
    for (mut source, at) in &mut sources {
        let gain = heard_from(&source, at.map_or(from, |at| at.translation()), from, right);
        match source.voice {
            None if !source.begun => {
                // Not begun until its sound is there to play.
                if let Some(sound) = sounds.get(source.sound) {
                    let memory = Source::Memory { samples: sound.samples.clone(), channels: sound.channels as usize, rate: sound.rate };
                    source.voice = Some(mixer.play(memory, gain, source.pitch, source.looping));
                    source.begun = true;
                }
            }
            None => {}
            Some(voice) if mixer.is_playing(voice) => {
                mixer.set_gain(voice, gain);
                mixer.set_pitch(voice, source.pitch);
            }
            Some(_) => source.voice = None,
        }
    }
}

/// Reads on in the music, and runs the mixer where no output does.
fn keep_playing(time: Res<Time>, mut audio: ResMut<Audio>) {
    audio.feed();
    audio.run_unheard(time.delta_secs());
}

/// A source's sound stops when the source is taken from its entity, or the entity goes.
fn stop_source(world: &mut World, entity: Entity) {
    let voice = world.get::<AudioSource>(entity).and_then(|source| source.voice);
    if let (Some(voice), Some(audio)) = (voice, world.get_resource_mut::<Audio>()) {
        audio.stop(voice);
    }
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        let mut audio = Audio::default();
        // `MIRA_AUDIO=0` is for tests and machines that should stay quiet.
        if std::env::var("MIRA_AUDIO").as_deref() != Ok("0") {
            match device::open(audio.mixer.clone()) {
                Ok(rate) => {
                    audio.heard = true;
                    log::info!("sound is playing at {rate} Hz");
                }
                Err(why) => log::warn!("no sound: {why}"),
            }
        }
        app.world.insert_resource(audio);
        app.init_resource::<Assets<Sound>>().add_systems(Stage::Last, (play_sources, keep_playing));
        app.world.on_remove::<AudioSource>(stop_source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_app() -> App {
        let mut app = App::new();
        app.add_plugins(crate::time::TimePlugin).add_plugins(crate::transform::TransformPlugin);
        app.world.insert_resource(Audio::default());
        app.init_resource::<Assets<Sound>>().add_systems(Stage::Last, (play_sources, keep_playing));
        app.world.on_remove::<AudioSource>(stop_source);
        app.world.resource_mut::<Time>().set_fixed_step(Some(std::time::Duration::from_millis(20)));
        app
    }

    #[test]
    fn a_sound_is_as_loud_in_all_wherever_it_is_between_the_ears() {
        let middle = ears(1.0, 0.0);
        assert!((middle[0] - 1.0).abs() < 1e-6 && (middle[1] - 1.0).abs() < 1e-6);
        let (left, right) = (ears(1.0, -1.0), ears(1.0, 1.0));
        assert!(left[1].abs() < 1e-6 && right[0].abs() < 1e-6 && left[0] > 1.0);
        for pan in [-1.0, -0.4, 0.0, 0.7, 1.0] {
            let [l, r] = ears(0.5, pan);
            assert!((l * l + r * r - 0.5).abs() < 1e-5, "at {pan}");
        }
    }

    #[test]
    fn a_source_is_quieter_further_off_and_louder_on_its_side() {
        let source = AudioSource::new(Handle::from_id(0)).heard_within(20.0);
        let at = |x: f32, z: f32| heard_from(&source, Vec3::new(x, 0.0, z), Vec3::ZERO, Vec3::X);
        let (near, far, gone) = (at(0.0, -2.0), at(0.0, -10.0), at(0.0, -25.0));
        assert!(near[0] > far[0] && far[0] > 0.0 && gone == [0.0, 0.0]);
        assert!((near[0] - near[1]).abs() < 1e-6, "ahead is in both ears alike");
        let (right, left) = (at(5.0, 0.0), at(-5.0, 0.0));
        assert!(right[1] > right[0] * 2.0 && left[0] > left[1] * 2.0);
        // One with no range is heard the same wherever it is.
        let everywhere = AudioSource::new(Handle::from_id(0)).with_volume(0.5);
        assert_eq!(heard_from(&everywhere, Vec3::new(900.0, 0.0, 0.0), Vec3::ZERO, Vec3::X), ears(0.5, 0.0));
    }

    #[test]
    fn a_source_plays_once_its_sound_is_there_ends_and_stops_with_its_entity() {
        let mut app = quiet_app();
        let (short, long) = {
            let sounds = app.world.resource_mut::<Assets<Sound>>();
            (sounds.add(Sound::tone(440.0, 0.1)), sounds.add(Sound::tone(220.0, 60.0)))
        };
        let once = app.world.spawn(AudioSource::new(short));
        let humming = app.world.spawn((crate::transform::Transform::from_xyz(3.0, 0.0, 0.0), AudioSource::new(long).looping().heard_within(30.0)));
        app.update();
        assert_eq!(app.world.resource::<Audio>().playing(), 2);
        assert!(!app.world.get::<AudioSource>(once).unwrap().is_finished());
        // No output is open here, so the frame runs the mixer: a tenth of a second on, the
        // short one is over, and says so.
        for _ in 0..8 {
            app.update();
        }
        assert_eq!(app.world.resource::<Audio>().playing(), 1);
        assert!(app.world.get::<AudioSource>(once).unwrap().is_finished());
        assert!(!app.world.get::<AudioSource>(humming).unwrap().is_finished());
        // Its entity gone, the other stops.
        app.world.despawn(humming);
        assert_eq!(app.world.resource::<Audio>().playing(), 0);
    }

    /// Reads real files, where there are any to read: `MIRA_TEST_WAV` and `MIRA_TEST_OGG`
    /// name one of each. Without them it checks nothing, since none are kept with the engine.
    #[test]
    fn real_files_are_read_and_music_plays_to_its_end() {
        if let Ok(path) = std::env::var("MIRA_TEST_WAV") {
            let sound = Sound::load(&path).unwrap();
            assert!(sound.seconds() > 0.05 && sound.samples.iter().any(|sample| sample.abs() > 0.01), "{path}");
            assert!(sound.samples.iter().all(|sample| sample.abs() <= 1.0));
        }
        if let Ok(path) = std::env::var("MIRA_TEST_OGG") {
            let whole = Sound::load(&path).unwrap();
            assert!(whole.seconds() > 0.05 && whole.samples.iter().any(|sample| sample.abs() > 0.01), "{path}");
            // The same file as music: read as it plays, it lasts as long and then is gone.
            let mut audio = Audio::default();
            audio.mixer().rate = whole.rate;
            let voice = audio.play_music(&path, Playback::default()).unwrap();
            let mut played = 0.0;
            while audio.is_playing(voice) && played < whole.seconds() + 5.0 {
                audio.feed();
                audio.run_unheard(0.05);
                played += 0.05;
            }
            assert!(!audio.is_playing(voice), "music still playing after {played} s");
            assert!((played - whole.seconds()).abs() < 0.2, "played for {played} s, the file is {} s", whole.seconds());
            // And what was reading it is let go of the next time the music is looked to.
            audio.feed();
            assert!(audio.music.is_empty());
        }
    }

    #[test]
    fn sounds_played_at_once_can_be_changed_and_stopped() {
        let mut audio = Audio::default();
        let voice = audio.play_with(&Sound::tone(440.0, 1.0), Playback { looping: true, ..Default::default() });
        assert!(audio.is_playing(voice) && !audio.is_heard());
        audio.set_paused(voice, true);
        audio.run_unheard(0.2);
        audio.set_master_volume(0.25);
        assert_eq!(audio.master_volume(), 0.25);
        audio.stop(voice);
        assert!(!audio.is_playing(voice));
        assert!(audio.play_music("no/such/music.ogg", Playback::default()).is_err());
    }
}