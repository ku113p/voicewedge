//! Microphone capture via cpal. No resampling — the STT API resamples server-side.

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

    /// Stop capture, downmix to mono, downsample to 16 kHz, return (samples, rate).
    /// 16 kHz is Whisper/gpt-4o's native rate and keeps the upload small — a 48 kHz
    /// 3-min recording is ~22 MB base64 and OpenRouter 502s on it.
    pub fn stop(self) -> (Vec<f32>, u32) {
        drop(self.stream);
        let raw = self.buf.lock().unwrap().clone();
        let mono: Vec<f32> = if self.channels > 1 {
            raw.chunks(self.channels as usize)
                .map(|c| c.iter().sum::<f32>() / c.len() as f32)
                .collect()
        } else {
            raw
        };
        downsample(mono, self.sample_rate, 16_000)
    }
}

/// Window-averaging downsampler (anti-aliased). No-op if already <= target.
fn downsample(input: Vec<f32>, from: u32, to: u32) -> (Vec<f32>, u32) {
    if from <= to || input.is_empty() {
        return (input, from);
    }
    let ratio = from as f64 / to as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for j in 0..out_len {
        let start = (j as f64 * ratio) as usize;
        let end = (((j + 1) as f64 * ratio) as usize).min(input.len());
        if start >= end {
            break;
        }
        out.push(input[start..end].iter().sum::<f32>() / (end - start) as f32);
    }
    (out, to)
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
