pub fn compress(samples: &mut [f32], ratio: f32) {
    if ratio <= 1.0 || samples.is_empty() {
        return;
    }
    let threshold = 10.0f32.powf(-20.0 / 20.0);
    let attack = (-1.0f32 / (0.001 * 44100.0)).exp();
    let release = (-1.0f32 / (0.050 * 44100.0)).exp();
    let mut env = 0.0f32;
    for s in samples.iter_mut() {
        let abs = s.abs();
        env = if abs > env {
            attack * env + (1.0 - attack) * abs
        } else {
            release * env + (1.0 - release) * abs
        };
        if env > threshold {
            let over = 20.0 * (env / threshold).log10();
            let gain = 10.0f32.powf(-(over - over / ratio) / 20.0);
            *s *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reduces_loud() {
        let mut s = vec![0.8f32; 44100];
        compress(&mut s, 4.0);
        // After the envelope settles, tail samples should be reduced
        let tail_peak = s[4410..]
            .iter()
            .map(|x| x.abs())
            .fold(0.0f32, f32::max);
        assert!(tail_peak < 0.8, "tail peak was {}", tail_peak);
    }

    #[test]
    fn test_leaves_quiet() {
        let mut s = vec![0.01f32; 4410];
        let o = s.clone();
        compress(&mut s, 4.0);
        for (a, b) in s.iter().zip(o.iter()) {
            assert!((a - b).abs() < 0.001);
        }
    }

    #[test]
    fn test_ratio_1_noop() {
        let mut s = vec![0.8f32; 1000];
        let o = s.clone();
        compress(&mut s, 1.0);
        assert_eq!(s, o);
    }
}
