//! Mixing: every sound that is playing, added together into what the device is given.
//!
//! This is the whole of what makes the sound, and it knows nothing of a device: it is asked
//! for so many frames at the device's rate and fills them. So it can be run, and tested,
//! where there is no device at all.

use std::collections::VecDeque;
use std::sync::Arc;

/// A sound that is playing. Kept to change it or stop it; a sound that has ended is simply
/// no longer there, and changing it does nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(pub(crate) u64);

/// Where a voice's samples come from.
pub(crate) enum Source {
    /// A sound held whole: frames of `channels` samples each, at `rate` frames a second.
    Memory { samples: Arc<[f32]>, channels: usize, rate: u32 },
    /// A sound that arrives as it plays, as music read from a file does: what has been
    /// decoded and not yet played, and whether there is no more to come.
    Stream { queue: VecDeque<f32>, channels: usize, rate: u32, ended: bool },
}

struct Voice {
    id: VoiceId,
    source: Source,
    /// How far through the source, in its frames. For a stream, counted from the front of
    /// what is queued.
    at: f64,
    /// How loud in the left ear and the right, as asked, and as it is now: the one follows
    /// the other over a few milliseconds, so that a change does not click.
    asked: [f32; 2],
    now: [f32; 2],
    pitch: f32,
    looping: bool,
    paused: bool,
}

/// How quickly a voice's loudness follows what is asked of it: the part of the difference
/// made up in one frame of output.
const FOLLOW: f32 = 0.004;

pub struct Mixer {
    voices: Vec<Voice>,
    next: u64,
    /// Frames a second the output is asked for at.
    pub(crate) rate: u32,
    /// Everything is this loud, 1 being as it is.
    pub(crate) volume: f32,
}

impl Mixer {
    pub(crate) fn new(rate: u32) -> Self {
        Self { voices: Vec::new(), next: 1, rate: rate.max(1), volume: 1.0 }
    }

    pub(crate) fn play(&mut self, source: Source, gain: [f32; 2], pitch: f32, looping: bool) -> VoiceId {
        let id = VoiceId(self.next);
        self.next += 1;
        // A sound begins as loud as it is asked to be: only changes are eased.
        self.voices.push(Voice { id, source, at: 0.0, asked: gain, now: gain, pitch: pitch.max(0.01), looping, paused: false });
        id
    }

    fn voice(&mut self, id: VoiceId) -> Option<&mut Voice> {
        self.voices.iter_mut().find(|voice| voice.id == id)
    }

    pub(crate) fn stop(&mut self, id: VoiceId) {
        self.voices.retain(|voice| voice.id != id);
    }

    pub(crate) fn set_gain(&mut self, id: VoiceId, gain: [f32; 2]) {
        if let Some(voice) = self.voice(id) {
            voice.asked = gain;
        }
    }

    pub(crate) fn set_pitch(&mut self, id: VoiceId, pitch: f32) {
        if let Some(voice) = self.voice(id) {
            voice.pitch = pitch.max(0.01);
        }
    }

    pub(crate) fn set_paused(&mut self, id: VoiceId, paused: bool) {
        if let Some(voice) = self.voice(id) {
            voice.paused = paused;
        }
    }

    pub(crate) fn is_playing(&self, id: VoiceId) -> bool {
        self.voices.iter().any(|voice| voice.id == id)
    }

    pub(crate) fn playing(&self) -> usize {
        self.voices.len()
    }

    /// How many frames a stream has decoded and not yet played, or none if it is not there.
    pub(crate) fn queued(&self, id: VoiceId) -> Option<usize> {
        self.voices.iter().find(|voice| voice.id == id).and_then(|voice| match &voice.source {
            Source::Stream { queue, channels, .. } => Some(queue.len() / channels.max(&1)),
            Source::Memory { .. } => None,
        })
    }

    /// Gives a stream more of its sound, and says whether that is the end of it.
    pub(crate) fn feed(&mut self, id: VoiceId, samples: &[f32], last: bool) {
        if let Some(Source::Stream { queue, ended, .. }) = self.voice(id).map(|voice| &mut voice.source) {
            queue.extend(samples);
            *ended |= last;
        }
    }

    /// Fills `out` with what is playing: frames of `channels` samples, the first two of each
    /// the left ear and the right, any others silent. Sounds that end in it are let go.
    pub fn mix(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        let channels = channels.max(1);
        let (rate, volume) = (self.rate as f64, self.volume);
        self.voices.retain_mut(|voice| {
            if voice.paused {
                return true;
            }
            let (of, from_rate) = match &voice.source {
                Source::Memory { channels, rate, .. } | Source::Stream { channels, rate, .. } => ((*channels).max(1), *rate as f64),
            };
            let step = voice.pitch as f64 * from_rate / rate;
            for frame in out.chunks_exact_mut(channels) {
                let held = match &voice.source {
                    Source::Memory { samples, .. } => samples.len() / of,
                    Source::Stream { queue, .. } => queue.len() / of,
                };
                // A sound that goes round begins again where it ends.
                if voice.looping && held > 0 && matches!(voice.source, Source::Memory { .. }) {
                    voice.at %= held as f64;
                }
                // The two frames of the source that this moment lies between.
                let (first, between) = (voice.at.floor() as usize, voice.at.fract() as f32);
                if first >= held {
                    match &voice.source {
                        Source::Memory { .. } => return false,
                        // A stream that has run dry waits for more, unless there is none.
                        Source::Stream { ended, .. } => {
                            if *ended {
                                return false;
                            }
                            break;
                        }
                    }
                }
                let second = match &voice.source {
                    Source::Memory { .. } if voice.looping => (first + 1) % held,
                    _ => (first + 1).min(held - 1),
                };
                let sample = |at: usize, channel: usize| match &voice.source {
                    Source::Memory { samples, .. } => samples[at * of + channel.min(of - 1)],
                    Source::Stream { queue, .. } => queue[at * of + channel.min(of - 1)],
                };
                for ear in 0..2 {
                    voice.now[ear] += (voice.asked[ear] - voice.now[ear]) * FOLLOW;
                    // One channel is heard in both ears; of two, each ear has its own.
                    let heard = sample(first, ear) * (1.0 - between) + sample(second, ear) * between;
                    let into = if channels == 1 { 0 } else { ear };
                    frame[into] += heard * voice.now[ear] * volume * if channels == 1 { 0.5 } else { 1.0 };
                }
                voice.at += step;
                // What a stream has played is let go of.
                if let Source::Stream { queue, .. } = &mut voice.source {
                    let played = voice.at.floor() as usize;
                    if played > 0 {
                        queue.drain(..(played * of).min(queue.len()));
                        voice.at -= played as f64;
                    }
                }
            }
            true
        });
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(samples: &[f32], channels: usize, rate: u32) -> Source {
        Source::Memory { samples: samples.into(), channels, rate }
    }

    #[test]
    fn a_sound_plays_through_once_and_is_let_go() {
        let mut mixer = Mixer::new(100);
        let voice = mixer.play(memory(&[0.5, 0.25, -0.5, 1.0], 1, 100), [1.0, 0.5], 1.0, false);
        let mut out = [9.0; 12];
        mixer.mix(&mut out, 2);
        // Each of its frames in both ears, as loud as each was asked; then silence.
        assert_eq!(&out[..8], &[0.5, 0.25, 0.25, 0.125, -0.5, -0.25, 1.0, 0.5]);
        assert_eq!(&out[8..], &[0.0; 4]);
        assert!(!mixer.is_playing(voice) && mixer.playing() == 0);
    }

    #[test]
    fn a_looping_sound_goes_round_and_stops_when_told() {
        let mut mixer = Mixer::new(10);
        let voice = mixer.play(memory(&[1.0, 0.0, -1.0], 1, 10), [0.5, 0.5], 1.0, true);
        let mut out = [0.0; 14];
        mixer.mix(&mut out, 2);
        let left: Vec<f32> = out.chunks(2).map(|frame| frame[0]).collect();
        assert_eq!(left, [0.5, 0.0, -0.5, 0.5, 0.0, -0.5, 0.5]);
        assert!(mixer.is_playing(voice));
        mixer.stop(voice);
        mixer.mix(&mut out, 2);
        assert_eq!(out, [0.0; 14]);
    }

    #[test]
    fn a_sound_at_another_rate_or_pitch_is_heard_at_the_right_speed() {
        // A ramp at 50 frames a second, into output at 100: each step takes two frames, the
        // one between them halfway.
        let mut mixer = Mixer::new(100);
        mixer.play(memory(&[0.0, 0.2, 0.4, 0.6], 1, 50), [1.0, 1.0], 1.0, false);
        let mut out = [0.0; 8];
        mixer.mix(&mut out, 1);
        for (heard, wanted) in out.iter().zip([0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.6]) {
            assert!((heard - wanted).abs() < 1e-6, "{out:?}");
        }
        // Twice the pitch is twice as fast, and so over in half the time.
        let mut mixer = Mixer::new(100);
        let voice = mixer.play(memory(&[0.0, 0.2, 0.4, 0.6], 1, 100), [1.0, 1.0], 2.0, false);
        mixer.mix(&mut out, 1);
        assert_eq!(&out[..3], &[0.0, 0.4, 0.0]);
        assert!(!mixer.is_playing(voice));
    }

    #[test]
    fn two_channels_keep_their_ears_and_sounds_add_up_without_going_over() {
        let mut mixer = Mixer::new(10);
        mixer.play(memory(&[0.25, -0.25, 0.5, -0.5], 2, 10), [1.0, 1.0], 1.0, false);
        mixer.play(memory(&[0.5, 0.75], 1, 10), [1.0, 1.0], 1.0, false);
        let mut out = [0.0; 4];
        mixer.mix(&mut out, 2);
        assert_eq!(out, [0.75, 0.25, 1.0, 0.25], "the second frame's left is held at one");
        // Everything is as loud as the mixer's volume says.
        let mut quiet = Mixer::new(10);
        quiet.volume = 0.5;
        quiet.play(memory(&[1.0], 1, 10), [1.0, 1.0], 1.0, false);
        quiet.mix(&mut out[..2], 2);
        assert_eq!(&out[..2], &[0.5, 0.5]);
    }

    #[test]
    fn loudness_asked_for_later_is_come_to_without_a_jump() {
        let mut mixer = Mixer::new(1000);
        let voice = mixer.play(memory(&[1.0; 4000], 1, 1000), [1.0, 1.0], 1.0, false);
        mixer.set_gain(voice, [0.0, 1.0]);
        let mut out = [0.0; 4000];
        mixer.mix(&mut out, 2);
        let left: Vec<f32> = out.chunks(2).map(|frame| frame[0]).collect();
        assert!(left[0] > 0.99 && left[1999] < 0.01);
        assert!(left.windows(2).all(|pair| pair[1] <= pair[0] && pair[0] - pair[1] < 0.005));
        assert!(out.chunks(2).all(|frame| frame[1] == 1.0));
    }

    #[test]
    fn a_stream_plays_what_it_is_given_waits_for_more_and_ends() {
        let mut mixer = Mixer::new(10);
        let voice = mixer.play(Source::Stream { queue: VecDeque::new(), channels: 1, rate: 10, ended: false }, [1.0, 1.0], 1.0, false);
        let mut out = [0.0; 4];
        mixer.mix(&mut out, 1);
        assert!(out == [0.0; 4] && mixer.is_playing(voice), "nothing yet, and it waits");
        mixer.feed(voice, &[0.25, 0.5, 0.75], false);
        assert_eq!(mixer.queued(voice), Some(3));
        mixer.mix(&mut out[..2], 1);
        assert_eq!(&out[..2], &[0.25, 0.5]);
        assert_eq!(mixer.queued(voice), Some(1), "what was played is let go of");
        mixer.feed(voice, &[1.0], true);
        mixer.mix(&mut out, 1);
        assert_eq!(out, [0.75, 1.0, 0.0, 0.0]);
        assert!(!mixer.is_playing(voice), "its end reached, it is gone");
    }

    #[test]
    fn a_paused_sound_keeps_its_place() {
        let mut mixer = Mixer::new(10);
        let voice = mixer.play(memory(&[0.25, 0.5], 1, 10), [1.0, 1.0], 1.0, false);
        let mut out = [0.0; 1];
        mixer.mix(&mut out, 1);
        mixer.set_paused(voice, true);
        mixer.mix(&mut out, 1);
        assert_eq!(out, [0.0]);
        mixer.set_paused(voice, false);
        mixer.mix(&mut out, 1);
        assert_eq!(out, [0.5]);
    }
}