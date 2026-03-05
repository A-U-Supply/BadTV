pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

fn rms_to_lufs(rms: f32) -> f32 {
    if rms <= 0.0 {
        return -100.0;
    }
    20.0 * rms.log10()
}

pub fn normalize_loudness(samples: &mut [f32], target_lufs: f32) {
    let current_rms = rms(samples);
    let current_lufs = rms_to_lufs(current_rms);
    if current_lufs <= -100.0 {
        return;
    }
    let gain_db = target_lufs - current_lufs;
    let gain_linear = 10.0f32.powf(gain_db / 20.0);
    for sample in samples.iter_mut() {
        *sample *= gain_linear;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_silence() {
        assert_eq!(rms(&[0.0; 100]), 0.0);
    }

    #[test]
    fn test_rms_known_signal() {
        let samples: Vec<f32> = (0..44100)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        assert!((rms(&samples) - 0.707).abs() < 0.01);
    }

    #[test]
    fn test_normalize_increases_quiet() {
        let mut s = vec![0.01; 1000];
        let before = rms(&s);
        normalize_loudness(&mut s, -16.0);
        assert!(rms(&s) > before);
    }

    #[test]
    fn test_normalize_decreases_loud() {
        let mut s = vec![0.9; 1000];
        let before = rms(&s);
        normalize_loudness(&mut s, -20.0);
        assert!(rms(&s) < before);
    }
}
