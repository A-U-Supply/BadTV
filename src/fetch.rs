use anyhow::{Context, Result};
use std::process::Command;

use crate::audio::AudioBuffer;

/// Fetch audio from an MP3 URL at a specific time range using ffmpeg.
///
/// ffmpeg handles HTTP range requests and MP3 seeking internally,
/// so this works efficiently even for timestamps deep into large files.
pub async fn fetch_audio_segment(
    _client: &reqwest::Client,
    mp3_url: &str,
    start_secs: f64,
    end_secs: f64,
    padding_secs: f64,
) -> Result<AudioBuffer> {
    let padded_start = (start_secs - padding_secs).max(0.0);
    let duration = (end_secs + padding_secs) - padded_start;

    let dir = tempfile::tempdir().context("Failed to create temp dir")?;
    let wav_path = dir.path().join("clip.wav");
    let wav_str = wav_path.to_str().context("Non-UTF-8 temp path")?;

    let output = Command::new("ffmpeg")
        .args([
            "-timeout", "10000000",  // 10s network timeout (in microseconds)
            "-ss", &format!("{:.3}", padded_start),
            "-t", &format!("{:.3}", duration),
            "-i", mp3_url,
            "-f", "wav",
            "-ac", "1",
            "-ar", "44100",
            "-y",
            wav_str,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("Failed to run ffmpeg — is it installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check for common failures.
        if stderr.contains("Server returned 404")
            || stderr.contains("HTTP error 404")
            || stderr.contains("does not exist")
        {
            anyhow::bail!("MP3 not found (404)");
        }
        anyhow::bail!("ffmpeg failed: {}", stderr.lines().last().unwrap_or("unknown error"));
    }

    AudioBuffer::read_wav(wav_str)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_ffmpeg_available() {
        let status = std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        assert!(status.is_ok(), "ffmpeg must be installed");
    }
}
