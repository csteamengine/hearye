use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use parking_lot::Mutex;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const TARGET_SR: u32 = 16_000;

pub fn list_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|iter| iter.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub struct Recording {
    pcm: Arc<Mutex<Vec<i16>>>,
    stop_tx: mpsc::Sender<()>,
    done_rx: mpsc::Receiver<Result<u32>>,
    join: Option<thread::JoinHandle<()>>,
}

impl Recording {
    pub fn into_wav_16k_mono(mut self) -> Result<Vec<u8>> {
        let _ = self.stop_tx.send(());
        let src_sr = match self.done_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(sr)) => sr,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err(anyhow!("audio thread did not finish in time")),
        };
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
        let pcm = std::mem::take(&mut *self.pcm.lock());
        let resampled = resample_linear(&pcm, src_sr, TARGET_SR);
        encode_wav(&resampled, TARGET_SR)
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

pub fn start(app: AppHandle, device_name: Option<String>) -> Recording {
    let pcm: Arc<Mutex<Vec<i16>>> = Arc::new(Mutex::new(Vec::with_capacity(16_000 * 30)));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel::<Result<u32>>();

    let pcm_thread = pcm.clone();
    let app_thread = app.clone();
    let join = thread::spawn(move || {
        let res = run_stream(app_thread, device_name, pcm_thread, stop_rx);
        if let Err(ref e) = res {
            log::error!("audio thread error: {e}");
        }
        let _ = done_tx.send(res);
    });

    Recording {
        pcm,
        stop_tx,
        done_rx,
        join: Some(join),
    }
}

fn run_stream(
    app: AppHandle,
    device_name: Option<String>,
    pcm: Arc<Mutex<Vec<i16>>>,
    stop_rx: mpsc::Receiver<()>,
) -> Result<u32> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(name) => host
            .input_devices()?
            .find(|d| d.name().ok().as_deref() == Some(name.as_str()))
            .ok_or_else(|| anyhow!("input device '{name}' not found"))?,
        None => host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device"))?,
    };
    let config = device.default_input_config()?;
    let src_sr = config.sample_rate().0;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let stream_config: StreamConfig = config.into();

    let pcm_cb = pcm.clone();
    let app_cb = app.clone();
    let err_fn = |err| log::error!("audio stream error: {err}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| handle_block_f32(data, channels, &pcm_cb, &app_cb),
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| handle_block_i16(data, channels, &pcm_cb, &app_cb),
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                let converted: Vec<i16> = data.iter().map(|&s| (s as i32 - 32768) as i16).collect();
                handle_block_i16(&converted, channels, &pcm_cb, &app_cb);
            },
            err_fn,
            None,
        )?,
        fmt => return Err(anyhow!("unsupported sample format: {fmt:?}")),
    };
    stream.play()?;
    let _ = stop_rx.recv();
    drop(stream);
    Ok(src_sr)
}

fn handle_block_f32(data: &[f32], channels: usize, pcm: &Mutex<Vec<i16>>, app: &AppHandle) {
    let mono = downmix_f32(data, channels);
    let rms = rms_f32(&mono);
    let mut buf = pcm.lock();
    buf.reserve(mono.len());
    for s in &mono {
        buf.push((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
    }
    drop(buf);
    let _ = app.emit("hearye://level", rms);
}

fn handle_block_i16(data: &[i16], channels: usize, pcm: &Mutex<Vec<i16>>, app: &AppHandle) {
    let mono = downmix_i16(data, channels);
    let rms = rms_i16(&mono);
    pcm.lock().extend_from_slice(&mono);
    let _ = app.emit("hearye://level", rms);
}

fn downmix_f32(data: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn downmix_i16(data: &[i16], channels: usize) -> Vec<i16> {
    if channels == 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

fn rms_f32(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = data.iter().map(|s| s * s).sum();
    (sum_sq / data.len() as f32).sqrt()
}

fn rms_i16(data: &[i16]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = data
        .iter()
        .map(|&s| (s as f64 / i16::MAX as f64).powi(2))
        .sum();
    (sum_sq / data.len() as f64).sqrt() as f32
}

fn resample_linear(input: &[i16], src_sr: u32, dst_sr: u32) -> Vec<i16> {
    if src_sr == dst_sr || input.is_empty() {
        return input.to_vec();
    }
    let ratio = src_sr as f64 / dst_sr as f64;
    let out_len = ((input.len() as f64) / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 * ratio;
        let idx = src_pos.floor() as usize;
        let frac = src_pos - idx as f64;
        let a = input[idx.min(input.len() - 1)] as f64;
        let b = input[(idx + 1).min(input.len() - 1)] as f64;
        out.push((a + (b - a) * frac) as i16);
    }
    out
}

fn encode_wav(samples: &[i16], sr: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: sr,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = std::io::Cursor::new(Vec::<u8>::new());
    {
        let mut writer = hound::WavWriter::new(&mut buf, spec)?;
        for &s in samples {
            writer.write_sample(s)?;
        }
        writer.finalize()?;
    }
    Ok(buf.into_inner())
}
