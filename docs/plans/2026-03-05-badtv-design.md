# BadTV Design Document

**Date:** 2026-03-05
**Status:** Approved

## Overview

BadTV is a standalone Rust CLI that takes a phrase, finds each word spoken on TV news
broadcasts via the Internet Archive's TV News Archive, extracts the audio of just that
word using Whisper-based word-level alignment, and stitches them into a single WAV file
with infomercial-style audio processing.

The result: a chaotic, punchy, cut-up audio collage that sounds like a fake infomercial
made from real TV clips — each word from a different voice, show, and era.

## Architecture

```
Input: "buy now and save"

  1. Search          2. Fetch           3. Whisper          4. Process        5. Export
 ┌──────────┐    ┌──────────┐    ┌──────────────┐    ┌──────────────┐    ┌─────────┐
 │ GDELT TV │───>│ Download  │───>│ Word-level   │───>│ Infomercial  │───>│  Write  │
 │ API      │    │ audio seg │    │ alignment &  │    │ treatment    │    │  WAV    │
 │ per word │    │ from IA   │    │ extraction   │    │ pipeline     │    │         │
 └──────────┘    └──────────┘    └──────────────┘    └──────────────┘    └─────────┘
                                                           │
                                        TUI: interactive clip selection per word
                                        Fallback: retry variations -> TTS
```

### Five Stages

1. **Search** — Query GDELT TV API for each word, get multiple clip candidates
2. **Fetch** — Download the relevant audio segment from archive.org (~30s around the
   timestamp, not the full hour)
3. **Whisper** — Run whisper-rs on the segment to get word-level timestamps via DTW,
   extract the exact word
4. **Process** — Apply infomercial treatment: pitch normalization, compression, EQ,
   reverb, crossfade
5. **Export** — Write final stitched WAV

## Data Flow & API Integration

### GDELT TV API Search

For each word in the input phrase:

```
GET https://api.gdeltproject.org/api/v2/tv/tv
  ?query={word} station:{station}
  &mode=clipgallery
  &format=json
  &MAXRECORDS=10
```

Response fields used:
- `ia_show_id` — archive.org item ID (e.g. `CNNW_20170705_180000_CNN_Newsroom_With_Brooke_Baldwin`)
- `preview_url` — contains `#start/{seconds}/end/{seconds}` timestamps
- `snippet` — caption text around the word
- `station`, `show`, `date`

### Audio Fetch

```
https://archive.org/download/{ia_show_id}/{ia_show_id}.mp3
```

Parse `start`/`end` from `preview_url` and download a ~30-second window around the
word. Decode from MP3 to PCM using `symphonia`.

### Word Extraction Pipeline

1. Download ~30s MP3 segment, decode to PCM with `symphonia`
2. Run `whisper-rs` on the PCM with DTW enabled for word-level timestamps
3. Find the target word in whisper output, extract start/end times
4. Cut the exact word audio with ~20ms padding for clean edges

### Fallback Chain

When a word is not found in the TV archive:

1. Try original word
2. Try common variations (plurals, tense changes)
3. Try phonetically similar words
4. Fall back to system TTS (`tts` crate — macOS `say` / Linux `espeak`)

## Audio Processing Pipeline

All processing operates on `Vec<f32>` PCM samples (mono, 44.1kHz).

```
Raw word clips -> Normalize -> Pitch shift -> EQ -> Compress -> Crossfade/stitch -> Reverb -> Limiter -> WAV
```

### Processing Stages

| Stage | Description | Default | CLI flag |
|-------|-------------|---------|----------|
| Loudness normalize | Match all clips to same LUFS level | -16 LUFS | `--loudness <db>` |
| Pitch normalize | Shift clips toward target pitch (enthusiastic/upbeat) | +2 semitones | `--pitch <semitones>` |
| EQ | Mid-boost + high-end roll-off for TV speaker character | "tv" preset | `--eq <preset\|off>` |
| Compressor | Aggressive compression for punchy infomercial energy | 4:1 ratio, fast attack | `--compress <ratio>` |
| Crossfade | Overlap between words for flow | 30ms | `--crossfade <ms>` |
| Gap | Silence between words | 50ms | `--gap <ms>` |
| Reverb | Slight room reverb for cohesion | 15% wet | `--reverb <0-100>` |
| Limiter | Brick-wall limiter to prevent clipping | -1 dBFS | `--limit <db>` |

### Implementation

- Pitch shifting: `pitch_shift` crate (phase vocoder)
- EQ: biquad filter using standard audio cookbook formulas
- Compressor: envelope follower + gain reduction
- Reverb: Freeverb algorithm
- Limiter: lookahead brickwall

## CLI Interface

### Basic Usage

```
badtv "buy now and save" -o infomercial.wav
```

### Full Options

```
badtv [OPTIONS] <PHRASE>

Arguments:
  <PHRASE>              The phrase to construct from TV clips

Options:
  -o, --output <PATH>   Output file path [default: badtv_output.wav]
  --station <STATION>   Filter to specific stations (repeatable)
  --exclude <STATION>   Exclude specific stations (repeatable)
  --no-tui              Skip interactive selection, pick randomly
  --model <PATH>        Path to whisper model file [default: ~/.badtv/ggml-base.en.bin]
  --download-model      Download the whisper model if not present

Audio processing:
  --pitch <SEMITONES>   Pitch shift [default: 2]
  --loudness <LUFS>     Target loudness [default: -16]
  --crossfade <MS>      Crossfade duration [default: 30]
  --gap <MS>            Gap between words [default: 50]
  --reverb <0-100>      Reverb wet mix [default: 15]
  --compress <RATIO>    Compressor ratio [default: 4.0]
  --eq <PRESET>         EQ preset: tv, bright, flat, off [default: tv]
  --limit <DB>          Limiter ceiling [default: -1.0]
  --raw                 Skip all audio processing, just stitch
```

### Interactive TUI (default)

For each word, the TUI shows candidate clips and lets the user choose:

```
Word 1: "act"

  > 1. CNN Newsroom (2019-03-14)    "we must ACT on..."
    2. MSNBC Rachel Maddow (2021)   "ACT of congress"
    3. Fox News (2020-11-15)        "ACT quickly to..."

  arrows: navigate  enter: select  r: random  p: preview  q: quit
```

- Arrow keys to navigate candidates
- Enter to select a clip
- `r` to pick random
- `p` to preview (play audio clip via `rodio`)
- `q` to quit

Non-interactive mode available via `--no-tui` for scripting.

## Project Structure

```
badtv/
├── Cargo.toml
├── README.md
├── docs/
│   └── plans/
│       └── 2026-03-05-badtv-design.md
└── src/
    ├── main.rs              — CLI parsing (clap), orchestration
    ├── search.rs            — GDELT TV API client
    ├── fetch.rs             — archive.org audio download
    ├── align.rs             — whisper-rs word alignment
    ├── fallback.rs          — retry variations + TTS fallback
    ├── process/
    │   ├── mod.rs           — processing pipeline orchestration
    │   ├── normalize.rs     — loudness normalization
    │   ├── pitch.rs         — pitch shifting
    │   ├── eq.rs            — biquad EQ
    │   ├── compressor.rs    — dynamics compression
    │   ├── reverb.rs        — freeverb
    │   ├── limiter.rs       — brick-wall limiter
    │   └── crossfade.rs     — crossfade + gap insertion
    ├── tui.rs               — ratatui interactive selection
    ├── audio.rs             — PCM types, decode/encode, sample rate conversion
    └── model.rs             — whisper model download/management
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `symphonia` | MP3 decoding |
| `hound` | WAV encoding |
| `rubato` | Resampling (if source sample rates differ) |
| `pitch_shift` | Phase vocoder pitch shifting |
| `whisper-rs` | Word-level speech alignment (compiles whisper.cpp into binary) |
| `tts` | System TTS fallback |
| `reqwest` | HTTP client (GDELT API + archive.org downloads) |
| `serde` / `serde_json` | JSON parsing |
| `clap` | CLI argument parsing |
| `ratatui` | TUI for interactive clip selection |
| `rodio` | Audio playback for TUI preview |
| `indicatif` | Progress bars |
| `anyhow` | Error handling |

Note: `whisper-rs` compiles whisper.cpp (C code) via a build script. The resulting
binary is fully standalone — no runtime dependency on whisper or ffmpeg.

## Error Handling

- `anyhow` for the binary — all errors propagate with `.context()`
- Graceful degradation: network errors on one word don't kill the whole run
- Each word search is independent — if one fails, report it and continue with fallback
- Clear error messages for common issues (missing model, network failures, no results)

## Whisper Model Management

- Default location: `~/.badtv/ggml-base.en.bin` (~142MB for base.en)
- `--download-model` flag fetches from huggingface on first run
- `--model` flag to specify a custom model path
- Clear error if model missing: "Run `badtv --download-model` to download the Whisper model"

## Testing Strategy

- Unit tests for each audio processor (feed known samples, verify output)
- Integration tests with mock GDELT responses + local test audio files
- Mock HTTP responses with `wiremock` (can't hit live APIs in CI)
