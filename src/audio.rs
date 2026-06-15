//! Microphone capture via cpal (cross-platform: WASAPI / ALSA / CoreAudio).
//! Records to an in-memory f32 buffer; on stop, downmixes to mono and the caller
//! writes a 16-bit WAV. No resampling yet — the STT API resamples server-side.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

pub struct Recorder {
    stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    channels: u16,
    sample_rate: u32,
}

impl Recorder {
    pub fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device (is a microphone connected?)"))?;
        let supported = device.default_input_config()?;
        let channels = supported.channels();
        let sample_rate = supported.sample_rate();
        let sample_format = supported.sample_format();
        let stream_config: cpal::StreamConfig = supported.into();
        tracing::info!("recording ({channels}ch @ {sample_rate}Hz, {sample_format:?})");

        let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
        let cb_buf = buf.clone();
        let err_fn = |e| tracing::error!("audio stream error: {e}");

        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                stream_config.clone(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    cb_buf.lock().unwrap().extend_from_slice(data);
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                stream_config.clone(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let mut b = cb_buf.lock().unwrap();
                    b.extend(data.iter().map(|&s| s as f32 / 32768.0));
                },
                err_fn,
                None,
            )?,
            cpal::SampleFormat::U16 => device.build_input_stream(
                stream_config.clone(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    let mut b = cb_buf.lock().unwrap();
                    b.extend(data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0));
                },
                err_fn,
                None,
            )?,
            other => return Err(anyhow!("unsupported sample format {other:?}")),
        };
        stream.play()?;
        Ok(Self { stream, buf, channels, sample_rate })
    }

    /// Stop capture, downmix to mono, return (samples, sample_rate).
    pub fn stop(self) -> (Vec<f32>, u32) {
        drop(self.stream); // stops the input stream
        let raw = self.buf.lock().unwrap().clone();
        let mono = if self.channels > 1 {
            raw.chunks(self.channels as usize)
                .map(|c| c.iter().sum::<f32>() / c.len() as f32)
                .collect()
        } else {
            raw
        };
        (mono, self.sample_rate)
    }
}

/// Write mono f32 samples as a 16-bit PCM WAV.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        writer.write_sample(v)?;
    }
    writer.finalize()?;
    Ok(())
}
