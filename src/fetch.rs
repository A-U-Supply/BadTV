use anyhow::{bail, Context, Result};
use std::io::Cursor;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::AudioBuffer;

/// Download MP3 from archive.org and decode the segment between
/// `start_secs` and `end_secs` into an AudioBuffer.
/// Adds `padding_secs` on each side for whisper context.
pub async fn fetch_audio_segment(
    client: &reqwest::Client,
    mp3_url: &str,
    start_secs: f64,
    end_secs: f64,
    padding_secs: f64,
) -> Result<AudioBuffer> {
    let padded_start = (start_secs - padding_secs).max(0.0);
    let padded_end = end_secs + padding_secs;

    let bytes = client
        .get(mp3_url)
        .send()
        .await
        .context("Failed to download MP3 from archive.org")?
        .bytes()
        .await
        .context("Failed to read MP3 bytes")?;

    decode_mp3_segment(&bytes, padded_start, padded_end)
}

/// Decode a segment of an MP3 byte buffer into mono f32 PCM.
pub fn decode_mp3_segment(mp3_bytes: &[u8], start_secs: f64, end_secs: f64) -> Result<AudioBuffer> {
    let cursor = Cursor::new(mp3_bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("Failed to probe MP3 format")?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .context("No audio track found in MP3")?;

    let sample_rate = track
        .codec_params
        .sample_rate
        .context("No sample rate in MP3")?;
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(1);
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .context("Failed to create MP3 decoder")?;

    let start_sample = (start_secs * sample_rate as f64) as u64;
    let end_sample = (end_secs * sample_rate as f64) as u64;

    let mut all_samples: Vec<f32> = Vec::new();
    let mut current_sample: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => bail!("Error reading MP3 packet: {}", e),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let num_frames = decoded.frames();
        let packet_end = current_sample + num_frames as u64;

        if packet_end < start_sample {
            current_sample = packet_end;
            continue;
        }

        if current_sample > end_sample {
            break;
        }

        let mut sample_buf = SampleBuffer::<f32>::new(num_frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let interleaved = sample_buf.samples();

        for frame in 0..num_frames {
            let global_sample = current_sample + frame as u64;
            if global_sample >= start_sample && global_sample <= end_sample {
                let mut sum = 0.0f32;
                for ch in 0..channels {
                    sum += interleaved[frame * channels + ch];
                }
                all_samples.push(sum / channels as f32);
            }
        }

        current_sample = packet_end;
    }

    if all_samples.is_empty() {
        bail!(
            "No audio decoded for segment {:.1}s - {:.1}s",
            start_secs,
            end_secs
        );
    }

    Ok(AudioBuffer::new(all_samples, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_empty_returns_error() {
        let result = decode_mp3_segment(&[], 0.0, 1.0);
        assert!(result.is_err());
    }
}
