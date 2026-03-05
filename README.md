# BadTV

Stitch TV news clips into infomercial-style audio collages.

BadTV takes a phrase, searches the Internet Archive's TV News Archive for each word,
extracts the exact audio of that word using Whisper-based alignment, and stitches them
together with infomercial-style audio processing — pitch normalization, compression, EQ,
and reverb.

The result: a punchy, chaotic audio collage where every word comes from a different TV
news broadcast, blended to sound like a fake infomercial.

## Quick Start

```sh
# Install
cargo install badtv

# Download the Whisper model (first time only, ~142MB)
badtv --download-model

# Generate an infomercial
badtv "buy now and save"
```

## Usage

```
badtv [OPTIONS] <PHRASE>

Arguments:
  <PHRASE>              The phrase to construct from TV clips

Options:
  -o, --output <PATH>   Output file [default: badtv_output.wav]
  --station <STATION>   Filter to specific stations (repeatable)
  --exclude <STATION>   Exclude stations (repeatable)
  --no-tui              Skip interactive selection, pick randomly
  --model <PATH>        Whisper model path [default: ~/.badtv/ggml-base.en.bin]
  --download-model      Download the whisper model

Audio processing:
  --pitch <SEMITONES>   Pitch shift [default: 2]
  --loudness <LUFS>     Target loudness [default: -16]
  --crossfade <MS>      Crossfade duration [default: 30]
  --gap <MS>            Gap between words [default: 50]
  --reverb <0-100>      Reverb wet mix [default: 15]
  --compress <RATIO>    Compressor ratio [default: 4.0]
  --eq <PRESET>         EQ preset: tv, bright, flat, off [default: tv]
  --limit <DB>          Limiter ceiling [default: -1.0]
  --raw                 Skip all audio processing
```

## How It Works

1. **Search** — Queries the GDELT TV API to find TV news clips containing each word
2. **Select** — Interactive TUI lets you pick which clip to use for each word
3. **Extract** — Downloads audio from archive.org, runs Whisper for word-level timestamps
4. **Process** — Applies infomercial treatment: pitch shift, EQ, compression, reverb
5. **Export** — Writes the final stitched WAV file

## Requirements

- Rust toolchain (for building)
- Internet connection (for searching and downloading clips)
- ~142MB disk space for the Whisper model

No ffmpeg or other external tools required — BadTV is a fully standalone binary.

## License

MIT
