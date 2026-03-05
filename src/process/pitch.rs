use pitch_shift::PitchShifter;

/// Shift pitch by the given number of semitones using a phase vocoder.
pub fn shift(samples: &[f32], sample_rate: u32, semitones: f32) -> Vec<f32> {
    if samples.is_empty() || semitones.abs() < 0.01 {
        return samples.to_vec();
    }
    let shift_factor = 2.0f32.powf(semitones / 12.0);
    let mut shifter = PitchShifter::new(50, sample_rate as usize);
    let mut output = vec![0.0f32; samples.len()];
    shifter.shift_pitch(16, shift_factor, samples, &mut output);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shift_zero_identity() {
        let s: Vec<f32> = (0..4410)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let shifted = shift(&s, 44100, 0.0);
        assert_eq!(shifted.len(), s.len());
        // Zero semitones should return the original samples unchanged
        for (a, b) in s.iter().zip(shifted.iter()) {
            assert!((a - b).abs() < 0.1);
        }
    }

    #[test]
    fn test_shift_preserves_length() {
        assert_eq!(shift(&vec![0.5; 44100], 44100, 5.0).len(), 44100);
    }

    #[test]
    fn test_shift_empty() {
        assert!(shift(&[], 44100, 3.0).is_empty());
    }
}
