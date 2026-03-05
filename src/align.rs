use anyhow::{Context, Result};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::AudioBuffer;

/// A single word with its time boundaries from whisper alignment.
#[derive(Debug, Clone)]
pub struct AlignedWord {
    pub text: String,
    pub start_secs: f32,
    pub end_secs: f32,
}

/// Run whisper on an AudioBuffer and return word-level timestamps.
pub fn align_words(audio: &AudioBuffer, model_path: &Path) -> Result<Vec<AlignedWord>> {
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

    let num_segments = state.full_n_segments();
    let mut words = Vec::new();

    for seg_idx in 0..num_segments {
        let segment = match state.get_segment(seg_idx) {
            Some(seg) => seg,
            None => continue,
        };

        let num_tokens = segment.n_tokens();
        for tok_idx in 0..num_tokens {
            let token = match segment.get_token(tok_idx) {
                Some(tok) => tok,
                None => continue,
            };

            let token_text = match token.to_str() {
                Ok(t) => t,
                Err(_) => continue,
            };

            let text = token_text.trim().to_string();
            if text.is_empty() || text.starts_with('[') {
                continue;
            }

            let token_data = token.token_data();
            let start = token_data.t0 as f32 / 100.0;
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

/// Find the best match for `target_word` in aligned words and extract audio.
pub fn extract_word(
    audio: &AudioBuffer,
    words: &[AlignedWord],
    target_word: &str,
    padding_ms: f32,
) -> Result<AudioBuffer> {
    let target_lower = target_word.to_lowercase();

    let best = words
        .iter()
        .find(|w| {
            w.text
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                == target_lower
        })
        .context(format!(
            "Word '{}' not found in whisper output",
            target_word
        ))?;

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

    let chunk_size = 1024;
    let mut resampler = FftFixedIn::<f32>::new(
        audio.sample_rate as usize,
        16000,
        chunk_size,
        1,
        1,
    )
    .context("Failed to create resampler")?;

    let mut output = Vec::new();

    for chunk in audio.samples.chunks(chunk_size) {
        let mut input_chunk = chunk.to_vec();
        if input_chunk.len() < chunk_size {
            input_chunk.resize(chunk_size, 0.0);
        }
        let input = vec![input_chunk];
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
        let audio = AudioBuffer::new(vec![0.5; 44100], 44100);
        let words = vec![
            AlignedWord {
                text: "hello".into(),
                start_secs: 0.1,
                end_secs: 0.4,
            },
            AlignedWord {
                text: "world".into(),
                start_secs: 0.5,
                end_secs: 0.8,
            },
        ];
        let result = extract_word(&audio, &words, "world", 20.0).unwrap();
        assert!(result.duration() > 0.3);
        assert!(result.duration() < 0.4);
    }

    #[test]
    fn test_extract_word_case_insensitive() {
        let audio = AudioBuffer::new(vec![0.5; 44100], 44100);
        let words = vec![AlignedWord {
            text: "Hello".into(),
            start_secs: 0.1,
            end_secs: 0.4,
        }];
        assert!(extract_word(&audio, &words, "hello", 20.0).is_ok());
    }

    #[test]
    fn test_extract_word_not_found() {
        let audio = AudioBuffer::new(vec![0.5; 44100], 44100);
        let words = vec![AlignedWord {
            text: "hello".into(),
            start_secs: 0.1,
            end_secs: 0.4,
        }];
        assert!(extract_word(&audio, &words, "goodbye", 20.0).is_err());
    }
}
