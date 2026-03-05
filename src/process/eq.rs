use std::f32::consts::PI;

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    fn peaking(sr: u32, freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sr as f32;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * w0.cos()) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * w0.cos()) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    fn low_shelf(sr: u32, freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sr as f32;
        let alpha = w0.sin() / (2.0 * q);
        let tsa = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) + (a - 1.0) * w0.cos() + tsa;
        Self {
            b0: a * ((a + 1.0) - (a - 1.0) * w0.cos() + tsa) / a0,
            b1: 2.0 * a * ((a - 1.0) - (a + 1.0) * w0.cos()) / a0,
            b2: a * ((a + 1.0) - (a - 1.0) * w0.cos() - tsa) / a0,
            a1: -2.0 * ((a - 1.0) + (a + 1.0) * w0.cos()) / a0,
            a2: ((a + 1.0) + (a - 1.0) * w0.cos() - tsa) / a0,
        }
    }

    fn high_shelf(sr: u32, freq: f32, gain_db: f32, q: f32) -> Self {
        let a = 10.0f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq / sr as f32;
        let alpha = w0.sin() / (2.0 * q);
        let tsa = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) - (a - 1.0) * w0.cos() + tsa;
        Self {
            b0: a * ((a + 1.0) + (a - 1.0) * w0.cos() + tsa) / a0,
            b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * w0.cos()) / a0,
            b2: a * ((a + 1.0) + (a - 1.0) * w0.cos() - tsa) / a0,
            a1: 2.0 * ((a - 1.0) - (a + 1.0) * w0.cos()) / a0,
            a2: ((a + 1.0) - (a - 1.0) * w0.cos() - tsa) / a0,
        }
    }

    fn process(&self, samples: &mut [f32]) {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for s in samples.iter_mut() {
            let x0 = *s;
            let y0 = self.b0 * x0 + self.b1 * x1 + self.b2 * x2 - self.a1 * y1 - self.a2 * y2;
            x2 = x1;
            x1 = x0;
            y2 = y1;
            y1 = y0;
            *s = y0;
        }
    }
}

pub fn apply_eq(samples: &mut [f32], sample_rate: u32, preset: &str) {
    match preset {
        "off" | "flat" => {}
        "tv" => {
            Biquad::low_shelf(sample_rate, 200.0, -3.0, 0.7).process(samples);
            Biquad::peaking(sample_rate, 2000.0, 4.0, 1.0).process(samples);
            Biquad::high_shelf(sample_rate, 8000.0, -4.0, 0.7).process(samples);
        }
        "bright" => {
            Biquad::peaking(sample_rate, 3000.0, 3.0, 1.0).process(samples);
            Biquad::high_shelf(sample_rate, 6000.0, 3.0, 0.7).process(samples);
        }
        _ => eprintln!("Unknown EQ preset '{}', using flat", preset),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eq_off_identity() {
        let o = vec![0.5, -0.3, 0.1, 0.0, -0.7];
        let mut s = o.clone();
        apply_eq(&mut s, 44100, "off");
        assert_eq!(s, o);
    }

    #[test]
    fn test_eq_tv_modifies() {
        let mut s: Vec<f32> = (0..4410)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / 44100.0).sin())
            .collect();
        let o = s.clone();
        apply_eq(&mut s, 44100, "tv");
        assert!(
            s.iter()
                .zip(o.iter())
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                > 0.0
        );
    }

    #[test]
    fn test_eq_preserves_length() {
        let mut s = vec![0.1; 1000];
        apply_eq(&mut s, 44100, "bright");
        assert_eq!(s.len(), 1000);
    }
}
