use crate::audio::AudioBuffer;

pub fn stitch(clips: &[AudioBuffer], crossfade_ms: u32, gap_ms: u32) -> AudioBuffer {
    if clips.is_empty() {
        return AudioBuffer::new(vec![], 44100);
    }
    let sr = clips[0].sample_rate;
    let xfade = (crossfade_ms as f32 / 1000.0 * sr as f32) as usize;
    let gap = (gap_ms as f32 / 1000.0 * sr as f32) as usize;
    let mut out: Vec<f32> = Vec::new();

    for (i, clip) in clips.iter().enumerate() {
        if i == 0 {
            out.extend_from_slice(&clip.samples);
            continue;
        }
        if gap > 0 {
            out.extend(std::iter::repeat_n(0.0f32, gap));
        }
        let xl = xfade.min(out.len()).min(clip.samples.len());
        if xl > 0 {
            let start = out.len() - xl;
            for j in 0..xl {
                let t = j as f32 / xl as f32;
                out[start + j] = out[start + j] * (1.0 - t) + clip.samples[j] * t;
            }
            out.extend_from_slice(&clip.samples[xl..]);
        } else {
            out.extend_from_slice(&clip.samples);
        }
    }
    AudioBuffer::new(out, sr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert!(stitch(&[], 30, 50).samples.is_empty());
    }

    #[test]
    fn test_single() {
        assert_eq!(
            stitch(&[AudioBuffer::new(vec![1.0; 100], 44100)], 30, 50)
                .samples
                .len(),
            100
        );
    }

    #[test]
    fn test_gap() {
        let a = AudioBuffer::new(vec![1.0; 1000], 44100);
        let b = AudioBuffer::new(vec![0.5; 1000], 44100);
        let r = stitch(&[a, b], 0, 100);
        assert_eq!(r.samples.len(), 1000 + (0.1 * 44100.0) as usize + 1000);
    }

    #[test]
    fn test_crossfade_blends() {
        let a = AudioBuffer::new(vec![1.0; 1000], 44100);
        let b = AudioBuffer::new(vec![0.0; 1000], 44100);
        let r = stitch(&[a, b], 500, 0);
        let xl = (0.5 * 44100.0) as usize;
        let xs = 1000 - xl.min(1000);
        let mid = xs + xl.min(1000) / 2;
        if mid < r.samples.len() {
            let v = r.samples[mid];
            assert!(v > 0.0 && v < 1.0, "got {}", v);
        }
    }
}
