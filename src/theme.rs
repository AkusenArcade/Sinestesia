//! Integrazione col tema di sistema.
//!
//! In modalità colore automatica i colori del grafico seguono l'**accent
//! color** di libadwaita: è la stessa fonte usata dalla chrome dell'app,
//! fornita dal portale `org.freedesktop.appearance` e aggiornata live quando
//! cambia il tema (matugen/noctalia). Questo evita di leggere file generati
//! che possono restare stale.

use crate::config::Rgb;
use crate::render::Palette;

/// Schiarisce un colore verso il bianco di un fattore `t` (0..1).
fn lighten(c: Rgb, t: f32) -> Rgb {
    Rgb::new(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
    )
}

/// Palette derivata dall'accent color di sistema: gradiente accent → tinta più
/// chiara. Coerente con la chrome e aggiornata live al cambio tema.
pub fn accent_palette() -> Palette {
    let rgba = adw::StyleManager::default().accent_color_rgba();
    let base = Rgb::new(rgba.red(), rgba.green(), rgba.blue());
    Palette {
        color_a: base,
        color_b: lighten(base, 0.5),
    }
}
