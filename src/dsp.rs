//! Analisi dello spettro: finestratura Hann, FFT, binning logaritmico,
//! smoothing temporale, normalizzazione e applicazione del gain.

use crate::audio::{AudioBuffer, Channel, SAMPLE_RATE};
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::sync::Arc;

/// Dimensione della finestra FFT (potenza di 2).
pub const FFT_SIZE: usize = 2048;
/// Numero di bande di output (barre).
pub const NUM_BANDS: usize = 64;

/// Frequenza minima/massima rappresentata nel binning logaritmico.
const FREQ_MIN: f32 = 30.0;
const FREQ_MAX: f32 = 16_000.0;

/// Coefficienti di smoothing temporale (attack veloce, decay più lento).
const ATTACK: f32 = 0.45;
const DECAY: f32 = 0.18;

/// Uno snapshot dello spettro: `NUM_BANDS` valori normalizzati 0.0–1.0.
pub type SpectrumFrame = [f32; NUM_BANDS];

/// Analizzatore di spettro riutilizzabile (alloca i buffer una sola volta).
pub struct Analyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
    /// Indici di confine dei bin logaritmici (NUM_BANDS+1 valori).
    band_edges: Vec<usize>,
    /// Valori smussati per frame-to-frame.
    smoothed: SpectrumFrame,
}

impl Analyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);

        // Finestra di Hann.
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|n| {
                let x = std::f32::consts::PI * 2.0 * n as f32 / (FFT_SIZE as f32 - 1.0);
                0.5 - 0.5 * x.cos()
            })
            .collect();

        let band_edges = compute_band_edges();

        Self {
            fft,
            window,
            samples: vec![0.0; FFT_SIZE],
            fft_buf: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            band_edges,
            smoothed: [0.0; NUM_BANDS],
        }
    }

    /// Legge gli ultimi campioni del canale indicato e produce lo spettro
    /// smussato, applicando il moltiplicatore `gain`.
    pub fn analyze(&mut self, audio: &AudioBuffer, channel: Channel, gain: f32) -> SpectrumFrame {
        audio.snapshot(channel, &mut self.samples);

        // Finestratura + caricamento nel buffer complesso.
        for i in 0..FFT_SIZE {
            self.fft_buf[i] = Complex::new(self.samples[i] * self.window[i], 0.0);
        }

        self.fft.process(&mut self.fft_buf);

        // Magnitudo per bin (solo metà spettro utile).
        // Aggreghiamo i bin FFT nelle bande logaritmiche prendendo il picco.
        let mut bands = [0.0f32; NUM_BANDS];
        for b in 0..NUM_BANDS {
            let lo = self.band_edges[b];
            let hi = self.band_edges[b + 1].max(lo + 1);
            let mut peak = 0.0f32;
            for bin in lo..hi {
                let m = self.fft_buf[bin].norm();
                if m > peak {
                    peak = m;
                }
            }
            bands[b] = peak;
        }

        // Normalizzazione: compressione logaritmica della gamma dinamica.
        // Scala in dB e mappa un range utile in 0..1.
        for b in 0..NUM_BANDS {
            let mag = bands[b] / (FFT_SIZE as f32 * 0.5);
            let db = 20.0 * (mag + 1e-9).log10();
            // -70 dB → 0, 0 dB → 1
            let norm = ((db + 70.0) / 70.0).clamp(0.0, 1.0);
            bands[b] = (norm * gain).clamp(0.0, 1.0);
        }

        // Smoothing temporale asimmetrico (attack/decay).
        for b in 0..NUM_BANDS {
            let target = bands[b];
            let coeff = if target > self.smoothed[b] {
                ATTACK
            } else {
                DECAY
            };
            self.smoothed[b] += (target - self.smoothed[b]) * coeff;
        }

        self.smoothed
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Calcola gli indici dei bin FFT che delimitano ogni banda su scala log.
fn compute_band_edges() -> Vec<usize> {
    let nyquist = SAMPLE_RATE as f32 / 2.0;
    let bin_hz = nyquist / (FFT_SIZE as f32 / 2.0);
    let log_min = FREQ_MIN.log10();
    let log_max = FREQ_MAX.log10();

    let mut edges = Vec::with_capacity(NUM_BANDS + 1);
    for i in 0..=NUM_BANDS {
        let t = i as f32 / NUM_BANDS as f32;
        let freq = 10f32.powf(log_min + t * (log_max - log_min));
        let bin = (freq / bin_hz).round() as usize;
        edges.push(bin.min(FFT_SIZE / 2 - 1));
    }
    edges
}
