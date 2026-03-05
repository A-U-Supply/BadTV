pub fn limit(samples: &mut [f32], ceiling_db: f32) {
    let ceiling = 10.0f32.powf(ceiling_db / 20.0);
    for s in samples.iter_mut() {
        if *s > ceiling {
            *s = ceiling;
        } else if *s < -ceiling {
            *s = -ceiling;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clips_loud() {
        let mut s = vec![0.0, 0.5, 1.0, -1.0, 1.5, -1.5];
        limit(&mut s, -1.0);
        let c = 10.0f32.powf(-1.0 / 20.0);
        for x in &s {
            assert!(x.abs() <= c + 0.001);
        }
    }

    #[test]
    fn test_preserves_quiet() {
        let o = vec![0.0, 0.1, -0.1, 0.05];
        let mut s = o.clone();
        limit(&mut s, -1.0);
        assert_eq!(s, o);
    }

    #[test]
    fn test_0db() {
        let mut s = vec![1.5, -1.5, 0.5];
        limit(&mut s, 0.0);
        assert!((s[0] - 1.0).abs() < 0.001);
        assert!((s[1] + 1.0).abs() < 0.001);
        assert!((s[2] - 0.5).abs() < 0.001);
    }
}
