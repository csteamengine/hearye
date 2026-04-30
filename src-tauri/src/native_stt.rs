use anyhow::{anyhow, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";
const MODEL_FILENAME: &str = "ggml-base.en.bin";
const BUNDLED_RESOURCE_PATH: &str = "models/ggml-base.en.bin";
// Sanity check on download — model is ~140 MB.
const MIN_MODEL_BYTES: u64 = 100 * 1024 * 1024;

static CTX: OnceLock<WhisperContext> = OnceLock::new();

fn log_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let dir = PathBuf::from(home).join("Library/Logs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("hearye.log")
}

fn log(msg: impl AsRef<str>) {
    let line = format!("{} [native_stt] {}\n", clock(), msg.as_ref());
    eprint!("{line}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = f.write_all(line.as_bytes());
    }
}

fn clock() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

fn model_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join("Library/Application Support/com.charlie.hearye/models")
}

fn model_path() -> PathBuf {
    model_dir().join(MODEL_FILENAME)
}

pub async fn transcribe_wav(app: AppHandle, wav: Vec<u8>) -> Result<String> {
    let model = ensure_model_downloaded(&app).await?;
    tauri::async_runtime::spawn_blocking(move || transcribe_blocking(&app, &model, &wav)).await?
}

/// Resolve the whisper model file. Tiered:
///   1. User override at ~/Library/Application Support/com.charlie.hearye/models/
///      (lets a user swap in a different model without rebuilding the .app)
///   2. Bundled resource shipped with the .app (Contents/Resources/models/)
///   3. One-time HTTPS download from HuggingFace (only happens in dev /
///      unbundled builds — packaged .app always finds tier 2 first)
async fn ensure_model_downloaded(app: &AppHandle) -> Result<PathBuf> {
    let override_path = model_path();
    if model_is_valid(&override_path) {
        log(format!("using override model at {}", override_path.display()));
        return Ok(override_path);
    }

    if let Ok(bundled) = app
        .path()
        .resolve(BUNDLED_RESOURCE_PATH, BaseDirectory::Resource)
    {
        if model_is_valid(&bundled) {
            log(format!("using bundled model at {}", bundled.display()));
            return Ok(bundled);
        }
    }

    log(format!(
        "no override or bundled model — downloading to {}",
        override_path.display()
    ));
    let _ = app.emit("hearye://state", "downloading-model");
    std::fs::create_dir_all(model_dir())?;
    let bytes = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?
        .get(MODEL_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if (bytes.len() as u64) < MIN_MODEL_BYTES {
        return Err(anyhow!(
            "downloaded model is suspiciously small ({} bytes)",
            bytes.len()
        ));
    }
    std::fs::write(&override_path, &bytes)?;
    log(format!("model downloaded ({} bytes)", bytes.len()));
    Ok(override_path)
}

fn model_is_valid(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.len() >= MIN_MODEL_BYTES)
        .unwrap_or(false)
}

fn transcribe_blocking(app: &AppHandle, model_path: &Path, wav: &[u8]) -> Result<String> {
    log(format!("decoding {} wav bytes", wav.len()));
    let mut samples = wav_to_f32_mono(wav)?;
    log(format!(
        "decoded {} samples ({:.2}s of audio)",
        samples.len(),
        samples.len() as f32 / 16_000.0
    ));
    // whisper.cpp rejects clips shorter than 1s; pad with silence so short
    // pushes-to-talk still produce a transcription.
    const MIN_SAMPLES: usize = 16_000 + 1_600; // ~1.1s
    if samples.len() < MIN_SAMPLES {
        log(format!(
            "padding {} samples up to {}",
            samples.len(),
            MIN_SAMPLES
        ));
        samples.resize(MIN_SAMPLES, 0.0);
    }
    let ctx = get_ctx(app, model_path)?;
    let mut state = ctx
        .create_state()
        .map_err(|e| anyhow!("whisper create_state: {e:?}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(num_threads());
    params.set_translate(false);
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    log(format!("running whisper.full"));
    state
        .full(params, &samples)
        .map_err(|e| anyhow!("whisper.full: {e:?}"))?;

    let num_segments = state
        .full_n_segments()
        .map_err(|e| anyhow!("full_n_segments: {e:?}"))?;
    let mut text = String::new();
    for i in 0..num_segments {
        let seg = state
            .full_get_segment_text(i)
            .map_err(|e| anyhow!("full_get_segment_text({i}): {e:?}"))?;
        text.push_str(&seg);
    }
    let cleaned = clean_whisper_output(&text);
    log(format!(
        "transcribed {} chars (raw {} chars)",
        cleaned.len(),
        text.trim().len()
    ));
    Ok(cleaned)
}

/// Strip whisper.cpp's bracketed annotations ([BLANK_AUDIO], [Music], [Applause], …)
/// and noise-marker parentheticals so we don't paste them into the user's app.
fn clean_whisper_output(text: &str) -> String {
    // Drop [...] segments.
    let mut buf = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '[' {
            for cc in chars.by_ref() {
                if cc == ']' {
                    break;
                }
            }
        } else {
            buf.push(c);
        }
    }
    // Drop common parenthetical noise markers.
    for marker in [
        "(silence)",
        "(Silence)",
        "(SILENCE)",
        "(inaudible)",
        "(Inaudible)",
        "(no speech)",
        "(No speech)",
    ] {
        buf = buf.replace(marker, "");
    }
    // Collapse whitespace into single spaces.
    buf.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn get_ctx(app: &AppHandle, model_path: &Path) -> Result<&'static WhisperContext> {
    if let Some(ctx) = CTX.get() {
        return Ok(ctx);
    }
    let path_str = model_path
        .to_str()
        .ok_or_else(|| anyhow!("model path is not valid UTF-8"))?;
    log(format!("loading whisper model into memory from {path_str}"));
    let _ = app.emit("hearye://state", "loading-model");
    let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
        .map_err(|e| anyhow!("WhisperContext::new: {e:?}"))?;
    log("whisper model loaded");
    let _ = CTX.set(ctx);
    Ok(CTX.get().expect("just set"))
}

fn num_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| (n.get() as i32).clamp(1, 8))
        .unwrap_or(4)
}

fn wav_to_f32_mono(wav: &[u8]) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::new(std::io::Cursor::new(wav))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(anyhow!("expected mono WAV, got {} channels", spec.channels));
    }
    if spec.sample_rate != 16_000 {
        return Err(anyhow!(
            "expected 16kHz WAV, got {} Hz",
            spec.sample_rate
        ));
    }
    let samples: std::result::Result<Vec<i16>, _> = reader.samples::<i16>().collect();
    let samples = samples.map_err(|e| anyhow!("wav read error: {e}"))?;
    Ok(samples
        .into_iter()
        .map(|s| s as f32 / i16::MAX as f32)
        .collect())
}
