struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
    damp: f32,
    damp_prev: f32,
}

impl CombFilter {
    fn new(size: usize, feedback: f32, damp: f32) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
            feedback,
            damp,
            damp_prev: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.buffer[self.index];
        self.damp_prev = out * (1.0 - self.damp) + self.damp_prev * self.damp;
        self.buffer[self.index] = input + self.damp_prev * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        out
    }
}

struct AllPassFilter {
    buffer: Vec<f32>,
    index: usize,
    feedback: f32,
}

impl AllPassFilter {
    fn new(size: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; size],
            index: 0,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf = self.buffer[self.index];
        self.buffer[self.index] = input + buf * self.feedback;
        self.index = (self.index + 1) % self.buffer.len();
        -input + buf
    }
}

struct Freeverb {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllPassFilter>,
}

impl Freeverb {
    fn new(sr: u32) -> Self {
        let scale = sr as f32 / 44100.0;
        Self {
            combs: [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617]
                .iter()
                .map(|&s| CombFilter::new((s as f32 * scale) as usize, 0.84, 0.2))
                .collect(),
            allpasses: [556, 441, 341, 225]
                .iter()
                .map(|&s| AllPassFilter::new((s as f32 * scale) as usize, 0.5))
                .collect(),
        }
    }

    fn tick(&mut self, input: f32) -> f32 {
        let mut out = 0.0;
        for c in &mut self.combs {
            out += c.process(input);
        }
        for a in &mut self.allpasses {
            out = a.process(out);
        }
        out
    }
}

pub fn apply_reverb(samples: &mut [f32], sample_rate: u32, wet_percent: u32) {
    if wet_percent == 0 || samples.is_empty() {
        return;
    }
    let wet = wet_percent.min(100) as f32 / 100.0;
    let dry = 1.0 - wet;
    let mut rev = Freeverb::new(sample_rate);
    for s in samples.iter_mut() {
        let d = *s;
        *s = d * dry + rev.tick(d) * wet;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_wet_identity() {
        let o = vec![0.5, -0.3, 0.1, 0.8, -0.6];
        let mut s = o.clone();
        apply_reverb(&mut s, 44100, 0);
        assert_eq!(s, o);
    }

    #[test]
    fn test_creates_tail() {
        let mut s = vec![0.0f32; 44100];
        s[0] = 1.0;
        let o = s.clone();
        apply_reverb(&mut s, 44100, 50);
        assert!(
            s[10000..20000].iter().map(|x| x.abs()).sum::<f32>()
                > o[10000..20000].iter().map(|x| x.abs()).sum::<f32>()
        );
    }

    #[test]
    fn test_preserves_length() {
        let mut s = vec![0.1; 1000];
        apply_reverb(&mut s, 44100, 30);
        assert_eq!(s.len(), 1000);
    }
}
