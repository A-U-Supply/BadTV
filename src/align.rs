use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write as IoWrite};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, Once};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::audio::AudioBuffer;

/// Python script that loads whisper once and processes WAV paths from stdin.
/// Reads one file path per line, outputs one JSON line per file.
const WHISPER_SERVER_SCRIPT: &str = r#"
import whisper, json, sys, warnings, os
warnings.filterwarnings("ignore")
model = whisper.load_model("base.en")
print("READY", flush=True)
for line in sys.stdin:
    path = line.strip()
    if not path:
        continue
    try:
        result = model.transcribe(path, word_timestamps=True)
        words = []
        for seg in result.get("segments", []):
            for w in seg.get("words", []):
                words.append({"text": w["word"].strip(), "start": w["start"], "end": w["end"]})
        print(json.dumps(words), flush=True)
    except Exception as e:
        print(json.dumps({"error": str(e)}), flush=True)
"#;

static WHISPER_PROCESS: Mutex<Option<WhisperServer>> = Mutex::new(None);

struct WhisperServer {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}

/// Get or spawn the persistent Python whisper process.
fn get_whisper_server() -> Result<std::sync::MutexGuard<'static, Option<WhisperServer>>> {
    let mut guard = WHISPER_PROCESS
        .lock()
        .map_err(|e| anyhow::anyhow!("Whisper server mutex poisoned: {}", e))?;

    // Check if existing process is still alive.
    if let Some(ref mut server) = *guard {
        match server.child.try_wait() {
            Ok(Some(_)) => {
                // Process exited, need to respawn.
                *guard = None;
            }
            Ok(None) => return Ok(guard), // Still running.
            Err(_) => {
                *guard = None;
            }
        }
    }

    // Spawn new process.
    eprint!(" [loading whisper model...]");
    let mut child = Command::new("python3")
        .args(["-c", WHISPER_SERVER_SCRIPT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn Python whisper server — is openai-whisper installed?")?;

    let stdout = child.stdout.take().context("No stdout from whisper server")?;
    let mut reader = BufReader::new(stdout);

    // Wait for READY signal (model loaded).
    let mut ready_line = String::new();
    reader
        .read_line(&mut ready_line)
        .context("Whisper server died during model load")?;
    if !ready_line.trim().starts_with("READY") {
        anyhow::bail!("Whisper server didn't send READY, got: {}", ready_line.trim());
    }

    *guard = Some(WhisperServer { child, reader });
    Ok(guard)
}

/// Run Python whisper with word_timestamps=True for accurate word alignment.
/// Uses a persistent Python process that loads the model once.
pub fn align_words_python(audio: &AudioBuffer) -> Result<Vec<AlignedWord>> {
    let dir = tempfile::tempdir().context("Failed to create temp dir")?;
    let wav_path = dir.path().join("segment.wav");
    let wav_str = wav_path.to_str().context("Non-UTF-8 temp path")?;
    audio.write_wav(wav_str)?;

    let mut guard = get_whisper_server()?;
    let server = guard.as_mut().context("Whisper server not initialized")?;

    // Send file path to the persistent process.
    let stdin = server
        .child
        .stdin
        .as_mut()
        .context("Whisper server stdin closed")?;
    writeln!(stdin, "{}", wav_str).context("Failed to write to whisper server")?;

    // Read one JSON line of results.
    let mut line = String::new();
    server
        .reader
        .read_line(&mut line)
        .context("Failed to read from whisper server")?;

    // Check for error response.
    if let Ok(err_obj) = serde_json::from_str::<serde_json::Value>(line.trim()) {
        if let Some(err_msg) = err_obj.get("error").and_then(|e| e.as_str()) {
            anyhow::bail!("Python whisper error: {}", err_msg);
        }
    }

    let parsed: Vec<serde_json::Value> =
        serde_json::from_str(line.trim()).context("Failed to parse whisper output")?;

    // Whisper DTW timestamps are systematically ~100ms early.
    // Shift right to compensate.
    let offset = 0.10_f32;

    let words: Vec<AlignedWord> = parsed
        .into_iter()
        .filter_map(|v| {
            let text = v.get("text")?.as_str()?.to_string();
            let start = v.get("start")?.as_f64()? as f32 + offset;
            let end = v.get("end")?.as_f64()? as f32 + offset;
            if text.trim().is_empty() || text.starts_with('[') {
                return None;
            }
            Some(AlignedWord {
                text,
                start_secs: start,
                end_secs: end,
            })
        })
        .collect();

    Ok(words)
}

static SUPPRESS_WHISPER_LOG: Once = Once::new();

/// No-op log callback to suppress all whisper.cpp output.
unsafe extern "C" fn whisper_log_noop(
    _level: whisper_rs_sys::ggml_log_level,
    _text: *const std::ffi::c_char,
    _user_data: *mut std::ffi::c_void,
) {
}

/// Suppress all whisper.cpp log output (model loading, backend info, etc.)
fn suppress_whisper_logs() {
    SUPPRESS_WHISPER_LOG.call_once(|| unsafe {
        whisper_rs_sys::whisper_log_set(Some(whisper_log_noop), std::ptr::null_mut());
    });
}

/// A single word with its time boundaries from whisper alignment.
#[derive(Debug, Clone)]
pub struct AlignedWord {
    pub text: String,
    pub start_secs: f32,
    pub end_secs: f32,
}

/// Load a whisper context from a model file. Call once and reuse.
pub fn load_whisper_context(model_path: &Path) -> Result<WhisperContext> {
    suppress_whisper_logs();
    WhisperContext::new_with_params(
        model_path.to_str().context("Invalid model path")?,
        WhisperContextParameters::default(),
    )
    .context("Failed to load whisper model")
}

/// Find the best match for `target_word` in aligned words and extract audio.
///
/// Uses ranked matching: exact > starts-with > contains > edit distance 1.
pub fn extract_word(
    audio: &AudioBuffer,
    words: &[AlignedWord],
    target_word: &str,
    padding_ms: f32,
) -> Result<AudioBuffer> {
    let target = target_word.to_lowercase();

    let duration = audio.duration();

    // Collect all matches with scores.
    // Light edge filter: skip words in the first/last 0.3s where whisper
    // timestamps are least reliable (scaled for short clips).
    let edge_margin = (duration * 0.05).clamp(0.2, 0.5);
    let matches: Vec<(i32, &AlignedWord)> = words
        .iter()
        .filter_map(|w| {
            if w.start_secs < edge_margin || w.end_secs > duration - edge_margin {
                return None;
            }
            // Skip words with implausible durations (>1.5s for a single word).
            let word_dur = w.end_secs - w.start_secs;
            if !(0.01..=1.5).contains(&word_dur) {
                return None;
            }

            let normalized = w
                .text
                .to_lowercase()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_string();
            if normalized.is_empty() {
                return None;
            }
            // For short words (≤3 chars), only accept exact matches —
            // edit distance 1 is too loose (33%+ of the word).
            let score = if normalized == target {
                4 // exact
            } else if target.len() <= 3 {
                return None;
            } else if edit_distance_1(&normalized, &target) {
                3 // close (typo/mishearing)
            } else if sounds_alike(&normalized, &target) {
                2 // homophone
            } else {
                return None;
            };
            Some((score, w))
        })
        .collect();

    // Pick the first match with the highest score.
    let best = matches
        .iter()
        .max_by_key(|(score, _)| *score)
        .and_then(|(best_score, _)| {
            // Return the FIRST match at the best score level.
            matches.iter().find(|(s, _)| s == best_score)
        });

    match best {
        Some((score, word)) => {
            // Find this word's index in the original list to check neighbors.
            let idx = words.iter().position(|w| std::ptr::eq(w, *word));

            // Use midpoints between this word and its neighbors as boundaries.
            // This cuts right in the inter-word gap, avoiding adjacent words.
            let start = match idx.and_then(|i| if i > 0 { Some(&words[i - 1]) } else { None }) {
                Some(prev) => {
                    // Cut at midpoint between previous word's end and this word's start.
                    let mid = (prev.end_secs + word.start_secs) / 2.0;
                    mid.max(word.start_secs - padding_ms / 1000.0)
                }
                None => (word.start_secs - padding_ms / 1000.0).max(0.0),
            };

            let end = match idx.and_then(|i| words.get(i + 1)) {
                Some(next) => {
                    // Cut at midpoint between this word's end and next word's start.
                    let mid = (word.end_secs + next.start_secs) / 2.0;
                    mid.min(word.end_secs + padding_ms / 1000.0)
                }
                None => (word.end_secs + padding_ms / 1000.0).min(duration),
            };

            let _ = score; // used in match above
            Ok(audio.slice(start, end))
        }
        None => {
            let heard: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
            anyhow::bail!(
                "Word '{}' not found in whisper output: [{}]",
                target_word,
                heard.join(", ")
            );
        }
    }
}

/// Check if two strings differ by at most one edit (insert, delete, or substitute).
fn edit_distance_1(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (la, lb) = (a.len(), b.len());

    if la.abs_diff(lb) > 1 {
        return false;
    }

    let mut diffs = 0;
    if la == lb {
        for i in 0..la {
            if a[i] != b[i] {
                diffs += 1;
                if diffs > 1 {
                    return false;
                }
            }
        }
        diffs == 1
    } else {
        let (longer, shorter) = if la > lb { (&a, &b) } else { (&b, &a) };
        let mut i = 0;
        let mut j = 0;
        while i < longer.len() && j < shorter.len() {
            if longer[i] != shorter[j] {
                diffs += 1;
                if diffs > 1 {
                    return false;
                }
            } else {
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// Transcribe an audio clip and return the full text (lowercased, trimmed).
pub fn transcribe(audio: &AudioBuffer, ctx: &WhisperContext) -> Result<String> {
    let samples_16k = resample_to_16k(audio)?;
    let mut state = ctx.create_state().context("Failed to create whisper state")?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);

    state
        .full(params, &samples_16k)
        .context("Whisper transcription failed")?;

    let mut text = String::new();
    for seg_idx in 0..state.full_n_segments() {
        if let Some(seg) = state.get_segment(seg_idx) {
            let num_tokens = seg.n_tokens();
            for tok_idx in 0..num_tokens {
                if let Some(tok) = seg.get_token(tok_idx) {
                    if let Ok(t) = tok.to_str() {
                        let t = t.trim();
                        if !t.is_empty() && !t.starts_with('[') {
                            text.push_str(t);
                            text.push(' ');
                        }
                    }
                }
            }
        }
    }
    Ok(text.trim().to_lowercase())
}

/// Check for common English homophones/near-homophones that whisper confuses.
fn sounds_alike(a: &str, b: &str) -> bool {
    let mut a = a.to_string();
    let mut b = b.to_string();
    // Normalize common whisper confusions
    for s in [&mut a, &mut b] {
        // buy/bye/by, war/wore, etc.
        *s = s
            .replace("bye", "buy")
            .replace("bi", "buy")
            .replace("by", "buy");
    }
    a == b || edit_distance_1(&a, &b)
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
        // 10s buffer so words at 3-4s pass the edge filter (>2s from start, >1s from end).
        let audio = AudioBuffer::new(vec![0.5; 441000], 44100);
        let words = vec![
            AlignedWord {
                text: "hello".into(),
                start_secs: 3.0,
                end_secs: 3.3,
            },
            AlignedWord {
                text: "world".into(),
                start_secs: 4.0,
                end_secs: 4.3,
            },
        ];
        let result = extract_word(&audio, &words, "world", 20.0).unwrap();
        assert!(result.duration() > 0.3);
        assert!(result.duration() < 0.4);
    }

    #[test]
    fn test_extract_word_case_insensitive() {
        let audio = AudioBuffer::new(vec![0.5; 441000], 44100);
        let words = vec![AlignedWord {
            text: "Hello".into(),
            start_secs: 3.0,
            end_secs: 3.3,
        }];
        assert!(extract_word(&audio, &words, "hello", 20.0).is_ok());
    }

    #[test]
    fn test_extract_word_not_found() {
        let audio = AudioBuffer::new(vec![0.5; 441000], 44100);
        let words = vec![AlignedWord {
            text: "hello".into(),
            start_secs: 3.0,
            end_secs: 3.3,
        }];
        assert!(extract_word(&audio, &words, "goodbye", 20.0).is_err());
    }
}
