use anyhow::Result;
use clap::Parser;

#[allow(dead_code)]
mod align;
#[allow(dead_code)]
mod audio;
#[allow(dead_code)]
mod fallback;
#[allow(dead_code)]
mod fetch;
#[allow(dead_code)]
mod model;
#[allow(dead_code)]
mod process;
#[allow(dead_code)]
mod search;
#[allow(dead_code)]
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

fn main() -> Result<()> {
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
