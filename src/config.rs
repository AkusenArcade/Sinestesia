//! Persistenza delle impostazioni in `~/.config/sinestesia/config.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Sorgente audio da catturare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioSource {
    /// Audio in uscita (monitor del sink di default).
    Output,
    /// Audio in ingresso (microfono / sorgente di default).
    Input,
}

impl Default for AudioSource {
    fn default() -> Self {
        AudioSource::Output
    }
}

/// Effetto di visualizzazione attivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// Barre verticali stile Cava (MVP).
    Bars,
    /// Linea continua dello spettro.
    Line,
    /// Radiale con particelle.
    Radial,
    /// Variante "neon" della linea: riempimento trasparente + bordo luminoso.
    LineSpectrum,
    /// Variante "neon" del radiale: curva continua trasparente + bordo luminoso.
    RadialSpectrum,
    /// Tunnel: anelli che congelano la sagoma dello spettro e si espandono
    /// verso l'osservatore, con vortice e campo di stelle.
    Tunnel,
    /// Poliedro: solido geodetico 3D con spigoli luminosi, deformato dallo
    /// spettro e con le facce che si estrudono sui transienti.
    Solid,
    /// Imaging: semicerchio frontale con la direzione percepita del suono
    /// (pan + coerenza di fase) sull'arco, un lobo per fascia di frequenza.
    Imaging,
    /// Rilievo: spettrogramma 3D: ogni frame nasce una cresta in primo piano e
    /// le vecchie scorrono verso l'orizzonte sfumando nella foschia.
    Terrain,
    /// Fase: vettorscopio esteso nel tempo — la traiettoria mid/side della
    /// forma d'onda si avvita in profondità mentre la camera oscilla.
    Phase,
    /// Nebulosa: campo di particelle su gusci sferici concentrici (bassi al
    /// centro, acuti in superficie); i transienti lanciano onde d'urto radiali.
    Nebula,
}

impl Default for Effect {
    fn default() -> Self {
        Effect::Bars
    }
}

/// Modalità di colorazione del visualizzatore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// Due colori scelti manualmente (gradiente A→B).
    Manual,
    /// Colori derivati dal tema di sistema (noctalia/matugen).
    Auto,
}

impl Default for ColorMode {
    fn default() -> Self {
        ColorMode::Auto
    }
}

/// Colore RGB normalizzato (0.0–1.0).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }
}

/// Impostazioni persistenti dell'applicazione.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub effect: Effect,
    pub source: AudioSource,
    pub color_mode: ColorMode,
    /// Primo colore del gradiente in modalità manuale.
    pub color_a: Rgb,
    /// Secondo colore del gradiente in modalità manuale.
    pub color_b: Rgb,
    /// Moltiplicatore di ampiezza (0.1–10.0).
    pub gain: f32,
    /// Intensità del motion blur (0.0–0.95).
    pub blur: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            effect: Effect::default(),
            source: AudioSource::default(),
            color_mode: ColorMode::default(),
            color_a: Rgb::new(0.84, 0.73, 1.0), // viola chiaro
            color_b: Rgb::new(0.95, 0.72, 0.77), // rosa
            gain: 1.0,
            blur: 0.0,
        }
    }
}

impl Settings {
    /// Percorso del file di configurazione (`~/.config/sinestesia/config.toml`).
    pub fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("sinestesia").join("config.toml"))
    }

    /// Carica le impostazioni dal file; ritorna i default se assente o malformato.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).unwrap_or_else(|e| {
                log::warn!("config malformato ({e}), uso i default");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Salva le impostazioni su file, creando la directory se necessario.
    pub fn save(&self) -> anyhow::Result<()> {
        let Some(path) = Self::config_path() else {
            anyhow::bail!("impossibile determinare la config dir");
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)?;
        std::fs::write(&path, s)?;
        Ok(())
    }

    /// Limita il gain al range valido.
    pub fn clamp_gain(gain: f32) -> f32 {
        gain.clamp(0.1, 10.0)
    }

    /// Limita il motion blur al range valido.
    pub fn clamp_blur(blur: f32) -> f32 {
        blur.clamp(0.0, 0.95)
    }
}
