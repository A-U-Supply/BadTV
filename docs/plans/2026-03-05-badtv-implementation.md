# BadTV Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a standalone Rust CLI that stitches TV news clips into infomercial-style audio collages.

**Architecture:** Five-stage pipeline — search GDELT TV API per word, fetch audio from archive.org, align with whisper-rs for word-level extraction, apply infomercial audio processing (pitch/EQ/compression/reverb), export WAV. Interactive TUI for clip selection. Pure Rust audio DSP, no ffmpeg dependency.

**Tech Stack:** Rust, clap, reqwest, symphonia, hound, whisper-rs, pitch_shift, ratatui, rodio, tts, anyhow, serde, rubato, indicatif, wiremock (dev)

---

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

**Step 1: Initialize cargo project**

Run: `cargo init --name badtv /Users/jake/au-supply/BadTV`

This will create `Cargo.toml` and `src/main.rs`. Since `Cargo.toml` may already
get a default, we overwrite it.

**Step 2: Write Cargo.toml with all dependencies**

```toml
[package]
name = "badtv"
version = "0.1.0"
edition = "2021"
description = "Stitch TV news clips into infomercial-style audio collages"
license = "MIT"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
hound = "3"
indicatif = "0.17"
pitch_shift = "1"
ratatui = "0.29"
crossterm = "0.28"
reqwest = { version = "0.12", features = ["json", "blocking"] }
rodio = "0.20"
rubato = "0.16"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
symphonia = { version = "0.5", features = ["mp3"] }
tokio = { version = "1", features = ["full"] }
tts = "0.26"
whisper-rs = "0.15"

[dev-dependencies]
wiremock = "0.6"
tempfile = "3"
```

**Step 3: Write minimal main.rs**

```rust
use anyhow::Result;

fn main() -> Result<()> {
    println!("badtv - coming soon");
    Ok(())
}
```

**Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully (may take a while for first build with whisper-rs)

Note: `whisper-rs` compiles whisper.cpp from source. This requires a C compiler
(`cc`/`clang`) and `cmake`. On macOS these come with Xcode command line tools.
If the build fails, check `xcode-select --install`.

**Step 5: Commit**

```
git add Cargo.toml src/main.rs
git commit -m "feat: initialize cargo project with all dependencies"
```

---

### Task 2: CLI Argument Parsing

**Files:**
- Modify: `src/main.rs`

**Step 1: Write a test for CLI parsing**

Add to `src/main.rs`:

```rust
use clap::Parser;

/// Stitch TV news clips into infomercial-style audio collages
#[derive(Parser, Debug)]
#[command(name = "badtv", version, about)]
struct Cli {
    /// The phrase to construct from TV clips
    phrase: String,

    /// Output file path
    #[arg(short, long, default_value = "badtv_output.wav")]
    output: String,

    /// Filter to specific stations (repeatable)
    #[arg(long)]
    station: Vec<String>,

    /// Exclude specific stations (repeatable)
    #[arg(long)]
    exclude: Vec<String>,

    /// Skip interactive selection, pick randomly
    #[arg(long)]
    no_tui: bool,

    /// Path to whisper model file
    #[arg(long, default_value = "~/.badtv/ggml-base.en.bin")]
    model: String,

    /// Download the whisper model if not present
    #[arg(long)]
    download_model: bool,

    /// Pitch shift in semitones
    #[arg(long, default_value = "2.0")]
    pitch: f32,

    /// Target loudness in LUFS
    #[arg(long, default_value = "-16.0")]
    loudness: f32,

    /// Crossfade duration in milliseconds
    #[arg(long, default_value = "30")]
    crossfade: u32,

    /// Gap between words in milliseconds
    #[arg(long, default_value = "50")]
    gap: u32,

    /// Reverb wet mix (0-100)
    #[arg(long, default_value = "15")]
    reverb: u32,

    /// Compressor ratio
    #[arg(long, default_value = "4.0")]
    compress: f32,

    /// EQ preset: tv, bright, flat, off
    #[arg(long, default_value = "tv")]
    eq: String,

    /// Limiter ceiling in dB
    #[arg(long, default_value = "-1.0")]
    limit: f32,

    /// Skip all audio processing, just stitch
    #[arg(long)]
    raw: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("Phrase: {}", cli.phrase);
    println!("Output: {}", cli.output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::parse_from(["badtv", "hello world"]);
        assert_eq!(cli.phrase, "hello world");
        assert_eq!(cli.output, "badtv_output.wav");
        assert_eq!(cli.pitch, 2.0);
        assert_eq!(cli.loudness, -16.0);
        assert_eq!(cli.crossfade, 30);
        assert_eq!(cli.gap, 50);
        assert_eq!(cli.reverb, 15);
        assert_eq!(cli.compress, 4.0);
        assert_eq!(cli.eq, "tv");
        assert_eq!(cli.limit, -1.0);
        assert!(!cli.raw);
        assert!(!cli.no_tui);
    }

    #[test]
    fn test_cli_custom_flags() {
        let cli = Cli::parse_from([
            "badtv", "buy now", "-o", "out.wav",
            "--station", "CNN", "--station", "MSNBC",
            "--pitch", "3.5", "--raw", "--no-tui",
        ]);
        assert_eq!(cli.phrase, "buy now");
        assert_eq!(cli.output, "out.wav");
        assert_eq!(cli.station, vec!["CNN", "MSNBC"]);
        assert_eq!(cli.pitch, 3.5);
        assert!(cli.raw);
        assert!(cli.no_tui);
    }
}
```

**Step 2: Run tests**

Run: `cargo test`
Expected: 2 tests pass

**Step 3: Commit**

```
git add src/main.rs
git commit -m "feat: add CLI argument parsing with clap"
```

---

### Task 3: Audio Buffer Types and WAV I/O

**Files:**
- Create: `src/audio.rs`
- Modify: `src/main.rs` (add `mod audio;`)

**Step 1: Write tests for audio types**

```rust
// src/audio.rs

/// A buffer of mono f32 PCM audio samples at a known sample rate.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioBuffer {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self { samples, sample_rate }
    }

    /// Duration in seconds.
    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// Create a silent buffer of a given duration.
    pub fn silence(duration_secs: f32, sample_rate: u32) -> Self {
        let num_samples = (duration_secs * sample_rate as f32) as usize;
        Self {
            samples: vec![0.0; num_samples],
            sample_rate,
        }
    }

    /// Extract a sub-range by time (seconds).
    pub fn slice(&self, start_secs: f32, end_secs: f32) -> Self {
        let start = (start_secs * self.sample_rate as f32) as usize;
        let end = (end_secs * self.sample_rate as f32).min(self.samples.len() as f32) as usize;
        Self {
            samples: self.samples[start..end].to_vec(),
            sample_rate: self.sample_rate,
        }
    }

    /// Write to WAV file.
    pub fn write_wav(&self, path: &str) -> anyhow::Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec)?;
        for &sample in &self.samples {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        Ok(())
    }

    /// Read from WAV file.
    pub fn read_wav(path: &str) -> anyhow::Result<Self> {
        let mut reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => {
                reader.samples::<f32>().map(|s| s.unwrap()).collect()
            }
            hound::SampleFormat::Int => {
                let max = (1 << (spec.bits_per_sample - 1)) as f32;
                reader.samples::<i32>().map(|s| s.unwrap() as f32 / max).collect()
            }
        };
        Ok(Self {
            samples,
            sample_rate: spec.sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration() {
        let buf = AudioBuffer::new(vec![0.0; 44100], 44100);
        assert!((buf.duration() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_silence() {
        let buf = AudioBuffer::silence(0.5, 44100);
        assert_eq!(buf.samples.len(), 22050);
        assert!(buf.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_slice() {
        let buf = AudioBuffer::new(vec![1.0; 44100], 44100);
        let sliced = buf.slice(0.0, 0.5);
        assert_eq!(sliced.samples.len(), 22050);
    }

    #[test]
    fn test_wav_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");
        let path_str = path.to_str().unwrap();

        let original = AudioBuffer::new(vec![0.0, 0.5, -0.5, 1.0, -1.0], 44100);
        original.write_wav(path_str).unwrap();

        let loaded = AudioBuffer::read_wav(path_str).unwrap();
        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.samples.len(), 5);
        for (a, b) in original.samples.iter().zip(loaded.samples.iter()) {
            assert!((a - b).abs() < 0.001);
        }
    }
}
```

**Step 2: Add module to main.rs**

Add `mod audio;` at the top of `src/main.rs`.

**Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass (previous CLI tests + 4 new audio tests)

**Step 4: Commit**

```
git add src/audio.rs src/main.rs
git commit -m "feat: add AudioBuffer type with WAV read/write"
```

---

### Task 4: GDELT TV API Search Client

**Files:**
- Create: `src/search.rs`
- Modify: `src/main.rs` (add `mod search;`)

**Step 1: Define types and write the client**

The GDELT TV API returns JSON like:
```json
{
  "query_details": {"title": "hello station:CNN"},
  "clips": [{
    "preview_url": "https://archive.org/details/SHOW_ID#start/3296/end/3331",
    "ia_show_id": "SHOW_ID",
    "date": "2017-07-05T18:55:11Z",
    "station": "CNN",
    "show": "CNN Newsroom",
    "snippet": "hello, newman."
  }]
}
```

```rust
// src/search.rs

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GdeltResponse {
    pub clips: Option<Vec<Clip>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Clip {
    pub preview_url: String,
    pub ia_show_id: String,
    pub date: String,
    pub station: String,
    pub show: String,
    pub snippet: String,
}

impl Clip {
    /// Parse start/end seconds from preview_url.
    /// URL format: https://archive.org/details/SHOW#start/SECONDS/end/SECONDS
    pub fn time_range(&self) -> Option<(f64, f64)> {
        let fragment = self.preview_url.split('#').nth(1)?;
        let parts: Vec<&str> = fragment.split('/').collect();
        // Expected: ["start", "N", "end", "N"]
        if parts.len() >= 4 && parts[0] == "start" && parts[2] == "end" {
            let start = parts[1].parse::<f64>().ok()?;
            let end = parts[3].parse::<f64>().ok()?;
            Some((start, end))
        } else {
            None
        }
    }

    /// The MP3 download URL for this clip's show.
    pub fn mp3_url(&self) -> String {
        format!(
            "https://archive.org/download/{}/{}.mp3",
            self.ia_show_id, self.ia_show_id
        )
    }
}

/// Search the GDELT TV API for clips containing `word`.
/// `stations` filters to specific stations. If empty, searches a default set.
pub async fn search_word(
    client: &reqwest::Client,
    word: &str,
    stations: &[String],
    exclude: &[String],
    base_url: &str,
) -> Result<Vec<Clip>> {
    // GDELT requires at least one station in the query.
    // If none specified, use a default broad set.
    let default_stations = vec![
        "CNN", "MSNBC", "FOXNEWS", "CNBC", "CSPAN", "BBCNEWS",
        "BLOOMBERG", "FBC",
    ];

    let station_list: Vec<&str> = if stations.is_empty() {
        default_stations
    } else {
        stations.iter().map(|s| s.as_str()).collect()
    };

    let mut all_clips = Vec::new();

    for station in &station_list {
        if exclude.iter().any(|e| e.eq_ignore_ascii_case(station)) {
            continue;
        }

        let query = format!("{} station:{}", word, station);
        let url = format!(
            "{}?query={}&mode=clipgallery&format=json&MAXRECORDS=5",
            base_url,
            urlencoding::encode(&query)
        );

        let resp = client
            .get(&url)
            .send()
            .await
            .context("GDELT API request failed")?;

        let text = resp.text().await?;

        // GDELT returns plain text errors, not JSON, for bad queries
        if let Ok(parsed) = serde_json::from_str::<GdeltResponse>(&text) {
            if let Some(clips) = parsed.clips {
                all_clips.extend(clips);
            }
        }
    }

    Ok(all_clips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_time_range() {
        let clip = Clip {
            preview_url: "https://archive.org/details/SHOW#start/3296/end/3331".to_string(),
            ia_show_id: "SHOW".to_string(),
            date: "2017-07-05T18:55:11Z".to_string(),
            station: "CNN".to_string(),
            show: "CNN Newsroom".to_string(),
            snippet: "hello world".to_string(),
        };
        let (start, end) = clip.time_range().unwrap();
        assert!((start - 3296.0).abs() < 0.01);
        assert!((end - 3331.0).abs() < 0.01);
    }

    #[test]
    fn test_clip_mp3_url() {
        let clip = Clip {
            preview_url: String::new(),
            ia_show_id: "CNNW_20170705_180000_Show".to_string(),
            date: String::new(),
            station: String::new(),
            show: String::new(),
            snippet: String::new(),
        };
        assert_eq!(
            clip.mp3_url(),
            "https://archive.org/download/CNNW_20170705_180000_Show/CNNW_20170705_180000_Show.mp3"
        );
    }

    #[test]
    fn test_clip_time_range_no_fragment() {
        let clip = Clip {
            preview_url: "https://archive.org/details/SHOW".to_string(),
            ia_show_id: "SHOW".to_string(),
            date: String::new(),
            station: String::new(),
            show: String::new(),
            snippet: String::new(),
        };
        assert!(clip.time_range().is_none());
    }
}
```

**Step 2: Add `urlencoding` dependency to Cargo.toml**

Add under `[dependencies]`:
```toml
urlencoding = "2"
```

**Step 3: Add `mod search;` to main.rs**

**Step 4: Run tests**

Run: `cargo test`
Expected: all tests pass

**Step 5: Commit**

```
git add src/search.rs src/main.rs Cargo.toml
git commit -m "feat: add GDELT TV API search client"
```

---

### Task 5: Archive.org Audio Fetch and MP3 Decode

**Files:**
- Create: `src/fetch.rs`
- Modify: `src/main.rs` (add `mod fetch;`)

**Step 1: Write the fetch module**

This module downloads a segment of an MP3 from archive.org and decodes it to PCM
using symphonia. Since full MP3s are ~42MB (1 hour), we download the whole file
but only decode the segment we need (symphonia supports seeking).

```rust
// src/fetch.rs

use anyhow::{bail, Context, Result};
use std::io::Cursor;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::AudioBuffer;

/// Download MP3 from archive.org and decode the segment between
/// `start_secs` and `end_secs` into an AudioBuffer.
/// Adds `padding_secs` on each side for whisper context.
pub async fn fetch_audio_segment(
    client: &reqwest::Client,
    mp3_url: &str,
    start_secs: f64,
    end_secs: f64,
    padding_secs: f64,
) -> Result<AudioBuffer> {
    let padded_start = (start_secs - padding_secs).max(0.0);
    let padded_end = end_secs + padding_secs;

    // Download the full MP3 — archive.org MP3s are typically 30-50MB.
    // We decode only the segment we need.
    let bytes = client
        .get(mp3_url)
        .send()
        .await
        .context("Failed to download MP3 from archive.org")?
        .bytes()
        .await
        .context("Failed to read MP3 bytes")?;

    decode_mp3_segment(&bytes, padded_start, padded_end)
}

/// Decode a segment of an MP3 byte buffer into mono f32 PCM.
pub fn decode_mp3_segment(mp3_bytes: &[u8], start_secs: f64, end_secs: f64) -> Result<AudioBuffer> {
    let cursor = Cursor::new(mp3_bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .context("Failed to probe MP3 format")?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .context("No audio track found in MP3")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .context("No sample rate in MP3")?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1);
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("Failed to create MP3 decoder")?;

    let start_sample = (start_secs * sample_rate as f64) as u64;
    let end_sample = (end_secs * sample_rate as f64) as u64;

    let mut all_samples: Vec<f32> = Vec::new();
    let mut current_sample: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => bail!("Error reading MP3 packet: {}", e),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let num_frames = decoded.frames() as u64;

        let packet_end = current_sample + num_frames;

        // Skip packets before our segment
        if packet_end < start_sample {
            current_sample = packet_end;
            continue;
        }

        // Stop after our segment
        if current_sample > end_sample {
            break;
        }

        let mut sample_buf = SampleBuffer::<f32>::new(decoded.frames() as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);

        let interleaved = sample_buf.samples();

        // Convert to mono and extract only the portion in our range
        for frame in 0..decoded.frames() {
            let global_sample = current_sample + frame as u64;
            if global_sample >= start_sample && global_sample <= end_sample {
                // Average channels to mono
                let mut sum = 0.0f32;
                for ch in 0..channels {
                    sum += interleaved[frame * channels + ch];
                }
                all_samples.push(sum / channels as f32);
            }
        }

        current_sample = packet_end;
    }

    if all_samples.is_empty() {
        bail!(
            "No audio decoded for segment {:.1}s - {:.1}s",
            start_secs,
            end_secs
        );
    }

    Ok(AudioBuffer::new(all_samples, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration test: create a WAV, check we can round-trip
    // Real MP3 decode tests would need test fixtures
    #[test]
    fn test_audio_buffer_from_decode_returns_error_on_empty() {
        let result = decode_mp3_segment(&[], 0.0, 1.0);
        assert!(result.is_err());
    }
}
```

**Step 2: Add `mod fetch;` to main.rs**

**Step 3: Run tests**

Run: `cargo test`
Expected: all tests pass

**Step 4: Commit**

```
git add src/fetch.rs src/main.rs
git commit -m "feat: add archive.org audio fetch and MP3 decode"
```

---

### Task 6: Whisper Word Alignment

**Files:**
- Create: `src/align.rs`
- Create: `src/model.rs`
- Modify: `src/main.rs` (add `mod align; mod model;`)

**Step 1: Write the model management module**

```rust
// src/model.rs

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

/// Resolve the model path, expanding ~ to home dir.
pub fn resolve_model_path(path: &str) -> Result<PathBuf> {
    if path.starts_with("~/") {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        Ok(home.join(&path[2..]))
    } else {
        Ok(PathBuf::from(path))
    }
}

/// Download the whisper model to the given path.
pub async fn download_model(client: &reqwest::Client, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create model directory")?;
    }

    eprintln!("Downloading Whisper model to {}...", dest.display());
    eprintln!("This is a one-time download (~142MB).");

    let resp = client
        .get(MODEL_URL)
        .send()
        .await
        .context("Failed to download whisper model")?;

    if !resp.status().is_success() {
        bail!("Model download failed with status: {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    std::fs::write(dest, &bytes)
        .context("Failed to write model file")?;

    eprintln!("Model downloaded successfully.");
    Ok(())
}

/// Check that the model file exists, return a helpful error if not.
pub fn ensure_model_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!(
            "Whisper model not found at: {}\n\
             Run `badtv --download-model` to download it.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute_path() {
        let path = resolve_model_path("/tmp/model.bin").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/model.bin"));
    }

    #[test]
    fn test_resolve_tilde_path() {
        let path = resolve_model_path("~/.badtv/model.bin").unwrap();
        assert!(path.to_str().unwrap().contains(".badtv/model.bin"));
        assert!(!path.to_str().unwrap().starts_with("~"));
    }

    #[test]
    fn test_ensure_model_missing() {
        let result = ensure_model_exists(Path::new("/nonexistent/model.bin"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("badtv --download-model"));
    }
}
```

**Step 2: Add `dirs` dependency to Cargo.toml**

```toml
dirs = "6"
```

**Step 3: Write the alignment module**

```rust
// src/align.rs

use anyhow::{bail, Context, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::AudioBuffer;

/// A word with its timestamp in the audio.
#[derive(Debug, Clone)]
pub struct AlignedWord {
    pub text: String,
    pub start_secs: f32,
    pub end_secs: f32,
}

/// Run whisper on an AudioBuffer and return word-level timestamps.
pub fn align_words(audio: &AudioBuffer, model_path: &Path) -> Result<Vec<AlignedWord>> {
    // Whisper expects 16kHz mono f32 audio
    let samples_16k = resample_to_16k(audio)?;

    let ctx = WhisperContext::new_with_params(
        model_path.to_str().context("Invalid model path")?,
        WhisperContextParameters::default(),
    )
    .context("Failed to load whisper model")?;

    let mut state = ctx.create_state().context("Failed to create whisper state")?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_token_timestamps(true);
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, &samples_16k)
        .context("Whisper transcription failed")?;

    let num_segments = state.full_n_segments()?;
    let mut words = Vec::new();

    for seg_idx in 0..num_segments {
        let num_tokens = state.full_n_tokens(seg_idx)?;
        for tok_idx in 0..num_tokens {
            let token_text = state
                .full_get_token_text(seg_idx, tok_idx)?;
            let token_data = state.full_get_token_data(seg_idx, tok_idx)?;

            let text = token_text.trim().to_string();
            if text.is_empty() || text.starts_with('[') {
                continue;
            }

            let start = token_data.t0 as f32 / 100.0; // centiseconds to seconds
            let end = token_data.t1 as f32 / 100.0;

            words.push(AlignedWord {
                text,
                start_secs: start,
                end_secs: end,
            });
        }
    }

    Ok(words)
}

/// Find the best match for `target_word` in aligned words.
/// Returns the AudioBuffer sliced to just that word (with padding).
pub fn extract_word(
    audio: &AudioBuffer,
    words: &[AlignedWord],
    target_word: &str,
    padding_ms: f32,
) -> Result<AudioBuffer> {
    let target_lower = target_word.to_lowercase();

    let best = words
        .iter()
        .find(|w| w.text.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()) == target_lower)
        .context(format!("Word '{}' not found in whisper output", target_word))?;

    let padding_secs = padding_ms / 1000.0;
    let start = (best.start_secs - padding_secs).max(0.0);
    let end = (best.end_secs + padding_secs).min(audio.duration());

    Ok(audio.slice(start, end))
}

/// Resample audio to 16kHz for whisper.
fn resample_to_16k(audio: &AudioBuffer) -> Result<Vec<f32>> {
    if audio.sample_rate == 16000 {
        return Ok(audio.samples.clone());
    }

    use rubato::{FftFixedIn, Resampler};

    let mut resampler = FftFixedIn::<f32>::new(
        audio.sample_rate as usize,
        16000,
        audio.samples.len().min(1024),
        1, // sub_chunks
        1, // channels
    )
    .context("Failed to create resampler")?;

    let mut output = Vec::new();
    let chunk_size = resampler.input_frames_next();

    for chunk in audio.samples.chunks(chunk_size) {
        let mut input = vec![chunk.to_vec()];
        // Pad last chunk if needed
        if input[0].len() < chunk_size {
            input[0].resize(chunk_size, 0.0);
        }
        let result = resampler.process(&input, None)?;
        output.extend_from_slice(&result[0]);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_word_finds_match() {
        let audio = AudioBuffer::new(vec![0.5; 44100], 44100); // 1 second
        let words = vec![
            AlignedWord { text: "hello".into(), start_secs: 0.1, end_secs: 0.4 },
            AlignedWord { text: "world".into(), start_secs: 0.5, end_secs: 0.8 },
        ];

        let result = extract_word(&audio, &words, "world", 20.0).unwrap();
        // Should be roughly 0.48 to 0.82 = 0.34 seconds
        assert!(result.duration() > 0.3);
        assert!(result.duration() < 0.4);
    }

    #[test]
    fn test_extract_word_case_insensitive() {
        let audio = AudioBuffer::new(vec![0.5; 44100], 44100);
        let words = vec![
            AlignedWord { text: "Hello".into(), start_secs: 0.1, end_secs: 0.4 },
        ];

        let result = extract_word(&audio, &words, "hello", 20.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_word_not_found() {
        let audio = AudioBuffer::new(vec![0.5; 44100], 44100);
        let words = vec![
            AlignedWord { text: "hello".into(), start_secs: 0.1, end_secs: 0.4 },
        ];

        let result = extract_word(&audio, &words, "goodbye", 20.0);
        assert!(result.is_err());
    }
}
```

**Step 4: Add modules to main.rs**

Add `mod align; mod model;` to `src/main.rs`.

**Step 5: Run tests**

Run: `cargo test`
Expected: all tests pass (whisper-dependent tests are unit-tested with mock data,
no model needed)

**Step 6: Commit**

```
git add src/align.rs src/model.rs src/main.rs Cargo.toml
git commit -m "feat: add whisper word alignment and model management"
```

---

### Task 7: Audio Processors — Loudness Normalization

**Files:**
- Create: `src/process/mod.rs`
- Create: `src/process/normalize.rs`
- Modify: `src/main.rs` (add `mod process;`)

**Step 1: Create process module and normalization**

```rust
// src/process/mod.rs
pub mod normalize;
pub mod pitch;
pub mod eq;
pub mod compressor;
pub mod reverb;
pub mod limiter;
pub mod crossfade;

use crate::audio::AudioBuffer;

/// Parameters for the full processing pipeline.
#[derive(Debug, Clone)]
pub struct ProcessParams {
    pub loudness_lufs: f32,
    pub pitch_semitones: f32,
    pub eq_preset: String,
    pub compress_ratio: f32,
    pub crossfade_ms: u32,
    pub gap_ms: u32,
    pub reverb_wet: u32,
    pub limit_db: f32,
}

impl Default for ProcessParams {
    fn default() -> Self {
        Self {
            loudness_lufs: -16.0,
            pitch_semitones: 2.0,
            eq_preset: "tv".to_string(),
            compress_ratio: 4.0,
            crossfade_ms: 30,
            gap_ms: 50,
            reverb_wet: 15,
            limit_db: -1.0,
        }
    }
}

/// Apply the full infomercial processing pipeline to a list of word clips.
/// Returns a single assembled AudioBuffer.
pub fn apply_pipeline(clips: &[AudioBuffer], params: &ProcessParams) -> AudioBuffer {
    let sample_rate = clips.first().map(|c| c.sample_rate).unwrap_or(44100);

    // Process each clip individually
    let processed: Vec<AudioBuffer> = clips
        .iter()
        .map(|clip| {
            let mut buf = clip.clone();
            normalize::normalize_loudness(&mut buf.samples, params.loudness_lufs);
            if params.pitch_semitones.abs() > 0.01 {
                buf.samples = pitch::shift(&buf.samples, buf.sample_rate, params.pitch_semitones);
            }
            eq::apply_eq(&mut buf.samples, buf.sample_rate, &params.eq_preset);
            compressor::compress(&mut buf.samples, params.compress_ratio);
            buf
        })
        .collect();

    // Stitch clips together with crossfade and gaps
    let mut assembled = crossfade::stitch(&processed, params.crossfade_ms, params.gap_ms);

    // Apply reverb and limiter to the full assembled output
    if params.reverb_wet > 0 {
        reverb::apply_reverb(&mut assembled.samples, assembled.sample_rate, params.reverb_wet);
    }
    limiter::limit(&mut assembled.samples, params.limit_db);

    assembled
}
```

```rust
// src/process/normalize.rs

/// Compute RMS loudness of samples.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Convert RMS to approximate LUFS (simplified — true LUFS uses K-weighting).
/// This is a practical approximation for short clips.
fn rms_to_lufs(rms: f32) -> f32 {
    if rms <= 0.0 {
        return -100.0;
    }
    20.0 * rms.log10()
}

/// Normalize samples to target LUFS level.
pub fn normalize_loudness(samples: &mut [f32], target_lufs: f32) {
    let current_rms = rms(samples);
    let current_lufs = rms_to_lufs(current_rms);

    if current_lufs <= -100.0 {
        return; // silence, nothing to normalize
    }

    let gain_db = target_lufs - current_lufs;
    let gain_linear = 10.0f32.powf(gain_db / 20.0);

    for sample in samples.iter_mut() {
        *sample *= gain_linear;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_silence() {
        assert_eq!(rms(&[0.0; 100]), 0.0);
    }

    #[test]
    fn test_rms_known_signal() {
        // Full-scale sine wave: RMS = 1/sqrt(2) ≈ 0.707
        let samples: Vec<f32> = (0..44100)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let r = rms(&samples);
        assert!((r - 0.707).abs() < 0.01);
    }

    #[test]
    fn test_normalize_loudness_increases_quiet() {
        let mut samples = vec![0.01; 1000];
        let rms_before = rms(&samples);
        normalize_loudness(&mut samples, -16.0);
        let rms_after = rms(&samples);
        assert!(rms_after > rms_before);
    }

    #[test]
    fn test_normalize_loudness_decreases_loud() {
        let mut samples = vec![0.9; 1000];
        let rms_before = rms(&samples);
        normalize_loudness(&mut samples, -20.0);
        let rms_after = rms(&samples);
        assert!(rms_after < rms_before);
    }
}
```

**Step 2: Add `mod process;` to main.rs**

**Step 3: Run tests**

Run: `cargo test process::normalize`
Expected: 4 tests pass

**Step 4: Commit**

```
git add src/process/ src/main.rs
git commit -m "feat: add loudness normalization processor"
```

---

### Task 8: Audio Processors — Pitch Shifting

**Files:**
- Create: `src/process/pitch.rs`

**Step 1: Write pitch shifter**

```rust
// src/process/pitch.rs

use pitch_shift::PitchShifter;

/// Shift the pitch of samples by `semitones`.
/// Positive = higher, negative = lower.
pub fn shift(samples: &[f32], sample_rate: u32, semitones: f32) -> Vec<f32> {
    if samples.is_empty() || semitones.abs() < 0.01 {
        return samples.to_vec();
    }

    let shift_factor = 2.0f32.powf(semitones / 12.0);
    let fft_size = 2048;

    let mut shifter = PitchShifter::new(fft_size, sample_rate as usize);
    let mut output = samples.to_vec();

    shifter.shift(&mut output, shift_factor);

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shift_zero_is_identity() {
        let samples: Vec<f32> = (0..4410)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let shifted = shift(&samples, 44100, 0.0);
        assert_eq!(shifted.len(), samples.len());
        // With 0 shift, output should be very close to input
        for (a, b) in samples.iter().zip(shifted.iter()) {
            assert!((a - b).abs() < 0.1, "a={}, b={}", a, b);
        }
    }

    #[test]
    fn test_shift_preserves_length() {
        let samples = vec![0.5; 44100];
        let shifted = shift(&samples, 44100, 5.0);
        assert_eq!(shifted.len(), samples.len());
    }

    #[test]
    fn test_shift_empty() {
        let shifted = shift(&[], 44100, 3.0);
        assert!(shifted.is_empty());
    }
}
```

**Step 2: Run tests**

Run: `cargo test process::pitch`
Expected: 3 tests pass

**Step 3: Commit**

```
git add src/process/pitch.rs
git commit -m "feat: add pitch shifting processor"
```

---

### Task 9: Audio Processors — EQ (Biquad Filter)

**Files:**
- Create: `src/process/eq.rs`

**Step 1: Write biquad EQ**

```rust
// src/process/eq.rs

use std::f32::consts::PI;

/// Biquad filter coefficients.
struct Biquad {
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,
}

impl Biquad {
    /// Peaking EQ filter (Audio EQ Cookbook by Robert Bristow-Johnson).
    fn peaking(sample_rate: u32, freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * w0.cos();
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * w0.cos();
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: a1 / a0, a2: a2 / a0,
        }
    }

    /// Low-shelf filter.
    fn low_shelf(sample_rate: u32, freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * q);
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) + (a - 1.0) * w0.cos() + two_sqrt_a_alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * w0.cos());
        let a2 = (a + 1.0) + (a - 1.0) * w0.cos() - two_sqrt_a_alpha;
        let b0 = a * ((a + 1.0) - (a - 1.0) * w0.cos() + two_sqrt_a_alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * w0.cos());
        let b2 = a * ((a + 1.0) - (a - 1.0) * w0.cos() - two_sqrt_a_alpha);

        Self {
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: a1 / a0, a2: a2 / a0,
        }
    }

    /// High-shelf filter.
    fn high_shelf(sample_rate: u32, freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sample_rate as f32;
        let alpha = w0.sin() / (2.0 * q);
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let a0 = (a + 1.0) - (a - 1.0) * w0.cos() + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * w0.cos());
        let a2 = (a + 1.0) - (a - 1.0) * w0.cos() - two_sqrt_a_alpha;
        let b0 = a * ((a + 1.0) + (a - 1.0) * w0.cos() + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * w0.cos());
        let b2 = a * ((a + 1.0) + (a - 1.0) * w0.cos() - two_sqrt_a_alpha);

        Self {
            b0: b0 / a0, b1: b1 / a0, b2: b2 / a0,
            a1: a1 / a0, a2: a2 / a0,
        }
    }

    fn process(&self, samples: &mut [f32]) {
        let mut x1 = 0.0f32;
        let mut x2 = 0.0f32;
        let mut y1 = 0.0f32;
        let mut y2 = 0.0f32;

        for sample in samples.iter_mut() {
            let x0 = *sample;
            let y0 = self.b0 * x0 + self.b1 * x1 + self.b2 * x2
                   - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            *sample = y0;
        }
    }
}

/// Apply EQ preset to samples.
/// Presets: "tv", "bright", "flat", "off"
pub fn apply_eq(samples: &mut [f32], sample_rate: u32, preset: &str) {
    match preset {
        "off" | "flat" => {} // no processing
        "tv" => {
            // Mimic TV speaker: mid-boost, bass cut, treble roll-off
            Biquad::low_shelf(sample_rate, 200.0, -3.0, 0.7).process(samples);
            Biquad::peaking(sample_rate, 2000.0, 4.0, 1.0).process(samples);
            Biquad::high_shelf(sample_rate, 8000.0, -4.0, 0.7).process(samples);
        }
        "bright" => {
            // Bright, present — boosted highs and upper mids
            Biquad::peaking(sample_rate, 3000.0, 3.0, 1.0).process(samples);
            Biquad::high_shelf(sample_rate, 6000.0, 3.0, 0.7).process(samples);
        }
        _ => {
            eprintln!("Unknown EQ preset '{}', using flat", preset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eq_off_is_identity() {
        let original = vec![0.5, -0.3, 0.1, 0.0, -0.7];
        let mut samples = original.clone();
        apply_eq(&mut samples, 44100, "off");
        assert_eq!(samples, original);
    }

    #[test]
    fn test_eq_tv_modifies_signal() {
        let mut samples: Vec<f32> = (0..4410)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let original = samples.clone();
        apply_eq(&mut samples, 44100, "tv");
        // Signal should be modified
        let diff: f32 = samples.iter().zip(original.iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.0);
    }

    #[test]
    fn test_eq_preserves_length() {
        let mut samples = vec![0.1; 1000];
        apply_eq(&mut samples, 44100, "bright");
        assert_eq!(samples.len(), 1000);
    }
}
```

**Step 2: Run tests**

Run: `cargo test process::eq`
Expected: 3 tests pass

**Step 3: Commit**

```
git add src/process/eq.rs
git commit -m "feat: add biquad EQ processor with TV/bright presets"
```

---

### Task 10: Audio Processors — Compressor

**Files:**
- Create: `src/process/compressor.rs`

**Step 1: Write compressor**

```rust
// src/process/compressor.rs

/// Simple envelope-follower compressor.
/// `ratio`: compression ratio (e.g. 4.0 = 4:1)
/// Uses a fixed threshold of -20 dBFS, fast attack (1ms), medium release (50ms).
pub fn compress(samples: &mut [f32], ratio: f32) {
    if ratio <= 1.0 || samples.is_empty() {
        return;
    }

    let threshold = 10.0f32.powf(-20.0 / 20.0); // -20 dBFS ≈ 0.1
    let attack_coeff = (-1.0f32 / (0.001 * 44100.0)).exp();  // 1ms
    let release_coeff = (-1.0f32 / (0.050 * 44100.0)).exp();  // 50ms

    let mut envelope = 0.0f32;

    for sample in samples.iter_mut() {
        let abs_sample = sample.abs();

        // Envelope follower
        if abs_sample > envelope {
            envelope = attack_coeff * envelope + (1.0 - attack_coeff) * abs_sample;
        } else {
            envelope = release_coeff * envelope + (1.0 - release_coeff) * abs_sample;
        }

        // Gain computation
        if envelope > threshold {
            let over_db = 20.0 * (envelope / threshold).log10();
            let compressed_db = over_db / ratio;
            let gain_reduction_db = over_db - compressed_db;
            let gain = 10.0f32.powf(-gain_reduction_db / 20.0);
            *sample *= gain;
        }
    }
}

/// Apply makeup gain to bring level back up after compression.
pub fn makeup_gain(samples: &mut [f32], gain_db: f32) {
    let gain = 10.0f32.powf(gain_db / 20.0);
    for sample in samples.iter_mut() {
        *sample *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_reduces_loud_signal() {
        // Signal above threshold
        let mut samples: Vec<f32> = vec![0.8; 4410];
        let peak_before = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        compress(&mut samples, 4.0);
        let peak_after = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(peak_after < peak_before, "Compressor should reduce loud signal");
    }

    #[test]
    fn test_compress_leaves_quiet_signal() {
        // Signal well below threshold (-20 dBFS ≈ 0.1)
        let mut samples: Vec<f32> = vec![0.01; 4410];
        let original = samples.clone();
        compress(&mut samples, 4.0);
        // Should be nearly unchanged
        for (a, b) in samples.iter().zip(original.iter()) {
            assert!((a - b).abs() < 0.001);
        }
    }

    #[test]
    fn test_compress_ratio_1_is_noop() {
        let mut samples: Vec<f32> = vec![0.8; 1000];
        let original = samples.clone();
        compress(&mut samples, 1.0);
        assert_eq!(samples, original);
    }
}
```

**Step 2: Run tests**

Run: `cargo test process::compressor`
Expected: 3 tests pass

**Step 3: Commit**

```
git add src/process/compressor.rs
git commit -m "feat: add dynamics compressor processor"
```

---

### Task 11: Audio Processors — Reverb (Freeverb)

**Files:**
- Create: `src/process/reverb.rs`

**Step 1: Write Freeverb implementation**

```rust
// src/process/reverb.rs

/// Comb filter for reverb.
struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
    damp: f32,
    damp_prev: f32,
}

impl CombFilter {
    fn new(size: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
            feedback,
            damp,
            damp_prev: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.index];
        self.damp_prev = output * (1.0 - self.damp) + self.damp_prev * self.damp;
        self.buffer[self.index] = input + self.damp_prev * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

/// All-pass filter for reverb.
struct AllPassFilter {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
}

impl AllPassFilter {
    fn new(size: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.index];
        let output = -input + buffered;
        self.buffer[self.index] = input + buffered * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        output
    }
}

/// Freeverb-style reverb processor.
struct Freeverb {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllPassFilter>,
}

impl Freeverb {
    fn new(sample_rate: u32) -> Self {
        let scale = sample_rate as f32 / 44100.0;

        // Freeverb comb filter delays (in samples at 44100Hz)
        let comb_sizes = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let allpass_sizes = [556, 441, 341, 225];

        let feedback = 0.84;
        let damp = 0.2;

        let combs = comb_sizes
            .iter()
            .map(|&s| CombFilter::new((s as f32 * scale) as usize, feedback, damp))
            .collect();

        let allpasses = allpass_sizes
            .iter()
            .map(|&s| AllPassFilter::new((s as f32 * scale) as usize, 0.5))
            .collect();

        Self { combs, allpasses }
    }

    fn process_sample(&mut self, input: f32) -> f32 {
        let mut output = 0.0;
        for comb in &mut self.combs {
            output += comb.process(input);
        }
        for allpass in &mut self.allpasses {
            output = allpass.process(output);
        }
        output
    }
}

/// Apply reverb to samples.
/// `wet_percent`: 0-100, how much reverb to mix in.
pub fn apply_reverb(samples: &mut [f32], sample_rate: u32, wet_percent: u32) {
    if wet_percent == 0 || samples.is_empty() {
        return;
    }

    let wet = wet_percent.min(100) as f32 / 100.0;
    let dry = 1.0 - wet;

    let mut reverb = Freeverb::new(sample_rate);

    for sample in samples.iter_mut() {
        let dry_signal = *sample;
        let wet_signal = reverb.process_sample(dry_signal);
        *sample = dry_signal * dry + wet_signal * wet;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverb_zero_wet_is_identity() {
        let original = vec![0.5, -0.3, 0.1, 0.8, -0.6];
        let mut samples = original.clone();
        apply_reverb(&mut samples, 44100, 0);
        assert_eq!(samples, original);
    }

    #[test]
    fn test_reverb_modifies_signal() {
        // Impulse signal — reverb should create a tail
        let mut samples = vec![0.0; 44100];
        samples[0] = 1.0;
        let original = samples.clone();

        apply_reverb(&mut samples, 44100, 50);

        // Later samples should now be nonzero (reverb tail)
        let tail_energy: f32 = samples[10000..20000].iter().map(|s| s.abs()).sum();
        let orig_tail: f32 = original[10000..20000].iter().map(|s| s.abs()).sum();
        assert!(tail_energy > orig_tail, "Reverb should create a tail after impulse");
    }

    #[test]
    fn test_reverb_preserves_length() {
        let mut samples = vec![0.1; 1000];
        apply_reverb(&mut samples, 44100, 30);
        assert_eq!(samples.len(), 1000);
    }
}
```

**Step 2: Run tests**

Run: `cargo test process::reverb`
Expected: 3 tests pass

**Step 3: Commit**

```
git add src/process/reverb.rs
git commit -m "feat: add Freeverb reverb processor"
```

---

### Task 12: Audio Processors — Limiter

**Files:**
- Create: `src/process/limiter.rs`

**Step 1: Write brick-wall limiter**

```rust
// src/process/limiter.rs

/// Brick-wall limiter.
/// `ceiling_db`: maximum output level in dBFS (e.g. -1.0).
pub fn limit(samples: &mut [f32], ceiling_db: f32) {
    let ceiling = 10.0f32.powf(ceiling_db / 20.0);

    for sample in samples.iter_mut() {
        if *sample > ceiling {
            *sample = ceiling;
        } else if *sample < -ceiling {
            *sample = -ceiling;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limit_clips_loud() {
        let mut samples = vec![0.0, 0.5, 1.0, -1.0, 1.5, -1.5];
        limit(&mut samples, -1.0);
        let ceiling = 10.0f32.powf(-1.0 / 20.0); // ≈ 0.891
        for s in &samples {
            assert!(s.abs() <= ceiling + 0.001, "Sample {} exceeds ceiling {}", s, ceiling);
        }
    }

    #[test]
    fn test_limit_preserves_quiet() {
        let original = vec![0.0, 0.1, -0.1, 0.05];
        let mut samples = original.clone();
        limit(&mut samples, -1.0);
        assert_eq!(samples, original);
    }

    #[test]
    fn test_limit_0db() {
        let mut samples = vec![1.5, -1.5, 0.5];
        limit(&mut samples, 0.0);
        assert!((samples[0] - 1.0).abs() < 0.001);
        assert!((samples[1] + 1.0).abs() < 0.001);
        assert!((samples[2] - 0.5).abs() < 0.001);
    }
}
```

**Step 2: Run tests**

Run: `cargo test process::limiter`
Expected: 3 tests pass

**Step 3: Commit**

```
git add src/process/limiter.rs
git commit -m "feat: add brick-wall limiter processor"
```

---

### Task 13: Audio Processors — Crossfade and Stitching

**Files:**
- Create: `src/process/crossfade.rs`

**Step 1: Write crossfade and stitching**

```rust
// src/process/crossfade.rs

use crate::audio::AudioBuffer;

/// Stitch multiple audio clips together with crossfading and gaps.
pub fn stitch(clips: &[AudioBuffer], crossfade_ms: u32, gap_ms: u32) -> AudioBuffer {
    if clips.is_empty() {
        return AudioBuffer::new(vec![], 44100);
    }

    let sample_rate = clips[0].sample_rate;
    let crossfade_samples = (crossfade_ms as f32 / 1000.0 * sample_rate as f32) as usize;
    let gap_samples = (gap_ms as f32 / 1000.0 * sample_rate as f32) as usize;

    let mut output: Vec<f32> = Vec::new();

    for (i, clip) in clips.iter().enumerate() {
        if i == 0 {
            output.extend_from_slice(&clip.samples);
            continue;
        }

        // Add gap (silence)
        if gap_samples > 0 {
            output.extend(std::iter::repeat(0.0f32).take(gap_samples));
        }

        // Crossfade: overlap the end of output with start of next clip
        let xfade_len = crossfade_samples.min(output.len()).min(clip.samples.len());

        if xfade_len > 0 {
            let output_start = output.len() - xfade_len;
            for j in 0..xfade_len {
                let t = j as f32 / xfade_len as f32;
                // Fade out the tail, fade in the new clip
                output[output_start + j] = output[output_start + j] * (1.0 - t)
                    + clip.samples[j] * t;
            }
            // Append the rest of the clip after the crossfade region
            output.extend_from_slice(&clip.samples[xfade_len..]);
        } else {
            output.extend_from_slice(&clip.samples);
        }
    }

    AudioBuffer::new(output, sample_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stitch_empty() {
        let result = stitch(&[], 30, 50);
        assert!(result.samples.is_empty());
    }

    #[test]
    fn test_stitch_single_clip() {
        let clip = AudioBuffer::new(vec![1.0; 100], 44100);
        let result = stitch(&[clip.clone()], 30, 50);
        assert_eq!(result.samples.len(), 100);
    }

    #[test]
    fn test_stitch_two_clips_with_gap() {
        let a = AudioBuffer::new(vec![1.0; 1000], 44100);
        let b = AudioBuffer::new(vec![0.5; 1000], 44100);
        let result = stitch(&[a, b], 0, 100); // ~100ms gap at 44100Hz ≈ 4410 samples... but 100ms

        let gap_samples = (0.1 * 44100.0) as usize; // 4410
        // Total should be 1000 + 4410 + 1000 = 6410
        assert_eq!(result.samples.len(), 1000 + gap_samples + 1000);
    }

    #[test]
    fn test_stitch_crossfade_blends() {
        let a = AudioBuffer::new(vec![1.0; 1000], 44100);
        let b = AudioBuffer::new(vec![0.0; 1000], 44100);
        let result = stitch(&[a, b], 500, 0); // large crossfade, no gap

        // The crossfade region should have values between 0 and 1
        // (output end fading to 0, input start fading from 0)
        let xfade_samples = (0.5 * 44100.0) as usize;
        let xfade_start = 1000 - xfade_samples.min(1000);
        // After crossfade, we have the rest of clip b
        // Check some midpoint values are blended
        let midpoint = xfade_start + xfade_samples.min(1000) / 2;
        if midpoint < result.samples.len() {
            let val = result.samples[midpoint];
            assert!(val > 0.0 && val < 1.0, "Crossfade midpoint should be blended, got {}", val);
        }
    }
}
```

**Step 2: Run tests**

Run: `cargo test process::crossfade`
Expected: 4 tests pass

**Step 3: Commit**

```
git add src/process/crossfade.rs
git commit -m "feat: add crossfade and stitching processor"
```

---

### Task 14: Fallback Chain (Variations + TTS)

**Files:**
- Create: `src/fallback.rs`
- Modify: `src/main.rs` (add `mod fallback;`)

**Step 1: Write fallback module**

```rust
// src/fallback.rs

use anyhow::{Context, Result};
use crate::audio::AudioBuffer;

/// Generate word variations to try when exact match fails.
pub fn word_variations(word: &str) -> Vec<String> {
    let mut variations = vec![word.to_string()];
    let lower = word.to_lowercase();

    // Plural/singular
    if lower.ends_with('s') {
        variations.push(lower[..lower.len()-1].to_string());
    } else {
        variations.push(format!("{}s", lower));
    }

    // Common verb tenses
    if lower.ends_with("ing") {
        // running -> run
        let stem = &lower[..lower.len()-3];
        variations.push(stem.to_string());
        // running -> runs
        variations.push(format!("{}s", stem));
    } else if lower.ends_with("ed") {
        let stem = &lower[..lower.len()-2];
        variations.push(stem.to_string());
        variations.push(format!("{}ing", stem));
    } else {
        variations.push(format!("{}ing", lower));
        variations.push(format!("{}ed", lower));
    }

    // Deduplicate
    variations.sort();
    variations.dedup();
    variations
}

/// Generate speech for a word using system TTS.
/// Returns an AudioBuffer at 44100Hz mono.
pub fn tts_word(word: &str) -> Result<AudioBuffer> {
    // Use the tts crate which delegates to system TTS
    // On macOS this uses AVSpeechSynthesizer, on Linux espeak
    //
    // For now, generate via command-line `say` on macOS as a simpler approach
    // that produces a WAV file we can read back.
    let dir = tempfile::tempdir().context("Failed to create temp dir for TTS")?;
    let aiff_path = dir.path().join("tts.aiff");
    let wav_path = dir.path().join("tts.wav");

    // macOS `say` command outputs AIFF
    let status = std::process::Command::new("say")
        .args(["-o", aiff_path.to_str().unwrap(), word])
        .status()
        .context("TTS failed — 'say' command not found (macOS only for now)")?;

    if !status.success() {
        anyhow::bail!("TTS 'say' command failed");
    }

    // Convert AIFF to WAV using afconvert (macOS)
    let status = std::process::Command::new("afconvert")
        .args([
            "-f", "WAVE",
            "-d", "LEI32",
            aiff_path.to_str().unwrap(),
            wav_path.to_str().unwrap(),
        ])
        .status()
        .context("afconvert failed")?;

    if !status.success() {
        anyhow::bail!("afconvert failed to convert AIFF to WAV");
    }

    AudioBuffer::read_wav(wav_path.to_str().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_variations_basic() {
        let vars = word_variations("buy");
        assert!(vars.contains(&"buy".to_string()));
        assert!(vars.contains(&"buys".to_string()));
        assert!(vars.contains(&"buying".to_string()));
    }

    #[test]
    fn test_word_variations_plural() {
        let vars = word_variations("cats");
        assert!(vars.contains(&"cats".to_string()));
        assert!(vars.contains(&"cat".to_string()));
    }

    #[test]
    fn test_word_variations_ing() {
        let vars = word_variations("running");
        assert!(vars.contains(&"running".to_string()));
        assert!(vars.contains(&"run".to_string()));
        assert!(vars.contains(&"runs".to_string()));
    }

    #[test]
    fn test_word_variations_ed() {
        let vars = word_variations("jumped");
        assert!(vars.contains(&"jumped".to_string()));
        assert!(vars.contains(&"jump".to_string()));
        assert!(vars.contains(&"jumping".to_string()));
    }
}
```

**Step 2: Add `tempfile` as a regular dependency (not just dev)**

It's already in `[dev-dependencies]`. Add it to `[dependencies]` too:
```toml
tempfile = "3"
```

**Step 3: Add `mod fallback;` to main.rs**

**Step 4: Run tests**

Run: `cargo test fallback`
Expected: 4 tests pass

**Step 5: Commit**

```
git add src/fallback.rs src/main.rs Cargo.toml
git commit -m "feat: add word variation fallback and TTS"
```

---

### Task 15: Interactive TUI

**Files:**
- Create: `src/tui.rs`
- Modify: `src/main.rs` (add `mod tui;`)

**Step 1: Write the TUI module**

This is the interactive clip selection interface using ratatui + crossterm.

```rust
// src/tui.rs

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
use std::io::stdout;

use crate::search::Clip;

/// Result of clip selection for one word.
#[derive(Debug)]
pub enum ClipChoice {
    Selected(Clip),
    Random,
    Quit,
}

/// Show interactive TUI for selecting a clip for one word.
/// Returns the user's choice.
pub fn select_clip(word: &str, clips: &[Clip], word_index: usize, total_words: usize) -> Result<ClipChoice> {
    if clips.is_empty() {
        eprintln!("No clips found for '{}', will use fallback", word);
        return Ok(ClipChoice::Random);
    }

    enable_raw_mode().context("Failed to enable raw mode")?;
    stdout().execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut state = ListState::default();
    state.select(Some(0));

    let result = run_selection_loop(&mut terminal, word, clips, &mut state, word_index, total_words);

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_selection_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    word: &str,
    clips: &[Clip],
    state: &mut ListState,
    word_index: usize,
    total_words: usize,
) -> Result<ClipChoice> {
    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(3)])
                .split(frame.area());

            // Header
            let header = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" Word {}/{}: ", word_index + 1, total_words),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("\"{}\"", word),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]))
            .block(Block::default().borders(Borders::ALL).title("BadTV"));

            // Clip list
            let items: Vec<ListItem> = clips
                .iter()
                .enumerate()
                .map(|(i, clip)| {
                    let content = format!(
                        " {}. {} ({}) \"{}\"",
                        i + 1,
                        clip.show,
                        &clip.date[..10.min(clip.date.len())],
                        highlight_word(&clip.snippet, word),
                    );
                    ListItem::new(content)
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::ALL))
                .highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("  > ");

            // Footer
            let footer = Paragraph::new(
                " arrows: navigate  enter: select  r: random  q: quit"
            )
            .style(Style::default().fg(Color::DarkGray));

            frame.render_widget(header, chunks[0]);
            frame.render_stateful_widget(list, chunks[1], state);
            frame.render_widget(footer, chunks[2]);
        })?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(ClipChoice::Quit),
                KeyCode::Char('r') => return Ok(ClipChoice::Random),
                KeyCode::Enter => {
                    let idx = state.selected().unwrap_or(0);
                    return Ok(ClipChoice::Selected(clips[idx].clone()));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some(if i == 0 { clips.len() - 1 } else { i - 1 }));
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let i = state.selected().unwrap_or(0);
                    state.select(Some((i + 1) % clips.len()));
                }
                _ => {}
            }
        }
    }
}

/// Highlight the target word in a snippet by uppercasing it.
fn highlight_word(snippet: &str, word: &str) -> String {
    let lower_snippet = snippet.to_lowercase();
    let lower_word = word.to_lowercase();
    if let Some(pos) = lower_snippet.find(&lower_word) {
        let before = &snippet[..pos];
        let matched = &snippet[pos..pos + word.len()];
        let after = &snippet[pos + word.len()..];
        format!("{}{}{}",before, matched.to_uppercase(), after)
    } else {
        snippet.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_word() {
        assert_eq!(
            highlight_word("we must act on this", "act"),
            "we must ACT on this"
        );
    }

    #[test]
    fn test_highlight_word_not_found() {
        assert_eq!(
            highlight_word("hello world", "missing"),
            "hello world"
        );
    }
}
```

**Step 2: Add `mod tui;` to main.rs**

**Step 3: Run tests**

Run: `cargo test tui`
Expected: 2 tests pass

**Step 4: Commit**

```
git add src/tui.rs src/main.rs
git commit -m "feat: add interactive TUI for clip selection"
```

---

### Task 16: Main Orchestration

**Files:**
- Modify: `src/main.rs`

**Step 1: Wire everything together**

Replace the contents of `main()` in `src/main.rs` with the full orchestration logic:

```rust
use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use rand::seq::SliceRandom;

mod align;
mod audio;
mod fallback;
mod fetch;
mod model;
mod process;
mod search;
mod tui;

use audio::AudioBuffer;
use process::ProcessParams;

// ... Cli struct stays the same as Task 2 ...

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    // Handle --download-model
    let model_path = model::resolve_model_path(&cli.model)?;
    if cli.download_model {
        model::download_model(&client, &model_path).await?;
        if cli.phrase.is_empty() {
            return Ok(());
        }
    }

    // Validate model exists
    model::ensure_model_exists(&model_path)?;

    let words: Vec<&str> = cli.phrase.split_whitespace().collect();
    if words.is_empty() {
        anyhow::bail!("Phrase cannot be empty");
    }

    eprintln!("Searching TV archive for {} words...\n", words.len());

    let base_url = "https://api.gdeltproject.org/api/v2/tv/tv";
    let mut word_clips: Vec<AudioBuffer> = Vec::new();

    let pb = ProgressBar::new(words.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:30}] {pos}/{len}")
            .unwrap(),
    );

    for (i, &word) in words.iter().enumerate() {
        pb.set_message(format!("\"{}\"", word));

        // Search for clips
        let clips = search::search_word(&client, word, &cli.station, &cli.exclude, base_url)
            .await
            .unwrap_or_default();

        // Select a clip (TUI or random)
        let selected_clip = if cli.no_tui || clips.is_empty() {
            clips.choose(&mut rand::rng()).cloned()
        } else {
            match tui::select_clip(word, &clips, i, words.len())? {
                tui::ClipChoice::Selected(c) => Some(c),
                tui::ClipChoice::Random => clips.choose(&mut rand::rng()).cloned(),
                tui::ClipChoice::Quit => {
                    eprintln!("\nAborted.");
                    return Ok(());
                }
            }
        };

        let word_audio = if let Some(clip) = selected_clip {
            // Fetch and extract word
            let (start, end) = clip.time_range().unwrap_or((0.0, 30.0));
            match fetch::fetch_audio_segment(&client, &clip.mp3_url(), start, end, 5.0).await {
                Ok(segment) => {
                    match align::align_words(&segment, &model_path) {
                        Ok(aligned) => {
                            match align::extract_word(&segment, &aligned, word, 20.0) {
                                Ok(extracted) => Some(extracted),
                                Err(_) => None,
                            }
                        }
                        Err(_) => None,
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        };

        // Fallback chain if word extraction failed
        let final_audio = match word_audio {
            Some(a) => a,
            None => {
                // Try variations
                let mut found = None;
                for variant in fallback::word_variations(word) {
                    let var_clips = search::search_word(
                        &client, &variant, &cli.station, &cli.exclude, base_url,
                    )
                    .await
                    .unwrap_or_default();

                    if let Some(clip) = var_clips.choose(&mut rand::rng()) {
                        let (start, end) = clip.time_range().unwrap_or((0.0, 30.0));
                        if let Ok(segment) = fetch::fetch_audio_segment(
                            &client, &clip.mp3_url(), start, end, 5.0,
                        ).await {
                            if let Ok(aligned) = align::align_words(&segment, &model_path) {
                                if let Ok(extracted) = align::extract_word(
                                    &segment, &aligned, &variant, 20.0,
                                ) {
                                    found = Some(extracted);
                                    break;
                                }
                            }
                        }
                    }
                }

                match found {
                    Some(a) => a,
                    None => {
                        eprintln!("  Falling back to TTS for '{}'", word);
                        fallback::tts_word(word)?
                    }
                }
            }
        };

        word_clips.push(final_audio);
        pb.inc(1);
    }

    pb.finish_with_message("Done searching");

    // Apply processing pipeline (or raw stitch)
    let output = if cli.raw {
        process::crossfade::stitch(&word_clips, cli.crossfade, cli.gap)
    } else {
        let params = ProcessParams {
            loudness_lufs: cli.loudness,
            pitch_semitones: cli.pitch,
            eq_preset: cli.eq.clone(),
            compress_ratio: cli.compress,
            crossfade_ms: cli.crossfade,
            gap_ms: cli.gap,
            reverb_wet: cli.reverb,
            limit_db: cli.limit,
        };
        process::apply_pipeline(&word_clips, &params)
    };

    // Write output
    output.write_wav(&cli.output)?;
    eprintln!("\nWrote {} ({:.1}s)", cli.output, output.duration());

    Ok(())
}
```

**Step 2: Add `rand` dependency to Cargo.toml**

```toml
rand = "0.9"
```

**Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

**Step 4: Commit**

```
git add src/main.rs Cargo.toml
git commit -m "feat: wire up main orchestration pipeline"
```

---

### Task 17: End-to-End Smoke Test

**Files:**
- (no new files — manual testing)

**Step 1: Run all unit tests**

Run: `cargo test`
Expected: all tests pass

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: no warnings

**Step 3: Fix any issues found**

Address any clippy warnings or test failures.

**Step 4: Test model download**

Run: `cargo run -- --download-model "test"`
Expected: downloads model to `~/.badtv/ggml-base.en.bin` (if not present)

**Step 5: Test with a real phrase (manual)**

Run: `cargo run -- --no-tui "hello world" -o test_output.wav`
Expected: searches GDELT, downloads audio, runs whisper, produces WAV

**Step 6: Commit any fixes**

```
git add -A
git commit -m "fix: address clippy warnings and smoke test issues"
```

---

### Task 18: Polish and Push

**Files:**
- May modify various files for final polish

**Step 1: Run the full test suite one more time**

Run: `cargo test`
Expected: all pass

**Step 2: Push to GitHub**

```
git push origin main
```

**Step 3: Tag initial release**

```
git tag -a v0.1.0 -m "Initial release: core pipeline working"
git push origin v0.1.0
```
