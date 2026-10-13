//! The machine's sound output: a thread that keeps it open and gives it what the mixer makes.

use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::mixer::Mixer;

/// Opens the default output and has it play the mixer from then on. Answers the rate it
/// plays at, in frames a second, or why there is no sound: no device, or one that would not
/// open.
///
/// The output is kept open by a thread of its own that does nothing else, since on some
/// systems what holds it may not be handed from thread to thread.
pub(crate) fn open(mixer: Arc<Mutex<Mixer>>) -> Result<u32, String> {
    let (told, answer) = mpsc::channel();
    std::thread::Builder::new()
        .name("mira audio".into())
        .spawn(move || match start(mixer) {
            Ok((stream, rate)) => {
                let _ = told.send(Ok(rate));
                // Dropping the stream stops the sound, so it is held for good.
                let _held = stream;
                loop {
                    std::thread::park();
                }
            }
            Err(why) => {
                let _ = told.send(Err(why));
            }
        })
        .map_err(|err| err.to_string())?;
    answer.recv().map_err(|_| "the audio thread stopped before it said how it went".to_string())?
}

fn start(mixer: Arc<Mutex<Mixer>>) -> Result<(cpal::Stream, u32), String> {
    let device = cpal::default_host().default_output_device().ok_or("there is no sound output")?;
    let supported = device.default_output_config().map_err(|err| err.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (channels, rate) = (config.channels as usize, config.sample_rate);
    mixer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).rate = rate;
    let failed = |err| log::error!("the sound output failed: {err}");
    // What the device wants its samples as: the mixer makes floats, which are turned into
    // whole numbers where those are wanted.
    let mix = move |into: &mut [f32]| mixer.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).mix(into, channels);
    let mut floats: Vec<f32> = Vec::new();
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream(config, move |out: &mut [f32], _: &cpal::OutputCallbackInfo| mix(out), failed, None),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |out: &mut [i16], _: &cpal::OutputCallbackInfo| {
                floats.resize(out.len(), 0.0);
                mix(&mut floats);
                for (to, from) in out.iter_mut().zip(&floats) {
                    *to = (from * 32_767.0) as i16;
                }
            },
            failed,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            config,
            move |out: &mut [u16], _: &cpal::OutputCallbackInfo| {
                floats.resize(out.len(), 0.0);
                mix(&mut floats);
                for (to, from) in out.iter_mut().zip(&floats) {
                    *to = ((from * 0.5 + 0.5) * 65_535.0) as u16;
                }
            },
            failed,
            None,
        ),
        other => return Err(format!("the sound output wants samples as {other:?}, which is not made here")),
    }
    .map_err(|err| err.to_string())?;
    stream.play().map_err(|err| err.to_string())?;
    Ok((stream, rate))
}