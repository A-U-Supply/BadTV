use anyhow::Result;
use clap::Parser;

mod align;
mod audio;
mod fallback;
mod fetch;
mod model;
mod process;
mod search;
mod tui;

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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    // Handle --download-model
    let model_path = model::resolve_model_path(&cli.model)?;
    if cli.download_model {
        model::download_model(&client, &model_path).await?;
    }

    // Validate model exists
    model::ensure_model_exists(&model_path)?;

    let words: Vec<&str> = cli.phrase.split_whitespace().collect();
    if words.is_empty() {
        anyhow::bail!("Phrase cannot be empty");
    }

    eprintln!("Searching TV archive for {} words...\n", words.len());

    let base_url = "https://api.gdeltproject.org/api/v2/tv/tv";
    let mut word_clips: Vec<audio::AudioBuffer> = Vec::new();

    for (i, &word) in words.iter().enumerate() {
        eprintln!("  Searching for \"{}\"...", word);

        // Search for clips
        let clips = search::search_word(&client, word, &cli.station, &cli.exclude, base_url)
            .await
            .unwrap_or_default();

        // Select a clip (TUI or random)
        let selected_clip = if cli.no_tui || clips.is_empty() {
            use rand::seq::IndexedRandom;
            clips.choose(&mut rand::rng()).cloned()
        } else {
            match tui::select_clip(word, &clips, i, words.len())? {
                tui::ClipChoice::Selected(c) => Some(c),
                tui::ClipChoice::Random => {
                    use rand::seq::IndexedRandom;
                    clips.choose(&mut rand::rng()).cloned()
                }
                tui::ClipChoice::Quit => {
                    eprintln!("\nAborted.");
                    return Ok(());
                }
            }
        };

        let word_audio = if let Some(clip) = selected_clip {
            let (start, end) = clip.time_range().unwrap_or((0.0_f64, 30.0_f64));
            eprintln!("  Fetching audio from {}...", clip.show);
            match fetch::fetch_audio_segment(&client, &clip.mp3_url(), start, end, 5.0).await {
                Ok(segment) => {
                    eprintln!("  Running whisper alignment...");
                    match align::align_words(&segment, &model_path) {
                        Ok(aligned) => {
                            match align::extract_word(&segment, &aligned, word, 20.0) {
                                Ok(extracted) => {
                                    eprintln!(
                                        "  Extracted \"{}\" [{:.2}s]",
                                        word,
                                        extracted.duration()
                                    );
                                    Some(extracted)
                                }
                                Err(e) => {
                                    eprintln!("  Word extraction failed: {}", e);
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("  Whisper alignment failed: {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  Audio fetch failed: {}", e);
                    None
                }
            }
        } else {
            eprintln!("  No clips found for \"{}\"", word);
            None
        };

        // Fallback chain
        let final_audio = match word_audio {
            Some(a) => a,
            None => {
                eprintln!("  Trying variations for \"{}\"...", word);
                let mut found = None;
                for variant in fallback::word_variations(word) {
                    if variant == word {
                        continue; // already tried
                    }
                    let var_clips = search::search_word(
                        &client, &variant, &cli.station, &cli.exclude, base_url,
                    )
                    .await
                    .unwrap_or_default();

                    if let Some(clip) = {
                        use rand::seq::IndexedRandom;
                        var_clips.choose(&mut rand::rng()).cloned()
                    } {
                        let (start, end) = clip.time_range().unwrap_or((0.0_f64, 30.0_f64));
                        if let Ok(segment) = fetch::fetch_audio_segment(
                            &client,
                            &clip.mp3_url(),
                            start,
                            end,
                            5.0,
                        )
                        .await
                        {
                            if let Ok(aligned) = align::align_words(&segment, &model_path) {
                                if let Ok(extracted) =
                                    align::extract_word(&segment, &aligned, &variant, 20.0)
                                {
                                    eprintln!("  Found via variation \"{}\"", variant);
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
                        eprintln!("  Falling back to TTS for \"{}\"", word);
                        fallback::tts_word(word)?
                    }
                }
            }
        };

        word_clips.push(final_audio);
    }

    eprintln!("\nAssembling output...");

    // Apply processing pipeline (or raw stitch)
    let output = if cli.raw {
        process::crossfade::stitch(&word_clips, cli.crossfade, cli.gap)
    } else {
        let params = process::ProcessParams {
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

    output.write_wav(&cli.output)?;
    eprintln!("Wrote {} ({:.1}s)", cli.output, output.duration());

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
