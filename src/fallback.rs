use anyhow::{Context, Result};

use crate::audio::AudioBuffer;

/// Generate speech for a word using system TTS (macOS `say`).
pub fn tts_word(word: &str) -> Result<AudioBuffer> {
    let dir = tempfile::tempdir().context("Failed to create temp dir for TTS")?;
    let aiff_path = dir.path().join("tts.aiff");
    let wav_path = dir.path().join("tts.wav");

    let aiff_str = aiff_path
        .to_str()
        .context("Non-UTF-8 temp path for AIFF")?;
    let wav_str = wav_path.to_str().context("Non-UTF-8 temp path for WAV")?;

    let status = std::process::Command::new("say")
        .args(["-o", aiff_str, word])
        .stderr(std::process::Stdio::null())
        .status()
        .context("TTS failed — 'say' command not found (macOS only for now)")?;

    if !status.success() {
        anyhow::bail!("TTS 'say' command failed");
    }

    let status = std::process::Command::new("afconvert")
        .args(["-f", "WAVE", "-d", "LEI32", aiff_str, wav_str])
        .stderr(std::process::Stdio::null())
        .status()
        .context("afconvert failed")?;

    if !status.success() {
        anyhow::bail!("afconvert failed to convert AIFF to WAV");
    }

    AudioBuffer::read_wav(wav_str)
}

