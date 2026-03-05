use anyhow::{Context, Result};

use crate::audio::AudioBuffer;

/// Generate word variations to try when exact match fails.
pub fn word_variations(word: &str) -> Vec<String> {
    let mut variations = vec![word.to_string()];
    let lower = word.to_lowercase();

    if lower.ends_with('s') {
        variations.push(lower[..lower.len() - 1].to_string());
    } else {
        variations.push(format!("{}s", lower));
    }

    if lower.ends_with("ing") {
        let stem = &lower[..lower.len() - 3];
        variations.push(stem.to_string());
        variations.push(format!("{}s", stem));
        // Handle doubled consonant: "running" -> "runn" -> also try "run"
        let stem_bytes = stem.as_bytes();
        if stem_bytes.len() >= 2 && stem_bytes[stem_bytes.len() - 1] == stem_bytes[stem_bytes.len() - 2] {
            let short_stem = &stem[..stem.len() - 1];
            variations.push(short_stem.to_string());
            variations.push(format!("{}s", short_stem));
        }
    } else if lower.ends_with("ed") {
        let stem = &lower[..lower.len() - 2];
        variations.push(stem.to_string());
        variations.push(format!("{}ing", stem));
        // Handle doubled consonant: "stopped" -> "stopp" -> also try "stop"
        let stem_bytes = stem.as_bytes();
        if stem_bytes.len() >= 2 && stem_bytes[stem_bytes.len() - 1] == stem_bytes[stem_bytes.len() - 2] {
            let short_stem = &stem[..stem.len() - 1];
            variations.push(short_stem.to_string());
            variations.push(format!("{}ing", short_stem));
        }
    } else {
        variations.push(format!("{}ing", lower));
        variations.push(format!("{}ed", lower));
    }

    variations.sort();
    variations.dedup();
    variations
}

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
        .status()
        .context("TTS failed — 'say' command not found (macOS only for now)")?;

    if !status.success() {
        anyhow::bail!("TTS 'say' command failed");
    }

    let status = std::process::Command::new("afconvert")
        .args(["-f", "WAVE", "-d", "LEI32", aiff_str, wav_str])
        .status()
        .context("afconvert failed")?;

    if !status.success() {
        anyhow::bail!("afconvert failed to convert AIFF to WAV");
    }

    AudioBuffer::read_wav(wav_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variations_basic() {
        let v = word_variations("buy");
        assert!(v.contains(&"buy".to_string()));
        assert!(v.contains(&"buys".to_string()));
        assert!(v.contains(&"buying".to_string()));
    }

    #[test]
    fn test_variations_plural() {
        let v = word_variations("cats");
        assert!(v.contains(&"cats".to_string()));
        assert!(v.contains(&"cat".to_string()));
    }

    #[test]
    fn test_variations_ing() {
        let v = word_variations("running");
        assert!(v.contains(&"running".to_string()));
        assert!(v.contains(&"run".to_string()));
        assert!(v.contains(&"runs".to_string()));
    }

    #[test]
    fn test_variations_ed() {
        let v = word_variations("jumped");
        assert!(v.contains(&"jumped".to_string()));
        assert!(v.contains(&"jump".to_string()));
        assert!(v.contains(&"jumping".to_string()));
    }
}
