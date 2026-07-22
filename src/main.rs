//! Sinestesia — visualizzatore audio per Linux.

mod audio;
mod config;
mod dsp;
mod render;
mod theme;

use adw::prelude::*;
use audio::{AudioBuffer, AudioHandle};
use config::{AudioSource, ColorMode, Effect, Rgb, Settings};
use render::{Palette, VizState};
use relm4::prelude::*;
use relm4::{adw, gtk};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// CSS applicato globalmente. L'area del visualizzatore resta nera
/// indipendentemente dal tema della chrome (FR-8.5); il resto dell'UI
/// eredita il tema di sistema (noctalia/matugen) via libadwaita (FR-8).
const APP_CSS: &str = "
.viz-area {
    background-color: #000000;
}
";

/// Stato dell'applicazione.
struct App {
    settings: Settings,
    /// Stato condiviso col renderer (spettro, palette, gain).
    viz: Rc<RefCell<VizState>>,
    /// Ring buffer condiviso, riusato tra le sessioni di cattura.
    buffer: Arc<AudioBuffer>,
    /// Sessione di cattura attiva (None solo transitoriamente).
    audio: Option<AudioHandle>,
    /// Watcher del file tema matugen (noctalia.css), per il live reload colori.
    _theme_watcher: Option<notify::RecommendedWatcher>,
    /// Modalità "solo visualizzatore": header bar e pannello controlli
    /// nascosti. Volutamente NON persistita: si rientra sempre in finestra
    /// normale, altrimenti al riavvio ci si ritroverebbe senza controlli e
    /// senza sapere come farli riapparire.
    chrome_hidden: bool,
    fullscreen: bool,
}

/// Messaggi dell'applicazione.
#[derive(Debug)]
enum Msg {
    SetEffect(Effect),
    SetSource(AudioSource),
    SetColorMode(ColorMode),
    SetColorA(Rgb),
    SetColorB(Rgb),
    SetGain(f64),
    SetBlur(f64),
    /// Il file colori noctalia è cambiato: ricarica la palette (se Auto).
    ReloadAutoTheme,
    /// Mostra/nasconde header bar e pannello controlli (H).
    ToggleChrome,
    /// Entra/esce da schermo intero, nascondendo anche le barre (F11).
    ToggleFullscreen,
    /// Esce dalla modalità immersiva e ripristina le barre (Esc).
    ExitImmersive,
}

/// Etichette del dropdown effetti (l'ordine definisce gli indici).
const EFFECT_LABELS: [&str; 8] = [
    "Barre",
    "Linea",
    "Radiale",
    "Linea Spettro",
    "Radiale Spettro",
    "Tunnel",
    "Poliedro",
    "Imaging",
];

/// Mappa l'effetto all'indice del dropdown.
fn effect_index(e: Effect) -> u32 {
    match e {
        Effect::Bars => 0,
        Effect::Line => 1,
        Effect::Radial => 2,
        Effect::LineSpectrum => 3,
        Effect::RadialSpectrum => 4,
        Effect::Tunnel => 5,
        Effect::Solid => 6,
        Effect::Imaging => 7,
    }
}

/// Mappa l'indice del dropdown all'effetto.
fn index_effect(i: u32) -> Effect {
    match i {
        1 => Effect::Line,
        2 => Effect::Radial,
        3 => Effect::LineSpectrum,
        4 => Effect::RadialSpectrum,
        5 => Effect::Tunnel,
        6 => Effect::Solid,
        7 => Effect::Imaging,
        _ => Effect::Bars,
    }
}

fn rgb_to_rgba(c: Rgb) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::new(c.r, c.g, c.b, 1.0)
}

fn rgba_to_rgb(c: gtk::gdk::RGBA) -> Rgb {
    Rgb::new(c.red(), c.green(), c.blue())
}

/// Palette iniziale in base alla modalità colore corrente.
fn palette_for(settings: &Settings) -> Palette {
    match settings.color_mode {
        ColorMode::Manual => Palette {
            color_a: settings.color_a,
            color_b: settings.color_b,
        },
        ColorMode::Auto => theme::auto_palette(),
    }
}

impl App {
    /// Riapplica la palette al renderer secondo la modalità corrente.
    fn apply_palette(&self) {
        self.viz.borrow_mut().palette = palette_for(&self.settings);
    }
}

#[relm4::component]
impl SimpleComponent for App {
    type Init = Settings;
    type Input = Msg;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some("Sinestesia"),
            set_default_size: (1100, 640),
            #[watch]
            set_fullscreened: model.fullscreen,

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,

                adw::HeaderBar {
                    #[watch]
                    set_visible: !model.chrome_hidden,
                    #[wrap(Some)]
                    set_title_widget = &adw::WindowTitle {
                        set_title: "Sinestesia",
                        set_subtitle: "Visualizzatore audio",
                    },
                    pack_end = &gtk::Button {
                        set_icon_name: "view-fullscreen-symbolic",
                        set_tooltip_text: Some(
                            "Solo visualizzatore (F11) · H nasconde le barre · Esc esce",
                        ),
                        connect_clicked[sender] => move |_| {
                            sender.input(Msg::ToggleFullscreen);
                        },
                    },
                },

                // Area principale del visualizzatore (sfondo nero).
                #[name = "viz_box"]
                gtk::Box {
                    set_vexpand: true,
                    set_hexpand: true,
                    add_css_class: "viz-area",
                },

                // Pannello controlli.
                gtk::Box {
                    #[watch]
                    set_visible: !model.chrome_hidden,
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 10,
                    set_margin_all: 12,
                    set_valign: gtk::Align::Center,

                    gtk::Label { set_label: "Effetto" },
                    gtk::DropDown {
                        set_model: Some(&gtk::StringList::new(&EFFECT_LABELS)),
                        set_selected: effect_index(model.settings.effect),
                        connect_selected_notify[sender] => move |dd| {
                            sender.input(Msg::SetEffect(index_effect(dd.selected())));
                        },
                    },

                    gtk::Separator { set_orientation: gtk::Orientation::Vertical },

                    gtk::Label { set_label: "Uscita" },
                    gtk::Switch {
                        set_valign: gtk::Align::Center,
                        set_active: matches!(model.settings.source, AudioSource::Input),
                        set_tooltip_text: Some("Sorgente: spento = audio in uscita, acceso = ingresso"),
                        connect_active_notify[sender] => move |sw| {
                            let s = if sw.is_active() {
                                AudioSource::Input
                            } else {
                                AudioSource::Output
                            };
                            sender.input(Msg::SetSource(s));
                        },
                    },
                    gtk::Label { set_label: "Ingresso" },

                    gtk::Separator { set_orientation: gtk::Orientation::Vertical },

                    gtk::Label { set_label: "Tema auto" },
                    gtk::Switch {
                        set_valign: gtk::Align::Center,
                        set_active: matches!(model.settings.color_mode, ColorMode::Auto),
                        set_tooltip_text: Some("Acceso: colori dal tema di sistema (noctalia)"),
                        connect_active_notify[sender] => move |sw| {
                            let m = if sw.is_active() {
                                ColorMode::Auto
                            } else {
                                ColorMode::Manual
                            };
                            sender.input(Msg::SetColorMode(m));
                        },
                    },
                    gtk::ColorDialogButton {
                        set_dialog: &gtk::ColorDialog::new(),
                        set_valign: gtk::Align::Center,
                        set_tooltip_text: Some("Colore A (base barre)"),
                        #[watch]
                        set_sensitive: matches!(model.settings.color_mode, ColorMode::Manual),
                        set_rgba: &rgb_to_rgba(model.settings.color_a),
                        connect_rgba_notify[sender] => move |b| {
                            sender.input(Msg::SetColorA(rgba_to_rgb(b.rgba())));
                        },
                    },
                    gtk::ColorDialogButton {
                        set_dialog: &gtk::ColorDialog::new(),
                        set_valign: gtk::Align::Center,
                        set_tooltip_text: Some("Colore B (cima barre)"),
                        #[watch]
                        set_sensitive: matches!(model.settings.color_mode, ColorMode::Manual),
                        set_rgba: &rgb_to_rgba(model.settings.color_b),
                        connect_rgba_notify[sender] => move |b| {
                            sender.input(Msg::SetColorB(rgba_to_rgb(b.rgba())));
                        },
                    },

                    gtk::Separator { set_orientation: gtk::Orientation::Vertical },

                    gtk::Label { set_label: "Gain" },
                    gtk::Scale {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_hexpand: true,
                        set_draw_value: true,
                        set_value_pos: gtk::PositionType::Right,
                        set_digits: 1,
                        set_range: (0.1, 10.0),
                        set_increments: (0.1, 1.0),
                        set_value: model.settings.gain as f64,
                        set_width_request: 140,
                        connect_value_changed[sender] => move |s| {
                            sender.input(Msg::SetGain(s.value()));
                        },
                    },

                    gtk::Label { set_label: "Scia" },
                    gtk::Scale {
                        set_orientation: gtk::Orientation::Horizontal,
                        set_hexpand: true,
                        set_draw_value: true,
                        set_value_pos: gtk::PositionType::Right,
                        set_digits: 2,
                        set_range: (0.0, 0.95),
                        set_increments: (0.05, 0.1),
                        set_value: model.settings.blur as f64,
                        set_width_request: 140,
                        set_tooltip_text: Some("Motion blur: scie più lunghe a valori alti"),
                        connect_value_changed[sender] => move |s| {
                            sender.input(Msg::SetBlur(s.value()));
                        },
                    },
                },
            }
        }
    }

    fn init(
        settings: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Stato condiviso col renderer, inizializzato dalle impostazioni.
        let viz = Rc::new(RefCell::new(VizState {
            spectrum_left: [0.0; dsp::NUM_BANDS],
            spectrum_right: [0.0; dsp::NUM_BANDS],
            imaging: dsp::ImagingFrame::default(),
            palette: palette_for(&settings),
            gain: settings.gain,
            effect: settings.effect,
            mirror: settings.source == AudioSource::Input,
            blur: settings.blur,
        }));

        // Avvia la cattura audio.
        let buffer = AudioBuffer::new(dsp::FFT_SIZE * 2);
        let audio = Some(audio::start(buffer.clone(), settings.source));

        // Osserva il file tema matugen (noctalia.css): in Auto la palette si
        // aggiorna live quando cambia il tema di sistema.
        let theme_watcher = {
            let input = sender.input_sender().clone();
            theme::watch_theme(move || {
                let _ = input.send(Msg::ReloadAutoTheme);
            })
        };

        let model = App {
            settings,
            viz: viz.clone(),
            buffer: buffer.clone(),
            audio,
            _theme_watcher: theme_watcher,
            chrome_hidden: false,
            fullscreen: false,
        };

        // Anche un eventuale cambio dell'accent color di sistema aggiorna i colori.
        adw::StyleManager::default().connect_accent_color_rgba_notify({
            let sender = sender.clone();
            move |_| sender.input(Msg::ReloadAutoTheme)
        });

        let widgets = view_output!();

        // Scorciatoie della modalità immersiva. Fase di cattura: altrimenti
        // il widget che ha il focus (dropdown, slider) si mangia il tasto
        // prima che arrivi alla finestra.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed({
            let sender = sender.clone();
            move |_, key, _, _| {
                let msg = match key {
                    gtk::gdk::Key::F11 => Msg::ToggleFullscreen,
                    gtk::gdk::Key::h | gtk::gdk::Key::H => Msg::ToggleChrome,
                    gtk::gdk::Key::Escape => Msg::ExitImmersive,
                    _ => return gtk::glib::Propagation::Proceed,
                };
                sender.input(msg);
                gtk::glib::Propagation::Stop
            }
        });
        root.add_controller(keys);

        // Inserisce la GLArea nell'area nera del visualizzatore.
        let gl_area = render::build_gl_area(buffer, viz);
        widgets.viz_box.append(&gl_area);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, _sender: ComponentSender<Self>) {
        match msg {
            Msg::SetEffect(e) => {
                self.settings.effect = e;
                self.viz.borrow_mut().effect = e;
            }
            Msg::SetGain(g) => {
                self.settings.gain = Settings::clamp_gain(g as f32);
                self.viz.borrow_mut().gain = self.settings.gain;
            }
            Msg::SetBlur(b) => {
                self.settings.blur = Settings::clamp_blur(b as f32);
                self.viz.borrow_mut().blur = self.settings.blur;
            }
            Msg::SetSource(s) => {
                if s != self.settings.source {
                    self.settings.source = s;
                    // In input usiamo un canale specchiato sui due lati.
                    self.viz.borrow_mut().mirror = s == AudioSource::Input;
                    // Riavvia lo stream PipeWire sulla nuova sorgente.
                    if let Some(h) = self.audio.take() {
                        h.stop();
                    }
                    self.audio = Some(audio::start(self.buffer.clone(), s));
                }
            }
            Msg::SetColorMode(m) => {
                self.settings.color_mode = m;
                self.apply_palette();
            }
            Msg::SetColorA(c) => {
                self.settings.color_a = c;
                if self.settings.color_mode == ColorMode::Manual {
                    self.apply_palette();
                }
            }
            Msg::SetColorB(c) => {
                self.settings.color_b = c;
                if self.settings.color_mode == ColorMode::Manual {
                    self.apply_palette();
                }
            }
            Msg::ReloadAutoTheme => {
                if self.settings.color_mode == ColorMode::Auto {
                    self.apply_palette();
                    log::info!("colori tema (matugen) aggiornati");
                }
                return; // nessuna impostazione da salvare
            }
            Msg::ToggleChrome => {
                self.chrome_hidden = !self.chrome_hidden;
                return;
            }
            Msg::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                // Lo schermo intero implica la modalità immersiva; uscendo si
                // ripristinano le barre.
                self.chrome_hidden = self.fullscreen;
                return;
            }
            Msg::ExitImmersive => {
                self.fullscreen = false;
                self.chrome_hidden = false;
                return;
            }
        }
        if let Err(e) = self.settings.save() {
            log::warn!("salvataggio config fallito: {e}");
        }
    }
}

fn main() {
    env_logger::init();
    render::init_gl_loader();
    let settings = Settings::load();

    let app = RelmApp::new("dev.akusen.sinestesia");
    relm4::set_global_css(APP_CSS);
    app.run::<App>(settings);
}
