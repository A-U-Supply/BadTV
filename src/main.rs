use anyhow::Result;
use clap::{Parser, Subcommand};

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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Stitch a phrase from TV news clips
    Stitch(StitchArgs),
    /// Download multiple isolated samples of a single word
    Sample(SampleArgs),
}

/// Shared options for model and station filtering.
#[derive(Parser, Debug, Clone)]
struct SharedArgs {
    /// Filter to specific stations (repeatable)
    #[arg(long)]
    station: Vec<String>,

    /// Exclude specific stations (repeatable)
    #[arg(long)]
    exclude: Vec<String>,

    /// Path to whisper model file
    #[arg(long, default_value = "~/.badtv/ggml-base.en.bin")]
    model: String,

    /// Download the whisper model if not present
    #[arg(long)]
    download_model: bool,
}

#[derive(Parser, Debug)]
struct StitchArgs {
    /// The phrase to construct from TV clips
    phrase: String,

    /// Output file path
    #[arg(short, long, default_value = "badtv_output.wav")]
    output: String,

    #[command(flatten)]
    shared: SharedArgs,

    /// Interactively select clips (default: pick randomly)
    #[arg(long, short)]
    interactive: bool,

    /// Pitch shift in semitones
    #[arg(long, default_value = "0.0")]
    pitch: f32,

    /// Target loudness in LUFS
    #[arg(long, default_value = "-16.0")]
    loudness: f32,

    /// Crossfade duration in milliseconds
    #[arg(long, default_value = "30")]
    crossfade: u32,

    /// Gap between words in milliseconds
    #[arg(long, default_value = "120")]
    gap: u32,

    /// Reverb wet mix (0-100)
    #[arg(long, default_value = "0")]
    reverb: u32,

    /// Compressor ratio
    #[arg(long, default_value = "2.0")]
    compress: f32,

    /// EQ preset: tv, bright, flat, off
    #[arg(long, default_value = "flat")]
    eq: String,

    /// Limiter ceiling in dB
    #[arg(long, default_value = "-1.0")]
    limit: f32,

    /// Padding around each extracted word in milliseconds
    #[arg(long, default_value = "0")]
    padding: f32,

    /// Skip all audio processing, just stitch
    #[arg(long)]
    raw: bool,

    /// Transcribe final output with whisper to verify intelligibility
    #[arg(long)]
    verify: bool,
}

#[derive(Parser, Debug)]
struct SampleArgs {
    /// The word to download samples of
    word: String,

    /// Number of samples to collect
    #[arg(short = 'n', long, default_value = "5")]
    count: usize,

    /// Output directory (default: ./{word}/)
    #[arg(short, long)]
    output_dir: Option<String>,

    /// Also create a stitched compilation of all samples
    #[arg(long)]
    stitch: bool,

    /// Padding around each extracted word in milliseconds
    #[arg(long, default_value = "0")]
    padding: f32,

    /// Gap between clips in stitched output (milliseconds)
    #[arg(long, default_value = "200")]
    gap: u32,

    #[command(flatten)]
    shared: SharedArgs,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Stitch(args) => run_stitch(args).await,
        Commands::Sample(args) => run_sample(args).await,
    }
}

async fn run_stitch(cli: StitchArgs) -> Result<()> {
    let client = reqwest::Client::new();

    let model_path = model::resolve_model_path(&cli.shared.model)?;
    if cli.shared.download_model {
        model::download_model(&client, &model_path).await?;
    }
    model::ensure_model_exists(&model_path)?;

    let whisper_ctx = align::load_whisper_context(&model_path)?;

    let words: Vec<String> = cli
        .phrase
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        anyhow::bail!("Phrase cannot be empty");
    }

    let base_url = "https://archive.org/services/search/beta/page_production/";
    let mut word_clips: Vec<audio::AudioBuffer> = Vec::new();

    let mut search_cache: std::collections::HashMap<String, Vec<search::Clip>> =
        std::collections::HashMap::new();
    let mut used_identifiers: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for (i, word) in words.iter().enumerate() {
        eprint!("[{}/{}] \"{}\"", i + 1, words.len(), word);

        let clips = if let Some(cached) = search_cache.get(word) {
            cached.clone()
        } else {
            let results = search::search_word(
                &client,
                word,
                &cli.shared.station,
                &cli.shared.exclude,
                base_url,
            )
            .await
            .unwrap_or_default();
            let available = search::filter_available_clips(&client, &results).await;
            eprint!(" ({}/{})", available.len(), results.len());
            search_cache.insert(word.clone(), available.clone());
            available
        };

        let selected_clip = if cli.interactive && !clips.is_empty() {
            match tui::select_clip(word, &clips, i, words.len())? {
                tui::ClipChoice::Quit => {
                    eprintln!("\nAborted.");
                    return Ok(());
                }
                tui::ClipChoice::Selected(clip) => Some(clip),
                tui::ClipChoice::Random => None,
            }
        } else {
            None
        };

        let sorted_clips = if let Some(ref picked) = selected_clip {
            let mut v = vec![picked.clone()];
            v.extend(
                clips
                    .iter()
                    .filter(|c| c.identifier != picked.identifier)
                    .cloned(),
            );
            v
        } else {
            let mut v = clips.clone();
            v.sort_by(|a, b| {
                let a_used = used_identifiers.contains(&a.identifier) as u8;
                let b_used = used_identifiers.contains(&b.identifier) as u8;
                a_used.cmp(&b_used).then(
                    a.start_secs
                        .partial_cmp(&b.start_secs)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
            });
            v
        };

        let mut verified = None;
        'clip_loop: for clip in &sorted_clips {
            let caption_hits = match search::find_word_in_srt(&client, clip, word).await {
                Ok(hits) if !hits.is_empty() => {
                    eprint!(" [{}:{}hits]", clip.station, hits.len());
                    hits
                }
                _ => continue,
            };

            for hit in &caption_hits {
                let segment = match fetch::fetch_audio_segment(
                    &client,
                    &clip.mp3_url(),
                    hit.start_secs,
                    hit.end_secs,
                    10.0,
                )
                .await
                {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let aligned = match align::align_words_python(&segment) {
                    Ok(a) => a,
                    Err(_) => continue,
                };

                let extracted = match align::extract_word(&segment, &aligned, word, cli.padding) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                used_identifiers.insert(clip.identifier.clone());
                verified = Some(extracted);
                break 'clip_loop;
            }
        }

        let final_audio = match verified {
            Some(a) => {
                eprintln!(" ok [{:.2}s]", a.duration());
                a
            }
            None => {
                eprintln!(" tts");
                fallback::tts_word(word)?
            }
        };

        word_clips.push(final_audio);
    }

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

    if cli.verify {
        match align::transcribe(&output, &whisper_ctx) {
            Ok(heard) => eprintln!("Whisper heard: \"{}\"", heard),
            Err(e) => eprintln!("Verification failed: {}", e),
        }
    }

    Ok(())
}

async fn run_sample(args: SampleArgs) -> Result<()> {
    let client = reqwest::Client::new();

    let model_path = model::resolve_model_path(&args.shared.model)?;
    if args.shared.download_model {
        model::download_model(&client, &model_path).await?;
    }
    model::ensure_model_exists(&model_path)?;

    let word = args
        .word
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if word.is_empty() {
        anyhow::bail!("Word cannot be empty");
    }

    let out_dir = args.output_dir.unwrap_or_else(|| word.clone());
    std::fs::create_dir_all(&out_dir)?;

    eprintln!(
        "Sampling \"{}\" (target: {} clips) -> {}/",
        word, args.count, out_dir
    );

    let base_url = "https://archive.org/services/search/beta/page_production/";
    let results = search::search_word(
        &client,
        &word,
        &args.shared.station,
        &args.shared.exclude,
        base_url,
    )
    .await?;
    let clips = search::filter_available_clips(&client, &results).await;
    eprintln!(
        "Found {} available clips (of {} search results)",
        clips.len(),
        results.len()
    );

    if clips.is_empty() {
        anyhow::bail!("No clips found for \"{}\"", word);
    }

    let mut collected = 0usize;
    let mut samples: Vec<audio::AudioBuffer> = Vec::new();

    for clip in &clips {
        if collected >= args.count {
            break;
        }

        eprint!("  [{}] {}", clip.station, clip.identifier);

        let caption_hits = match search::find_word_in_srt(&client, clip, &word).await {
            Ok(hits) if !hits.is_empty() => {
                eprint!(" ({}hits)", hits.len());
                hits
            }
            _ => {
                eprintln!(" skip (no SRT match)");
                continue;
            }
        };

        for hit in &caption_hits {
            if collected >= args.count {
                break;
            }

            let segment = match fetch::fetch_audio_segment(
                &client,
                &clip.mp3_url(),
                hit.start_secs,
                hit.end_secs,
                10.0,
            )
            .await
            {
                Ok(s) => s,
                Err(_) => continue,
            };

            let aligned = match align::align_words_python(&segment) {
                Ok(a) => a,
                Err(_) => continue,
            };

            let extracted = match align::extract_word(&segment, &aligned, &word, args.padding) {
                Ok(e) => e,
                Err(_) => continue,
            };

            let filename = format!("{}/{}-{}.wav", out_dir, word, clip.identifier);
            extracted.write_wav(&filename)?;
            collected += 1;
            eprintln!(" ok [{:.2}s] -> {}", extracted.duration(), filename);
            samples.push(extracted);
            break; // one sample per clip for variety
        }
    }

    eprintln!(
        "\nCollected {}/{} samples in {}/",
        collected, args.count, out_dir
    );

    if collected == 0 {
        anyhow::bail!(
            "Failed to extract any samples for \"{}\" from {} clips",
            word,
            clips.len()
        );
    }

    if args.stitch && samples.len() > 1 {
        let stitched = process::crossfade::stitch(&samples, 0, args.gap);
        let stitch_path = format!("{}/{}-stitched.wav", out_dir, word);
        stitched.write_wav(&stitch_path)?;
        eprintln!("Stitched {} clips -> {} ({:.1}s)", samples.len(), stitch_path, stitched.duration());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_stitch_defaults() {
        let cli = Cli::parse_from(["badtv", "stitch", "hello world"]);
        match cli.command {
            Commands::Stitch(args) => {
                assert_eq!(args.phrase, "hello world");
                assert_eq!(args.output, "badtv_output.wav");
                assert_eq!(args.pitch, 0.0);
                assert_eq!(args.loudness, -16.0);
                assert_eq!(args.crossfade, 30);
                assert_eq!(args.gap, 120);
                assert_eq!(args.reverb, 0);
                assert_eq!(args.compress, 2.0);
                assert_eq!(args.eq, "flat");
                assert_eq!(args.limit, -1.0);
                assert!(!args.raw);
                assert!(!args.interactive);
            }
            _ => panic!("Expected Stitch command"),
        }
    }

    #[test]
    fn test_stitch_custom_flags() {
        let cli = Cli::parse_from([
            "badtv", "stitch", "buy now", "-o", "out.wav", "--station", "CNN", "--station",
            "MSNBC", "--pitch", "3.5", "--raw", "--interactive",
        ]);
        match cli.command {
            Commands::Stitch(args) => {
                assert_eq!(args.phrase, "buy now");
                assert_eq!(args.output, "out.wav");
                assert_eq!(args.shared.station, vec!["CNN", "MSNBC"]);
                assert_eq!(args.pitch, 3.5);
                assert!(args.raw);
                assert!(args.interactive);
            }
            _ => panic!("Expected Stitch command"),
        }
    }

    #[test]
    fn test_sample_defaults() {
        let cli = Cli::parse_from(["badtv", "sample", "buy"]);
        match cli.command {
            Commands::Sample(args) => {
                assert_eq!(args.word, "buy");
                assert_eq!(args.count, 5);
                assert!(args.output_dir.is_none());
                assert!(!args.stitch);
                assert_eq!(args.gap, 200);
                assert!(args.shared.station.is_empty());
            }
            _ => panic!("Expected Sample command"),
        }
    }

    #[test]
    fn test_sample_custom_flags() {
        let cli = Cli::parse_from([
            "badtv", "sample", "buy", "-n", "10", "-o", "samples", "--station", "CNN",
            "--stitch", "--gap", "300",
        ]);
        match cli.command {
            Commands::Sample(args) => {
                assert_eq!(args.word, "buy");
                assert_eq!(args.count, 10);
                assert_eq!(args.output_dir, Some("samples".to_string()));
                assert_eq!(args.shared.station, vec!["CNN"]);
                assert!(args.stitch);
                assert_eq!(args.gap, 300);
            }
            _ => panic!("Expected Sample command"),
        }
    }
}
