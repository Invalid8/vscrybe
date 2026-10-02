use std::path::Path;
use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use serde::Deserialize;

pub const N_FFT: usize = 400;
pub const HOP: usize = 160;
pub const WINDOW_SAMPLES: usize = 480_000;
pub const FRAMES: usize = WINDOW_SAMPLES / HOP;
const BINS: usize = N_FFT / 2 + 1;

#[derive(Deserialize)]
struct PreprocessorConfig {
    feature_size: usize,
    sampling_rate: usize,
    mel_filters: Option<Vec<Vec<f64>>>,
}

pub struct LogMel {
    pub n_mels: usize,
    filters: Vec<f32>,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

impl LogMel {
    pub fn load(model: &Path, n_mels: usize) -> Result<Self, String> {
        let file = model.join("preprocessor_config.json");
        let config = if file.is_file() {
            let text = std::fs::read_to_string(&file).map_err(|e| format!("Couldn't read {}: {e}", file.display()))?;
            serde_json::from_str(&text).map_err(|e| format!("Bad {}: {e}", file.display()))?
        } else {
            PreprocessorConfig { feature_size: n_mels, sampling_rate: 16_000, mel_filters: None }
        };
        let filters = match config.mel_filters {
            Some(rows) => rows.into_iter().flatten().map(|v| v as f32).collect(),
            None => mel_spec::mel::mel(config.sampling_rate as f64, N_FFT, config.feature_size, None, None, false, true)
                .iter()
                .map(|v| *v as f32)
                .collect(),
        };
        Ok(Self::new(config.feature_size, filters))
    }

    pub fn new(n_mels: usize, filters: Vec<f32>) -> Self {
        assert_eq!(filters.len(), n_mels * BINS, "mel filters must be n_mels × {BINS}");
        let window = (0..N_FFT).map(|n| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / N_FFT as f32).cos()).collect();
        Self { n_mels, filters, window, fft: FftPlanner::new().plan_fft_forward(N_FFT) }
    }

    pub fn compute(&self, audio: &[f32]) -> Vec<f32> {
        let mut signal = audio[..audio.len().min(WINDOW_SAMPLES)].to_vec();
        signal.resize(WINDOW_SAMPLES, 0.0);
        let pad = N_FFT / 2;
        let padded: Vec<f32> = (1..=pad)
            .rev()
            .map(|i| signal[i])
            .chain(signal.iter().copied())
            .chain((0..pad).map(|i| signal[WINDOW_SAMPLES - 2 - i]))
            .collect();

        let mut mel = vec![0.0f32; self.n_mels * FRAMES];
        let mut buffer = vec![Complex32::default(); N_FFT];
        let mut power = [0.0f32; BINS];
        for frame in 0..FRAMES {
            let start = frame * HOP;
            for (n, slot) in buffer.iter_mut().enumerate() {
                *slot = Complex32::new(padded[start + n] * self.window[n], 0.0);
            }
            self.fft.process(&mut buffer);
            for (bin, p) in power.iter_mut().enumerate() {
                *p = buffer[bin].norm_sqr();
            }
            for m in 0..self.n_mels {
                let row = &self.filters[m * BINS..(m + 1) * BINS];
                let energy: f32 = row.iter().zip(&power).map(|(f, p)| f * p).sum();
                mel[m * FRAMES + frame] = energy.max(1e-10).log10();
            }
        }
        let floor = mel.iter().copied().fold(f32::NEG_INFINITY, f32::max) - 8.0;
        mel.iter_mut().for_each(|v| *v = (v.max(floor) + 4.0) / 4.0);
        mel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_flat_and_tone_lights_its_band() {
        let mel = LogMel::new(80, mel_spec::mel::mel(16000.0, N_FFT, 80, None, None, false, true).iter().map(|v| *v as f32).collect());
        let silence = mel.compute(&[]);
        assert_eq!(silence.len(), 80 * FRAMES);
        assert!(silence.iter().all(|v| (*v - silence[0]).abs() < 1e-6));

        let tone: Vec<f32> = (0..16000).map(|i| (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 16000.0).sin()).collect();
        let features = mel.compute(&tone);
        let band = |m: usize| features[m * FRAMES + 50];
        let loudest = (0..80).max_by(|a, b| band(*a).total_cmp(&band(*b))).unwrap();
        assert!((25..45).contains(&loudest), "1 kHz peaked in mel band {loudest}");
        assert!(features[loudest * FRAMES + 2000] < band(loudest), "padding after the tone is quieter");
    }
}
