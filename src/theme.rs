//! Integrazione col tema di sistema (matugen: Bioma o noctalia).
//!
//! In modalità colore automatica i colori del grafico seguono l'accent del
//! tema generato da matugen, che la shell scrive in `~/.config/gtk-4.0/`:
//! `bioma.css` con Bioma, `noctalia.css` con noctalia (rigenerati ad ogni
//! cambio tema). Si legge il primo che c'è, nell'ordine di [`THEME_FILES`], e
//! si osservano entrambi con un file watcher per l'aggiornamento live.
//!
//! Se nessuno dei due esiste, si ripiega sull'accent color di libadwaita.

use crate::config::Rgb;
use crate::render::Palette;
use notify::Watcher;
use std::ffi::OsStr;
use std::path::PathBuf;

/// Schiarisce un colore verso il bianco di un fattore `t` (0..1).
fn lighten(c: Rgb, t: f32) -> Rgb {
    Rgb::new(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
    )
}

/// File CSS GTK generati da matugen, in ordine di preferenza. Bioma viene
/// prima: se c'è, è la shell in uso; `noctalia.css` può restare su disco,
/// fermo, dopo il passaggio.
const THEME_FILES: [&str; 2] = ["bioma.css", "noctalia.css"];

/// Cartella dei CSS GTK 4, dove la shell scrive i file tema.
fn gtk4_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gtk-4.0"))
}

/// Converte `#rrggbb` in [`Rgb`] normalizzato.
fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    if s.len() < 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Rgb::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
}

/// Estrae il valore di `@define-color <name> #rrggbb;` da un CSS GTK.
fn parse_css_color(content: &str, name: &str) -> Option<Rgb> {
    for line in content.lines() {
        let l = line.trim().trim_end_matches(';');
        let mut it = l.split_whitespace();
        if it.next() != Some("@define-color") {
            continue;
        }
        if it.next() != Some(name) {
            continue;
        }
        if let Some(val) = it.next() {
            if let Some(rgb) = parse_hex(val) {
                return Some(rgb);
            }
        }
    }
    None
}

/// Legge l'accent del tema matugen dal primo file tema che lo definisce.
fn theme_accent() -> Option<Rgb> {
    let dir = gtk4_dir()?;
    THEME_FILES.iter().find_map(|name| {
        let content = std::fs::read_to_string(dir.join(name)).ok()?;
        parse_css_color(&content, "accent_color")
            .or_else(|| parse_css_color(&content, "accent_bg_color"))
    })
}

/// Palette automatica: accent del tema matugen → tinta più chiara. Se il file
/// matugen non c'è, ripiega sull'accent color di libadwaita.
pub fn auto_palette() -> Palette {
    let base = theme_accent().unwrap_or_else(|| {
        let rgba = adw::StyleManager::default().accent_color_rgba();
        Rgb::new(rgba.red(), rgba.green(), rgba.blue())
    });
    Palette {
        color_a: base,
        color_b: lighten(base, 0.5),
    }
}

/// Osserva i file del tema matugen: `on_change` viene invocata (da un thread
/// del watcher) ad ogni modifica di uno dei [`THEME_FILES`].
pub fn watch_theme<F>(on_change: F) -> Option<notify::RecommendedWatcher>
where
    F: Fn() + Send + 'static,
{
    let dir = gtk4_dir()?;
    let mut watcher = notify::recommended_watcher(
        move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else {
                return;
            };
            if matches!(
                event.kind,
                notify::EventKind::Modify(_) | notify::EventKind::Create(_)
            ) && event
                .paths
                .iter()
                .any(|p| {
                    p.file_name()
                        .is_some_and(|n| THEME_FILES.iter().any(|f| n == OsStr::new(f)))
                })
            {
                on_change();
            }
        },
    )
    .ok()?;
    watcher
        .watch(&dir, notify::RecursiveMode::NonRecursive)
        .ok()?;
    Some(watcher)
}
