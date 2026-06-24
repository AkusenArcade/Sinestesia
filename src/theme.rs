//! Integrazione col tema di sistema (matugen / noctalia).
//!
//! In modalità colore automatica i colori del grafico seguono l'accent del
//! tema generato da matugen, che noctalia scrive in
//! `~/.config/gtk-4.0/noctalia.css` (rigenerato ad ogni cambio tema). Lo si
//! legge e lo si osserva con un file watcher per l'aggiornamento live.
//!
//! Se quel file non esiste, si ripiega sull'accent color di libadwaita.

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

/// File CSS GTK generato da noctalia (matugen).
fn noctalia_css_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("gtk-4.0").join("noctalia.css"))
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

/// Legge l'accent del tema matugen da noctalia.css.
fn noctalia_accent() -> Option<Rgb> {
    let content = std::fs::read_to_string(noctalia_css_path()?).ok()?;
    parse_css_color(&content, "accent_color")
        .or_else(|| parse_css_color(&content, "accent_bg_color"))
}

/// Palette automatica: accent del tema matugen → tinta più chiara. Se il file
/// matugen non c'è, ripiega sull'accent color di libadwaita.
pub fn auto_palette() -> Palette {
    let base = noctalia_accent().unwrap_or_else(|| {
        let rgba = adw::StyleManager::default().accent_color_rgba();
        Rgb::new(rgba.red(), rgba.green(), rgba.blue())
    });
    Palette {
        color_a: base,
        color_b: lighten(base, 0.5),
    }
}

/// Osserva il file del tema matugen: `on_change` viene invocata (da un thread
/// del watcher) ad ogni modifica di `noctalia.css`.
pub fn watch_theme<F>(on_change: F) -> Option<notify::RecommendedWatcher>
where
    F: Fn() + Send + 'static,
{
    let path = noctalia_css_path()?;
    let dir = path.parent()?.to_path_buf();
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
                .any(|p| p.file_name() == Some(OsStr::new("noctalia.css")))
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
