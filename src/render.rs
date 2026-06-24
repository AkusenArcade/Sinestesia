//! Rendering OpenGL del visualizzatore tramite `GtkGLArea` + `glow`.
//!
//! Effetti supportati (selezionabili a runtime): barre stile Cava, curva dello
//! spettro (area riempita) e radiale con particelle. Tutti condividono lo
//! stesso programma shader (posizione + colore RGBA per vertice) e variano solo
//! i vertici e la primitiva di disegno.
//!
//! Due rifiniture: le barre/linea partono trasparenti alla base e diventano
//! opache verso l'alto (alpha per vertice + blending); un parametro `blur`
//! controlla il motion blur disegnando un velo nero semi-trasparente al posto
//! del clear (le scie persistono e sfumano).

use crate::audio::{AudioBuffer, Channel};
use crate::config::{Effect, Rgb};
use crate::dsp::{Analyzer, SpectrumFrame, NUM_BANDS};
use glow::HasContext;
use gtk::glib;
use gtk::prelude::*;
use relm4::gtk;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// Palette colori del visualizzatore (gradiente A→B).
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub color_a: Rgb,
    pub color_b: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            color_a: Rgb::new(0.84, 0.73, 1.0),
            color_b: Rgb::new(0.95, 0.72, 0.77),
        }
    }
}

/// Stato condiviso tra UI (scrittura impostazioni), tick (calcolo spettro) e
/// callback di render (lettura).
///
/// Gli spettri sono due (sinistro/destro) per la visualizzazione speculare:
/// il centro rappresenta le basse frequenze, i bordi le alte.
pub struct VizState {
    pub spectrum_left: SpectrumFrame,
    pub spectrum_right: SpectrumFrame,
    pub palette: Palette,
    pub gain: f32,
    pub effect: Effect,
    /// In modalità input usiamo un solo canale specchiato sui due lati.
    pub mirror: bool,
    /// Intensità del motion blur (0.0 = nessuno, →1.0 = scie lunghe).
    pub blur: f32,
}

impl Default for VizState {
    fn default() -> Self {
        Self {
            spectrum_left: [0.0; NUM_BANDS],
            spectrum_right: [0.0; NUM_BANDS],
            palette: Palette::default(),
            gain: 1.0,
            effect: Effect::Bars,
            mirror: false,
            blur: 0.0,
        }
    }
}

/// Carica i puntatori alle funzioni OpenGL tramite libepoxy.
/// Da chiamare una sola volta all'avvio, prima di realizzare la `GLArea`.
pub fn init_gl_loader() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        #[cfg(all(unix, not(target_os = "macos")))]
        let lib = unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }
            .expect("libepoxy non trovata");
        epoxy::load_with(|name| {
            unsafe { lib.get::<*const ()>(name.as_bytes()) }
                .map(|sym| *sym as *const _)
                .unwrap_or(std::ptr::null())
        });
        // Manteniamo la libreria viva per tutta la durata del processo.
        std::mem::forget(lib);
    });
}

/// Costruisce la `GLArea` del visualizzatore, cablando realize/render e il
/// tick di analisi audio.
pub fn build_gl_area(audio: Arc<AudioBuffer>, state: Rc<RefCell<VizState>>) -> gtk::GLArea {
    let area = gtk::GLArea::builder()
        .hexpand(true)
        .vexpand(true)
        .has_depth_buffer(false)
        .has_stencil_buffer(false)
        .build();

    let renderer: Rc<RefCell<Option<Renderer>>> = Rc::new(RefCell::new(None));

    area.connect_realize({
        let renderer = renderer.clone();
        move |area| {
            area.make_current();
            if let Some(err) = area.error() {
                log::error!("errore GLArea: {err}");
                return;
            }
            let gl = unsafe {
                glow::Context::from_loader_function(|s| epoxy::get_proc_addr(s) as *const _)
            };
            match Renderer::new(gl) {
                Ok(r) => *renderer.borrow_mut() = Some(r),
                Err(e) => log::error!("init renderer fallita: {e}"),
            }
        }
    });

    area.connect_render({
        let renderer = renderer.clone();
        let state = state.clone();
        move |area, _ctx| {
            if let Some(r) = renderer.borrow_mut().as_mut() {
                let st = state.borrow();
                let (w, h) = (area.width().max(1), area.height().max(1));
                r.draw(
                    &st.spectrum_left,
                    &st.spectrum_right,
                    &st.palette,
                    st.effect,
                    w,
                    h,
                    st.blur,
                );
            }
            glib::Propagation::Stop
        }
    });

    // Tick a ogni frame: analizza i due canali e richiede il redraw.
    // Due Analyzer distinti per mantenere smoothing indipendenti per canale.
    let analyzer_l = Rc::new(RefCell::new(Analyzer::new()));
    let analyzer_r = Rc::new(RefCell::new(Analyzer::new()));
    area.add_tick_callback({
        let state = state.clone();
        move |area, _clock| {
            let (gain, mirror) = {
                let s = state.borrow();
                (s.gain, s.mirror)
            };
            let left = analyzer_l.borrow_mut().analyze(&audio, Channel::Left, gain);
            // In input lo stesso canale è specchiato sui due lati.
            let right = if mirror {
                left
            } else {
                analyzer_r.borrow_mut().analyze(&audio, Channel::Right, gain)
            };
            {
                let mut s = state.borrow_mut();
                s.spectrum_left = left;
                s.spectrum_right = right;
            }
            area.queue_render();
            glib::ControlFlow::Continue
        }
    });

    area
}

// GtkGLArea fornisce un contesto OpenGL ES: usiamo GLSL ES 3.00.
// Colore RGBA per vertice (l'alpha dà il gradiente di trasparenza).
const VERTEX_SRC: &str = r#"#version 300 es
in vec2 position;
in vec4 color;
out vec4 v_color;
void main() {
    v_color = color;
    gl_PointSize = 2.5;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const FRAGMENT_SRC: &str = r#"#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 frag;
void main() {
    frag = v_color;
}
"#;

// Shader dedicato alle particelle: punti con falloff radiale (nucleo luminoso
// + alone) pensati per il blending additivo, così sembrano luci che brillano.
const GLOW_VERTEX_SRC: &str = r#"#version 300 es
in vec2 position;
in vec4 color;
out vec4 v_color;
void main() {
    v_color = color;
    gl_PointSize = 8.0;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const GLOW_FRAGMENT_SRC: &str = r#"#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 frag;
void main() {
    float d = length(gl_PointCoord - vec2(0.5));
    float glow = smoothstep(0.5, 0.0, d);
    // nucleo + alone modulati dalla vita (alpha); intensità per il bagliore
    vec3 col = v_color.rgb * v_color.a * glow * 1.5;
    frag = vec4(col, 1.0);
}
"#;

// Shader per il bordo "neon": un ribbon lungo la curva in cui l'alpha del
// vertice trasporta la coordinata perpendicolare (-1..1). Nucleo sottile e
// luminoso (la "linea da 1px") + alone, pensato per il blending additivo.
const NEON_VERTEX_SRC: &str = r#"#version 300 es
in vec2 position;
in vec4 color;
out vec4 v_color;
void main() {
    v_color = color;
    gl_Position = vec4(position, 0.0, 1.0);
}
"#;

const NEON_FRAGMENT_SRC: &str = r#"#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 frag;
void main() {
    // Solo alone morbido: il bordo netto è una linea da 1px disegnata a parte.
    float d = abs(v_color.a);          // 0 al centro del ribbon, 1 ai bordi
    float halo = 1.0 - d;
    halo = halo * halo * 0.7;
    frag = vec4(v_color.rgb * halo, 1.0);
}
"#;

/// Componenti per vertice: x, y, r, g, b, a.
const VERT_FLOATS: usize = 6;

/// Una particella per l'effetto radiale (coordinate in spazio "quadrato").
struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    /// posizione nel gradiente A→B (0..1).
    t: f32,
}

/// Renderer OpenGL dei vari effetti.
struct Renderer {
    gl: glow::Context,
    program: glow::Program,
    /// Programma per le particelle (punti luminosi additivi).
    glow_program: glow::Program,
    /// Programma per il bordo neon (ribbon additivo).
    neon_program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    pos_loc: u32,
    col_loc: u32,
    glow_pos_loc: u32,
    glow_col_loc: u32,
    neon_pos_loc: u32,
    neon_col_loc: u32,
    particles: Vec<Particle>,
    rng: u32,
    /// Spettri del frame precedente, per stimare il movimento delle barre
    /// (emissione delle particelle dalle punte dei raggi).
    prev_left: SpectrumFrame,
    prev_right: SpectrumFrame,
    /// Al primo frame puliamo il buffer (evita garbage iniziale col blur).
    first_frame: bool,
}

impl Renderer {
    fn new(gl: glow::Context) -> anyhow::Result<Self> {
        unsafe {
            let program = link_program(&gl, VERTEX_SRC, FRAGMENT_SRC)?;
            let glow_program = link_program(&gl, GLOW_VERTEX_SRC, GLOW_FRAGMENT_SRC)?;
            let neon_program = link_program(&gl, NEON_VERTEX_SRC, NEON_FRAGMENT_SRC)?;
            let vao = gl
                .create_vertex_array()
                .map_err(|e| anyhow::anyhow!("VAO: {e}"))?;
            let vbo = gl.create_buffer().map_err(|e| anyhow::anyhow!("VBO: {e}"))?;
            let pos_loc = gl.get_attrib_location(program, "position").unwrap_or(0);
            let col_loc = gl.get_attrib_location(program, "color").unwrap_or(1);
            let glow_pos_loc = gl.get_attrib_location(glow_program, "position").unwrap_or(0);
            let glow_col_loc = gl.get_attrib_location(glow_program, "color").unwrap_or(1);
            let neon_pos_loc = gl.get_attrib_location(neon_program, "position").unwrap_or(0);
            let neon_col_loc = gl.get_attrib_location(neon_program, "color").unwrap_or(1);
            Ok(Self {
                gl,
                program,
                glow_program,
                neon_program,
                vao,
                vbo,
                pos_loc,
                col_loc,
                glow_pos_loc,
                glow_col_loc,
                neon_pos_loc,
                neon_col_loc,
                particles: Vec::new(),
                rng: 0x1234_5678,
                prev_left: [0.0; NUM_BANDS],
                prev_right: [0.0; NUM_BANDS],
                first_frame: true,
            })
        }
    }

    /// Generatore pseudo-casuale leggero (xorshift32) in [0, 1).
    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32) / (u32::MAX as f32)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(
        &mut self,
        left: &SpectrumFrame,
        right: &SpectrumFrame,
        palette: &Palette,
        effect: Effect,
        width: i32,
        height: i32,
        blur: f32,
    ) {
        unsafe {
            self.gl.viewport(0, 0, width, height);
            self.gl.enable(glow::BLEND);
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.gl.use_program(Some(self.program));
            self.gl.bind_vertex_array(Some(self.vao));

            if self.first_frame {
                self.gl.clear_color(0.0, 0.0, 0.0, 1.0);
                self.gl.clear(glow::COLOR_BUFFER_BIT);
                self.first_frame = false;
            }
        }

        // Velo nero al posto del clear: opacità 1.0 = pulizia totale (nessun
        // blur), opacità bassa = le scie del frame precedente persistono.
        let fade = (1.0 - blur).clamp(0.05, 1.0);
        let veil = build_fade_quad(fade);
        self.draw_arrays(&veil, glow::TRIANGLES, self.pos_loc, self.col_loc);

        match effect {
            Effect::Bars => {
                let verts = build_bar_vertices(left, right, palette);
                self.draw_arrays(&verts, glow::TRIANGLES, self.pos_loc, self.col_loc);
            }
            Effect::Line => {
                let verts = build_line_vertices(left, right, palette);
                self.draw_arrays(&verts, glow::TRIANGLE_STRIP, self.pos_loc, self.col_loc);
            }
            Effect::Radial => {
                let inv_aspect = height as f32 / width as f32;
                let spokes = build_radial_spokes(left, right, palette, inv_aspect);
                self.draw_arrays(&spokes, glow::TRIANGLES, self.pos_loc, self.col_loc);

                self.update_particles(left, right);
                let pts = build_particle_vertices(&self.particles, palette, inv_aspect);
                // Particelle: programma glow + blending additivo per il bagliore.
                unsafe {
                    self.gl.use_program(Some(self.glow_program));
                    self.gl.blend_func(glow::ONE, glow::ONE);
                }
                self.draw_arrays(&pts, glow::POINTS, self.glow_pos_loc, self.glow_col_loc);
            }
            Effect::LineSpectrum => {
                let (fill, glow_ribbon, line) = build_line_spectrum(left, right, palette);
                self.draw_neon(&fill, &glow_ribbon, &line);
            }
            Effect::RadialSpectrum => {
                let inv_aspect = height as f32 / width as f32;
                let (fill, glow_ribbon, line) =
                    build_radial_spectrum(left, right, palette, inv_aspect);
                self.draw_neon(&fill, &glow_ribbon, &line);

                // Particelle come nel radiale: emesse dalla curva in movimento.
                self.update_particles(left, right);
                let pts = build_particle_vertices(&self.particles, palette, inv_aspect);
                unsafe {
                    self.gl.use_program(Some(self.glow_program));
                    self.gl.blend_func(glow::ONE, glow::ONE);
                }
                self.draw_arrays(&pts, glow::POINTS, self.glow_pos_loc, self.glow_col_loc);
            }
        }

        unsafe {
            self.gl.bind_vertex_array(None);
            self.gl.disable(glow::BLEND);
        }
    }

    /// Carica i vertici e disegna con la primitiva indicata, usando le
    /// location degli attributi del programma attualmente in uso.
    fn draw_arrays(&self, verts: &[f32], mode: u32, pos_loc: u32, col_loc: u32) {
        if verts.is_empty() {
            return;
        }
        let gl = &self.gl;
        unsafe {
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes =
                std::slice::from_raw_parts(verts.as_ptr() as *const u8, std::mem::size_of_val(verts));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);

            let stride = (VERT_FLOATS * std::mem::size_of::<f32>()) as i32;
            gl.enable_vertex_attrib_array(pos_loc);
            gl.vertex_attrib_pointer_f32(pos_loc, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(col_loc);
            gl.vertex_attrib_pointer_f32(
                col_loc,
                4,
                glow::FLOAT,
                false,
                stride,
                2 * std::mem::size_of::<f32>() as i32,
            );

            let count = (verts.len() / VERT_FLOATS) as i32;
            gl.draw_arrays(mode, 0, count);
        }
    }

    /// Disegna una variante "neon": riempimento semi-trasparente + alone
    /// luminoso (ribbon additivo) + bordo netto da 1 pixel (LINE_STRIP).
    fn draw_neon(&self, fill: &[f32], glow_ribbon: &[f32], line: &[f32]) {
        // Riempimento: programma principale, blending normale (già attivo).
        self.draw_arrays(fill, glow::TRIANGLE_STRIP, self.pos_loc, self.col_loc);
        // Alone: programma neon, blending additivo.
        unsafe {
            self.gl.use_program(Some(self.neon_program));
            self.gl.blend_func(glow::ONE, glow::ONE);
        }
        self.draw_arrays(glow_ribbon, glow::TRIANGLE_STRIP, self.neon_pos_loc, self.neon_col_loc);
        // Bordo netto da 1px: programma principale, blending normale.
        unsafe {
            self.gl.use_program(Some(self.program));
            self.gl
                .blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            self.gl.line_width(1.0);
        }
        self.draw_arrays(line, glow::LINE_STRIP, self.pos_loc, self.col_loc);
    }

    /// Aggiorna il sistema di particelle: ogni raggio (barra) che cresce
    /// "spara" particelle dalla propria punta verso l'esterno, con velocità
    /// proporzionale a quanto rapidamente la barra è salita (delta tra frame).
    fn update_particles(&mut self, left: &SpectrumFrame, right: &SpectrumFrame) {
        let pi = std::f32::consts::PI;
        let denom = denom_bands();
        const CAP: usize = 2200;

        for i in 0..NUM_BANDS {
            let t = i as f32 / denom;
            // (angolo del raggio, altezza attuale, altezza precedente) per i
            // due canali: destro = semicerchio destro, sinistro = sinistro.
            let lanes = [
                (
                    std::f32::consts::FRAC_PI_2 - t * pi,
                    right[i],
                    self.prev_right[i],
                ),
                (
                    std::f32::consts::FRAC_PI_2 + t * pi,
                    left[i],
                    self.prev_left[i],
                ),
            ];
            for (ang, h, h_prev) in lanes {
                let delta = h - h_prev;
                if delta <= 0.02 {
                    continue; // la barra non sta salendo: niente emissione
                }
                let count = ((delta * 9.0) as usize).min(3);
                let tip_r = RADIAL_INNER + h.clamp(0.0, 1.0) * 0.62;
                // Velocità d'uscita proporzionale al movimento della barra
                // (dimezzata rispetto a prima per un moto più morbido).
                let base_speed = 0.005 + delta * 0.06;
                for _ in 0..count {
                    if self.particles.len() >= CAP {
                        break;
                    }
                    let jitter = (self.rand() - 0.5) * 0.06;
                    let (s, c) = (ang + jitter).sin_cos();
                    let sp = base_speed * (0.7 + self.rand() * 0.6);
                    let tt = self.rand();
                    self.particles.push(Particle {
                        x: c * tip_r,
                        y: s * tip_r,
                        vx: c * sp,
                        vy: s * sp,
                        life: 1.0,
                        t: tt,
                    });
                }
            }
        }

        // Avanzamento e rimozione delle particelle morte.
        for p in &mut self.particles {
            p.x += p.vx;
            p.y += p.vy;
            p.life -= 0.018;
        }
        self.particles
            .retain(|p| p.life > 0.0 && p.x.abs() < 1.6 && p.y.abs() < 1.6);

        // Memorizza lo spettro per il calcolo del movimento al frame successivo.
        self.prev_left = *left;
        self.prev_right = *right;
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_program(self.glow_program);
            self.gl.delete_program(self.neon_program);
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_buffer(self.vbo);
        }
    }
}

/// Raggio interno dell'anello nell'effetto radiale (spazio quadrato).
const RADIAL_INNER: f32 = 0.28;

/// Interpola linearmente due colori.
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    Rgb::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}

/// Quad nero a schermo intero con la data opacità (velo per il motion blur).
fn build_fade_quad(alpha: f32) -> Vec<f32> {
    let mut v = Vec::with_capacity(6 * VERT_FLOATS);
    let mut push = |x: f32, y: f32| v.extend_from_slice(&[x, y, 0.0, 0.0, 0.0, alpha]);
    push(-1.0, -1.0);
    push(1.0, -1.0);
    push(1.0, 1.0);
    push(-1.0, -1.0);
    push(1.0, 1.0);
    push(-1.0, 1.0);
    v
}

/// Barre verticali speculari (TRIANGLES): centro = basse frequenze, bordi =
/// alte. Metà sinistra = canale `left`, metà destra = canale `right`. La base è
/// trasparente (alpha 0) e diventa opaca verso l'alto (alpha 1).
fn build_bar_vertices(left: &SpectrumFrame, right: &SpectrumFrame, palette: &Palette) -> Vec<f32> {
    let mut v = Vec::with_capacity(NUM_BANDS * 2 * 6 * VERT_FLOATS);
    let slot = 1.0 / NUM_BANDS as f32;
    let gap = slot * 0.12;
    let ca = palette.color_a;
    let cb = palette.color_b;

    let push = |v: &mut Vec<f32>, x: f32, y: f32, c: Rgb, a: f32| {
        v.extend_from_slice(&[x, y, c.r, c.g, c.b, a]);
    };
    let bar = |v: &mut Vec<f32>, x_l: f32, x_r: f32, h: f32| {
        let y_b = -1.0;
        let y_t = -1.0 + 2.0 * h.clamp(0.0, 1.0);
        push(v, x_l, y_b, ca, 0.0);
        push(v, x_r, y_b, ca, 0.0);
        push(v, x_r, y_t, cb, 1.0);
        push(v, x_l, y_b, ca, 0.0);
        push(v, x_r, y_t, cb, 1.0);
        push(v, x_l, y_t, cb, 1.0);
    };

    for i in 0..NUM_BANDS {
        let xr_l = i as f32 * slot + gap;
        let xr_r = (i as f32 + 1.0) * slot - gap;
        bar(&mut v, xr_l, xr_r, right[i]);
        bar(&mut v, -xr_r, -xr_l, left[i]);
    }
    v
}

/// Curva continua e speculare dello spettro come area riempita
/// (TRIANGLE_STRIP unico dal bordo sinistro al destro passando per il centro).
/// Base trasparente, cima opaca.
fn build_line_vertices(left: &SpectrumFrame, right: &SpectrumFrame, palette: &Palette) -> Vec<f32> {
    let mut v = Vec::with_capacity((NUM_BANDS * 2) * 2 * VERT_FLOATS);
    let ca = palette.color_a;
    let cb = palette.color_b;
    let denom = (NUM_BANDS as f32 - 1.0).max(1.0);

    let column = |v: &mut Vec<f32>, x: f32, h: f32| {
        let y_t = -1.0 + 2.0 * h.clamp(0.0, 1.0);
        v.extend_from_slice(&[x, -1.0, ca.r, ca.g, ca.b, 0.0]);
        v.extend_from_slice(&[x, y_t, cb.r, cb.g, cb.b, 1.0]);
    };

    for i in (0..NUM_BANDS).rev() {
        column(&mut v, -(i as f32 / denom), left[i]);
    }
    for i in 0..NUM_BANDS {
        column(&mut v, i as f32 / denom, right[i]);
    }
    v
}

/// Opacità del riempimento nelle varianti "neon" (volutamente bassa).
const FILL_ALPHA: f32 = 0.20;
/// Semi-larghezza del ribbon del bordo neon, in NDC.
const NEON_HALF_WIDTH: f32 = 0.024;

/// Riempimento semi-trasparente tra una linea base e la curva (TRIANGLE_STRIP).
/// `closed` chiude l'anello (per il radiale).
fn build_fill(pts: &[(f32, f32)], bases: &[(f32, f32)], palette: &Palette, closed: bool) -> Vec<f32> {
    let n = pts.len();
    if n == 0 {
        return Vec::new();
    }
    let ca = palette.color_a;
    let cb = palette.color_b;
    let count = if closed { n + 1 } else { n };
    let mut v = Vec::with_capacity(count * 2 * VERT_FLOATS);
    for k in 0..count {
        let i = k % n;
        let (bx, by) = bases[i];
        let (px, py) = pts[i];
        v.extend_from_slice(&[bx, by, ca.r, ca.g, ca.b, 0.0]); // base trasparente
        v.extend_from_slice(&[px, py, cb.r, cb.g, cb.b, FILL_ALPHA]); // curva
    }
    v
}

/// Picco di volume dello spettro (max tra le bande dei due canali), 0..1.
/// Usato per modulare la visibilità dell'outline nelle varianti "neon".
fn peak_level(left: &SpectrumFrame, right: &SpectrumFrame) -> f32 {
    let mut m = 0.0f32;
    for i in 0..NUM_BANDS {
        m = m.max(left[i]).max(right[i]);
    }
    m.clamp(0.0, 1.0)
}

/// Bordo "neon": ribbon lungo la curva, l'alpha del vertice porta la coordinata
/// perpendicolare (+1/-1) usata dallo shader neon. Colore = primario del tema,
/// scalato per `vis` (visibilità ∝ picco di volume).
fn build_neon(pts: &[(f32, f32)], palette: &Palette, closed: bool, vis: f32) -> Vec<f32> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    // Colore primario scalato per la visibilità (glow additivo → si dissolve).
    let primary = Rgb::new(
        palette.color_a.r * vis,
        palette.color_a.g * vis,
        palette.color_a.b * vis,
    );
    let w = NEON_HALF_WIDTH;
    let count = if closed { n + 1 } else { n };
    let mut v = Vec::with_capacity(count * 2 * VERT_FLOATS);
    for k in 0..count {
        let i = k % n;
        let prev = if i == 0 {
            if closed { pts[n - 1] } else { pts[0] }
        } else {
            pts[i - 1]
        };
        let next = if i == n - 1 {
            if closed { pts[0] } else { pts[n - 1] }
        } else {
            pts[i + 1]
        };
        let tx = next.0 - prev.0;
        let ty = next.1 - prev.1;
        let len = (tx * tx + ty * ty).sqrt().max(1e-6);
        let nx = -ty / len;
        let ny = tx / len;
        let (px, py) = pts[i];
        v.extend_from_slice(&[px + nx * w, py + ny * w, primary.r, primary.g, primary.b, 1.0]);
        v.extend_from_slice(&[px - nx * w, py - ny * w, primary.r, primary.g, primary.b, -1.0]);
    }
    v
}

/// Bordo netto da 1px: la curva come polilinea (LINE_STRIP) nel colore
/// primario, con opacità = `vis` (visibilità ∝ picco di volume).
fn build_neon_line(pts: &[(f32, f32)], palette: &Palette, closed: bool, vis: f32) -> Vec<f32> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let c = palette.color_a;
    let count = if closed { n + 1 } else { n };
    let mut v = Vec::with_capacity(count * VERT_FLOATS);
    for k in 0..count {
        let (x, y) = pts[k % n];
        v.extend_from_slice(&[x, y, c.r, c.g, c.b, vis]);
    }
    v
}

/// Variante "neon" della linea: curva speculare (centro = basse) con
/// riempimento trasparente, alone e bordo da 1px. Ritorna (fill, alone, linea).
fn build_line_spectrum(
    left: &SpectrumFrame,
    right: &SpectrumFrame,
    palette: &Palette,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let denom = (NUM_BANDS as f32 - 1.0).max(1.0);
    let mut pts = Vec::with_capacity(NUM_BANDS * 2);
    for i in (0..NUM_BANDS).rev() {
        let x = -(i as f32 / denom);
        pts.push((x, -1.0 + 2.0 * left[i].clamp(0.0, 1.0)));
    }
    for i in 0..NUM_BANDS {
        let x = i as f32 / denom;
        pts.push((x, -1.0 + 2.0 * right[i].clamp(0.0, 1.0)));
    }
    let bases: Vec<(f32, f32)> = pts.iter().map(|p| (p.0, -1.0)).collect();
    let vis = peak_level(left, right);
    (
        build_fill(&pts, &bases, palette, false),
        build_neon(&pts, palette, false, vis),
        build_neon_line(&pts, palette, false, vis),
    )
}

/// Variante "neon" del radiale: curva continua chiusa attorno al cerchio
/// (basse in alto), riempimento trasparente verso l'anello interno, alone e
/// bordo da 1px. Semicerchio destro = `right`, sinistro = `left`.
fn build_radial_spectrum(
    left: &SpectrumFrame,
    right: &SpectrumFrame,
    palette: &Palette,
    inv_aspect: f32,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let pi = std::f32::consts::PI;
    let denom = denom_bands();
    let mut pts = Vec::with_capacity(NUM_BANDS * 2);
    let mut bases = Vec::with_capacity(NUM_BANDS * 2);

    let mut add = |ang: f32, h: f32| {
        let r = RADIAL_INNER + h.clamp(0.0, 1.0) * 0.62;
        let (s, c) = ang.sin_cos();
        pts.push((c * r * inv_aspect, s * r));
        bases.push((c * RADIAL_INNER * inv_aspect, s * RADIAL_INNER));
    };
    // Semicerchio destro: basse in alto (90°) → alte in basso (-90°).
    for i in 0..NUM_BANDS {
        let t = i as f32 / denom;
        add(std::f32::consts::FRAC_PI_2 - t * pi, right[i]);
    }
    // Semicerchio sinistro: dal basso (270°) torna su (90°) chiudendo l'anello.
    for i in (0..NUM_BANDS).rev() {
        let t = i as f32 / denom;
        add(std::f32::consts::FRAC_PI_2 + t * pi, left[i]);
    }
    let vis = peak_level(left, right);
    (
        build_fill(&pts, &bases, palette, true),
        build_neon(&pts, palette, true, vis),
        build_neon_line(&pts, palette, true, vis),
    )
}

/// Anello radiale speculare (TRIANGLES), due soli grafici: semicerchio destro
/// = canale `right`, semicerchio sinistro = canale `left`. Le basse frequenze
/// partono dall'alto (90°) e scendono verso le alte in basso (una spazzata di
/// 180° per canale). I raggi partono trasparenti all'interno e diventano opachi
/// verso la punta.
fn build_radial_spokes(
    left: &SpectrumFrame,
    right: &SpectrumFrame,
    palette: &Palette,
    inv_aspect: f32,
) -> Vec<f32> {
    let mut v = Vec::with_capacity(NUM_BANDS * 2 * 6 * VERT_FLOATS);
    let ca = palette.color_a;
    let cb = palette.color_b;
    let pi = std::f32::consts::PI;
    let half_w = (pi / NUM_BANDS as f32) * 0.40;

    let push = |v: &mut Vec<f32>, x: f32, y: f32, c: Rgb, a: f32| {
        v.extend_from_slice(&[x, y, c.r, c.g, c.b, a]);
    };
    let spoke = |v: &mut Vec<f32>, ang: f32, h: f32| {
        let r0 = RADIAL_INNER;
        let r1 = RADIAL_INNER + h.clamp(0.0, 1.0) * 0.62;
        let pt = |a: f32, r: f32| -> (f32, f32) {
            let (s, c) = a.sin_cos();
            (c * r * inv_aspect, s * r)
        };
        let (ilx, ily) = pt(ang - half_w, r0);
        let (irx, iry) = pt(ang + half_w, r0);
        let (olx, oly) = pt(ang - half_w, r1);
        let (orx, ory) = pt(ang + half_w, r1);
        // Interno (base) trasparente, punta opaca.
        push(v, ilx, ily, ca, 0.0);
        push(v, irx, iry, ca, 0.0);
        push(v, orx, ory, cb, 1.0);
        push(v, ilx, ily, ca, 0.0);
        push(v, orx, ory, cb, 1.0);
        push(v, olx, oly, cb, 1.0);
    };

    for i in 0..NUM_BANDS {
        let t = i as f32 / denom_bands();
        spoke(&mut v, std::f32::consts::FRAC_PI_2 - t * pi, right[i]);
        spoke(&mut v, std::f32::consts::FRAC_PI_2 + t * pi, left[i]);
    }
    v
}

/// Denominatore per la rampa di frequenza nel radiale (evita /0).
fn denom_bands() -> f32 {
    (NUM_BANDS as f32 - 1.0).max(1.0)
}

/// Vertici delle particelle (POINTS), dissolvenza tramite l'alpha (= life).
fn build_particle_vertices(particles: &[Particle], palette: &Palette, inv_aspect: f32) -> Vec<f32> {
    let mut v = Vec::with_capacity(particles.len() * VERT_FLOATS);
    for p in particles {
        let c = mix(palette.color_a, palette.color_b, p.t);
        let life = p.life.clamp(0.0, 1.0);
        v.extend_from_slice(&[p.x * inv_aspect, p.y, c.r, c.g, c.b, life]);
    }
    v
}

unsafe fn link_program(
    gl: &glow::Context,
    vertex_src: &str,
    fragment_src: &str,
) -> anyhow::Result<glow::Program> {
    let program = gl
        .create_program()
        .map_err(|e| anyhow::anyhow!("create_program: {e}"))?;

    let shaders = [
        (glow::VERTEX_SHADER, vertex_src),
        (glow::FRAGMENT_SHADER, fragment_src),
    ];
    let mut compiled = Vec::new();
    for (kind, src) in shaders {
        let shader = gl
            .create_shader(kind)
            .map_err(|e| anyhow::anyhow!("create_shader: {e}"))?;
        gl.shader_source(shader, src);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let log = gl.get_shader_info_log(shader);
            anyhow::bail!("compilazione shader fallita: {log}");
        }
        gl.attach_shader(program, shader);
        compiled.push(shader);
    }

    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        anyhow::bail!("link program fallito: {log}");
    }

    for shader in compiled {
        gl.detach_shader(program, shader);
        gl.delete_shader(shader);
    }

    Ok(program)
}
