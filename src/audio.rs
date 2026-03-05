use anyhow::{Context, Result};

/// A buffer of mono f32 PCM audio samples at a known sample rate.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl AudioBuffer {
    pub fn new(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }

    /// Duration in seconds.
    pub fn duration(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }

    /// Create a silent buffer of a given duration.
    pub fn silence(duration_secs: f32, sample_rate: u32) -> Self {
        let num_samples = (duration_secs * sample_rate as f32) as usize;
        Self {
            samples: vec![0.0; num_samples],
            sample_rate,
        }
    }

    /// Extract a sub-range by time (seconds).
    pub fn slice(&self, start_secs: f32, end_secs: f32) -> Self {
        let start = (start_secs * self.sample_rate as f32) as usize;
        let end = (end_secs * self.sample_rate as f32).min(self.samples.len() as f32) as usize;
        Self {
            samples: self.samples[start..end].to_vec(),
            sample_rate: self.sample_rate,
        }
    }

    /// Write to WAV file.
    pub fn write_wav(&self, path: &str) -> Result<()> {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: self.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer =
            hound::WavWriter::create(path, spec).context("Failed to create WAV file")?;
        for &sample in &self.samples {
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        Ok(())
    }

    /// Read from WAV file.
    pub fn read_wav(path: &str) -> Result<Self> {
        let mut reader =
            hound::WavReader::open(path).context("Failed to open WAV file")?;
        let spec = reader.spec();
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<hound::Result<Vec<f32>>>()
                .context("Failed to read float samples")?,
            hound::SampleFormat::Int => {
                let max = (1 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .collect::<hound::Result<Vec<i32>>>()
                    .context("Failed to read int samples")?
                    .into_iter()
                    .map(|s| s as f32 / max)
                    .collect()
            }
        };
        Ok(Self {
            samples,
            sample_rate: spec.sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration() {
        let buf = AudioBuffer::new(vec![0.0; 44100], 44100);
        assert!((buf.duration() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_silence() {
        let buf = AudioBuffer::silence(0.5, 44100);
        assert_eq!(buf.samples.len(), 22050);
        assert!(buf.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_slice() {
        let buf = AudioBuffer::new(vec![1.0; 44100], 44100);
        let sliced = buf.slice(0.0, 0.5);
        assert_eq!(sliced.samples.len(), 22050);
    }

    #[test]
    fn test_wav_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wav");
        let path_str = path.to_str().unwrap();

        let original = AudioBuffer::new(vec![0.0, 0.5, -0.5, 1.0, -1.0], 44100);
        original.write_wav(path_str).unwrap();

        let loaded = AudioBuffer::read_wav(path_str).unwrap();
        assert_eq!(loaded.sample_rate, 44100);
        assert_eq!(loaded.samples.len(), 5);
        for (a, b) in original.samples.iter().zip(loaded.samples.iter()) {
            assert!((a - b).abs() < 0.001);
        }
    }
}
