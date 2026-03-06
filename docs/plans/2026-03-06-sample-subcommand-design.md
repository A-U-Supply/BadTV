# Sample Subcommand Design

## Summary

Add a `badtv sample` subcommand that downloads multiple isolated word clips
from TV news archives, saving them as individual WAV files for later use.

## CLI Changes

Restructured from flat args to subcommands:

- `badtv stitch "phrase" [opts]` — existing phrase-stitching behavior
- `badtv sample "word" [opts]` — new word sampling behavior

Shared options (station filters, model path) are in a flattened `SharedArgs`
struct reused by both subcommands.

## Sample Subcommand

```
badtv sample "buy" -n 10 --station CNN
```

### Arguments

| Arg | Short | Default | Description |
|-----|-------|---------|-------------|
| `word` | (positional) | required | Word to sample |
| `--count` | `-n` | 5 | Number of clips to collect |
| `--output-dir` | `-o` | `./{word}/` | Output directory |
| `--station` | | (none) | Filter to stations |
| `--exclude` | | (none) | Exclude stations |
| `--model` | | `~/.badtv/ggml-base.en.bin` | Whisper model |
| `--download-model` | | false | Auto-download model |

### Pipeline

1. Search IA TV caption index for the word
2. Filter to clips with both MP3 and SRT available
3. For each clip (until count reached):
   - Parse SRT for caption hits containing the word
   - Fetch audio segment around caption timing (10s padding)
   - Run whisper alignment for word-level timestamps
   - Extract isolated word audio
   - Save as `{word}-{identifier}.wav`
4. One sample per clip (for variety across sources)

### Output

```
buy/
  buy-CNBC_20090829_080000_Mad_Money.wav
  buy-CNN_20170705_180000_Newsroom.wav
  buy-FOXNEWS_20200115_120000_Outnumbered.wav
  ...
```
