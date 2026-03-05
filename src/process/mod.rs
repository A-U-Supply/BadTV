pub mod normalize;
pub mod pitch;
pub mod eq;
pub mod compressor;
pub mod reverb;
pub mod limiter;
pub mod crossfade;

use crate::audio::AudioBuffer;

#[derive(Debug, Clone)]
pub struct ProcessParams {
    pub loudness_lufs: f32,
    pub pitch_semitones: f32,
    pub eq_preset: String,
    pub compress_ratio: f32,
    pub crossfade_ms: u32,
    pub gap_ms: u32,
    pub reverb_wet: u32,
    pub limit_db: f32,
}

impl Default for ProcessParams {
    fn default() -> Self {
        Self {
            loudness_lufs: -16.0,
            pitch_semitones: 2.0,
            eq_preset: "tv".to_string(),
            compress_ratio: 4.0,
            crossfade_ms: 30,
            gap_ms: 50,
            reverb_wet: 15,
            limit_db: -1.0,
        }
    }
}

pub fn apply_pipeline(clips: &[AudioBuffer], params: &ProcessParams) -> AudioBuffer {
    let processed: Vec<AudioBuffer> = clips
        .iter()
        .map(|clip| {
            let mut buf = clip.clone();
            normalize::normalize_loudness(&mut buf.samples, params.loudness_lufs);
            if params.pitch_semitones.abs() > 0.01 {
                buf.samples = pitch::shift(&buf.samples, buf.sample_rate, params.pitch_semitones);
            }
            eq::apply_eq(&mut buf.samples, buf.sample_rate, &params.eq_preset);
            compressor::compress(&mut buf.samples, params.compress_ratio);
            buf
        })
        .collect();

    let mut assembled = crossfade::stitch(&processed, params.crossfade_ms, params.gap_ms);

    if params.reverb_wet > 0 {
        reverb::apply_reverb(&mut assembled.samples, assembled.sample_rate, params.reverb_wet);
    }
    limiter::limit(&mut assembled.samples, params.limit_db);

    assembled
}
