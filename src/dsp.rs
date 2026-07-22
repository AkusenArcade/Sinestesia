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

/// Frequenza centrale della banda `b`, in Hz. Inverte il binning logaritmico.
pub fn band_center_hz(b: usize) -> f32 {
    let t = (b as f32 + 0.5) / NUM_BANDS as f32;
    FREQ_MIN * (FREQ_MAX / FREQ_MIN).powf(t)
}

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

/// Differenza di tempo che porta l'immagine al bordo del palco frontale.
///
/// Nella stereofonia a due canali una differenza di ~0.6 ms tra i due segnali
/// basta a spostare l'immagine fino al diffusore (legge del ritardo, Haas):
/// oltre non si va, l'immagine si limita a restare al bordo.
const ITD_FULL: f32 = 0.0006;
/// Estremi del crossover della teoria duplex: sotto domina l'ITD, sopra l'ILD.
const DUPLEX_LO_HZ: f32 = 700.0;
const DUPLEX_HI_HZ: f32 = 1600.0;
/// Semiapertura del palco stereo: i due diffusori del triangolo standard
/// stanno a ±30° dalla direzione di ascolto (ITU-R BS.775).
pub const STAGE_HALF_ANGLE: f32 = std::f32::consts::PI / 6.0;

/// Snapshot dell'immagine stereo **frontale**: per ogni banda, da dove arriva
/// il suono dentro il palco e quanto è localizzato.
#[derive(Debug, Clone, Copy)]
pub struct ImagingFrame {
    /// Azimut della componente direzionale, radianti: 0 = fronte,
    /// ±[`STAGE_HALF_ANGLE`] = diffusore destro / sinistro.
    ///
    /// Solo **arco frontale**, ed entro il palco: da due canali il fronte/retro
    /// non è recuperabile (cono di confusione) e una sorgente panpottata non
    /// esiste fuori dalla base dei diffusori. Disegnare oltre sarebbe inventare.
    pub azimuth: [f32; NUM_BANDS],
    /// Diffusività: 0 = sorgente localizzata, 1 = campo decorrelato (ambienza,
    /// riverbero). È l'energia che non ha una direzione, non "il suono dietro".
    pub diffuseness: [f32; NUM_BANDS],
    /// Energia della banda (0..1), stessa mappatura dB degli spettri.
    pub energy: [f32; NUM_BANDS],
}

impl Default for ImagingFrame {
    fn default() -> Self {
        Self {
            azimuth: [0.0; NUM_BANDS],
            diffuseness: [0.0; NUM_BANDS],
            energy: [0.0; NUM_BANDS],
        }
    }
}

/// Peso dell'ITD nel crossover duplex: pieno alle basse, nullo alle alte.
///
/// Sopra ~1.5 kHz la lunghezza d'onda è più corta della testa e la fase
/// interaurale diventa ambigua (`ωτ` supera π), quindi la direzione la porta
/// l'ombra della testa, cioè il livello.
fn duplex_itd_weight(freq: f32) -> f32 {
    ((DUPLEX_HI_HZ - freq) / (DUPLEX_HI_HZ - DUPLEX_LO_HZ)).clamp(0.0, 1.0)
}

/// Analizzatore dell'immagine stereo.
///
/// A differenza di [`Analyzer`], che scarta la fase, qui servono i bin
/// **complessi** di entrambi i canali: due suoni con lo stesso livello su L e R
/// possono essere uno al centro (in fase) o larghissimo (in controfase), e solo
/// il cross-spettro `L · conj(R)` distingue i due casi.
pub struct ImagingAnalyzer {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    samples: Vec<f32>,
    buf_l: Vec<Complex<f32>>,
    buf_r: Vec<Complex<f32>>,
    band_edges: Vec<usize>,
    /// Frequenza centrale di ogni banda, in Hz (serve al crossover duplex e a
    /// convertire la fase interaurale in un ritardo).
    band_freqs: Vec<f32>,
    smoothed: ImagingFrame,
}

impl ImagingAnalyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|n| {
                let x = std::f32::consts::PI * 2.0 * n as f32 / (FFT_SIZE as f32 - 1.0);
                0.5 - 0.5 * x.cos()
            })
            .collect();
        let band_edges = compute_band_edges();
        let bin_hz = (SAMPLE_RATE as f32 / 2.0) / (FFT_SIZE as f32 / 2.0);
        let band_freqs = (0..NUM_BANDS)
            .map(|b| {
                let lo = band_edges[b];
                let hi = band_edges[b + 1].max(lo + 1);
                (lo + hi) as f32 * 0.5 * bin_hz
            })
            .collect();
        Self {
            fft,
            window,
            samples: vec![0.0; FFT_SIZE],
            buf_l: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            buf_r: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            band_edges,
            band_freqs,
            smoothed: ImagingFrame::default(),
        }
    }

    /// Calcola pan, coerenza ed energia per banda dal cross-spettro L/R.
    pub fn analyze(&mut self, audio: &AudioBuffer, gain: f32) -> ImagingFrame {
        audio.snapshot(Channel::Left, &mut self.samples);
        for i in 0..FFT_SIZE {
            self.buf_l[i] = Complex::new(self.samples[i] * self.window[i], 0.0);
        }
        audio.snapshot(Channel::Right, &mut self.samples);
        for i in 0..FFT_SIZE {
            self.buf_r[i] = Complex::new(self.samples[i] * self.window[i], 0.0);
        }
        self.fft.process(&mut self.buf_l);
        self.fft.process(&mut self.buf_r);

        for b in 0..NUM_BANDS {
            let lo = self.band_edges[b];
            let hi = self.band_edges[b + 1].max(lo + 1);

            // Auto-spettri e cross-spettro **complesso**, aggregati sulla banda.
            // Serve complesso: il modulo dice quanto i canali sono lo stesso
            // segnale, la fase di quanto è ritardato tra i due.
            let (mut sll, mut srr) = (0.0f32, 0.0f32);
            let mut slr = Complex::new(0.0f32, 0.0f32);
            let mut peak = 0.0f32;
            for k in lo..hi {
                let (l, r) = (self.buf_l[k], self.buf_r[k]);
                let (pl, pr) = (l.norm_sqr(), r.norm_sqr());
                sll += pl;
                srr += pr;
                slr += l * r.conj();
                peak = peak.max(((pl + pr) * 0.5).sqrt());
            }

            // Coerenza in **modulo**: 1 = stesso segnale (anche se ritardato),
            // 0 = scorrelati. La parte reale, che usavo prima, vale cos(ωτ) e
            // quindi oscilla con la frequenza: su materiale binaurale cambiava
            // segno da una banda all'altra e sballottava l'immagine.
            let coh = (slr.norm() / (sll * srr).sqrt().max(1e-12)).clamp(0.0, 1.0);

            // Differenza di ampiezza, normalizzata: -1 = tutto a sinistra,
            // 0 = centro, +1 = tutto a destra. È esattamente il rapporto dei
            // guadagni di un pan-pot, quindi non serve alcuna scala in dB.
            let (al, ar) = (sll.sqrt(), srr.sqrt());
            let pan_ild = (ar - al) / (ar + al + 1e-12);

            // ITD: la fase del cross-spettro è ωτ. Fase positiva = R ritardato
            // = sorgente a sinistra, da cui il segno meno.
            let freq = self.band_freqs[b].max(1.0);
            let tau = slr.arg() / (std::f32::consts::TAU * freq);
            let pan_itd = -(tau / ITD_FULL).clamp(-1.0, 1.0) * duplex_itd_weight(freq) * coh;

            // I due cue si sommano (time-intensity trading): un pan-pot dà solo
            // ampiezza, una coppia di microfoni distanziati solo tempo, e le
            // tecniche intermedie entrambi.
            let pan = (pan_itd + pan_ild).clamp(-1.0, 1.0);

            let mag = peak / (FFT_SIZE as f32 * 0.5);
            let db = 20.0 * (mag + 1e-9).log10();
            let energy = (((db + 70.0) / 70.0).clamp(0.0, 1.0) * gain).clamp(0.0, 1.0);

            let coeff = if energy > self.smoothed.energy[b] {
                ATTACK
            } else {
                DECAY
            };
            self.smoothed.energy[b] += (energy - self.smoothed.energy[b]) * coeff;

            // Le bande deboli hanno direzione dominata dal rumore: le si fa
            // muovere lentamente, così l'immagine non sfarfalla nei silenzi.
            let k = 0.10 + 0.30 * self.smoothed.energy[b];

            // Legge della tangente: tan(θ) / tan(θ₀) = pan, con θ₀ semiapertura
            // del palco. È la relazione che lega i guadagni dei due diffusori
            // alla direzione effettivamente percepita al punto di ascolto, ed è
            // il motivo per cui un pan a metà strada *non* si sente a 15°.
            let azimuth = (pan * STAGE_HALF_ANGLE.tan()).atan();
            self.smoothed.azimuth[b] += (azimuth - self.smoothed.azimuth[b]) * k;

            // La coerenza misura la diffusività solo se **entrambi** i canali
            // portano segnale: con un pan estremo `sll → 0`, il rapporto è fatto
            // di rumore e crolla — ma quello è il caso più localizzato che
            // esista, non un campo diffuso. Quindi la si spegne man mano che
            // l'immagine si sbilancia, altrimenti un suono tutto a destra viene
            // letto come ambienza e spalmato su tutto l'arco.
            let diffuse = (1.0 - coh) * (1.0 - pan.abs());
            self.smoothed.diffuseness[b] += (diffuse - self.smoothed.diffuseness[b]) * k;
        }

        self.smoothed
    }
}

impl Default for ImagingAnalyzer {
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
