//! Rendering OpenGL del visualizzatore tramite `GtkGLArea` + `glow`.
//!
//! Effetti supportati (selezionabili a runtime): barre stile Cava, curva dello
//! spettro (area riempita), radiale con particelle, le varianti "neon" e il
//! tunnel. Condividono lo stesso formato di vertice (posizione + colore RGBA)
//! e variano vertici, primitiva e programma shader.
//!
//! Due rifiniture: le barre/linea partono trasparenti alla base e diventano
//! opache verso l'alto (alpha per vertice + blending); un parametro `blur`
//! controlla il motion blur disegnando un velo nero semi-trasparente al posto
//! del clear (le scie persistono e sfumano).

use crate::audio::{AudioBuffer, Channel};
use crate::config::{Effect, Rgb};
use crate::dsp::{Analyzer, ImagingAnalyzer, ImagingFrame, SpectrumFrame, NUM_BANDS};
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
    /// Immagine stereo, calcolata solo quando l'effetto Imaging è attivo
    /// (richiede due FFT extra per frame).
    pub imaging: ImagingFrame,
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
            imaging: ImagingFrame::default(),
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
                    &st.imaging,
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
    let analyzer_img = Rc::new(RefCell::new(ImagingAnalyzer::new()));
    area.add_tick_callback({
        let state = state.clone();
        move |area, _clock| {
            let (gain, mirror, effect) = {
                let s = state.borrow();
                (s.gain, s.mirror, s.effect)
            };
            let left = analyzer_l.borrow_mut().analyze(&audio, Channel::Left, gain);
            // In input lo stesso canale è specchiato sui due lati.
            let right = if mirror {
                left
            } else {
                analyzer_r.borrow_mut().analyze(&audio, Channel::Right, gain)
            };
            // Le due FFT dell'imaging costano, quindi solo quando serve.
            let imaging = (effect == Effect::Imaging)
                .then(|| analyzer_img.borrow_mut().analyze(&audio, gain));
            {
                let mut s = state.borrow_mut();
                s.spectrum_left = left;
                s.spectrum_right = right;
                if let Some(img) = imaging {
                    s.imaging = img;
                }
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

// Shader del tunnel: come il neon (l'alpha del vertice porta la coordinata
// perpendicolare al ribbon, -1..1) ma con un nucleo netto e brillante oltre
// all'alone, così ogni anello sembra un tubo di luce senza bisogno di una
// LINE_STRIP separata. La luminosità dell'anello è già premoltiplicata nel
// colore, quindi il blending additivo fa da solo la dissolvenza.
const TUNNEL_FRAGMENT_SRC: &str = r#"#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 frag;
void main() {
    float d = abs(v_color.a);           // 0 al centro del ribbon, 1 ai bordi
    float core = smoothstep(0.30, 0.0, d);
    float halo = 1.0 - d;
    halo = halo * halo * halo * 0.45;
    frag = vec4(v_color.rgb * (core + halo), 1.0);
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

/// Un anello del tunnel: la sagoma dello spettro viene "congelata" alla
/// nascita e da lì l'anello si limita a espandersi e ruotare, così il tunnel
/// mostra la storia recente della traccia come una serie di sezioni.
struct Ring {
    /// Sagoma a raggio ~1 (spazio quadrato), già smussata alla nascita.
    shape: Vec<(f32, f32)>,
    /// Fattore di scala: cresce in modo esponenziale (prospettiva).
    scale: f32,
    /// Rotazione accumulata: gli anelli vecchi hanno ruotato di più → vortice.
    angle: f32,
    /// Posizione nel gradiente A→B (0..1), fissata alla nascita.
    tint: f32,
}

/// Una stella del campo che scorre dal centro verso i bordi nel tunnel.
struct Star {
    x: f32,
    y: f32,
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
    /// Programma per gli anelli del tunnel (ribbon additivo con nucleo netto).
    tunnel_program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    pos_loc: u32,
    col_loc: u32,
    glow_pos_loc: u32,
    glow_col_loc: u32,
    neon_pos_loc: u32,
    neon_col_loc: u32,
    tunnel_pos_loc: u32,
    tunnel_col_loc: u32,
    particles: Vec<Particle>,
    /// Anelli e stelle dell'effetto tunnel.
    rings: Vec<Ring>,
    stars: Vec<Star>,
    /// Accumulatore per l'emissione di un nuovo anello (spawn a ogni 1.0).
    ring_emit: f32,
    /// Geometria statica del poliedro e stato della sua animazione.
    solid: SolidMesh,
    /// Estrusione corrente di ogni faccia (calciata dai transienti).
    face_pop: Vec<f32>,
    solid_spin: f32,
    solid_time: f32,
    /// Inviluppo lento dei bassi: serve a isolare i transienti (un colpo di
    /// cassa è ciò che supera la media, non il livello assoluto — che con la
    /// musica resta quasi costante e non darebbe alcun contrasto).
    bass_env: f32,
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
            // Il tunnel riusa il vertex shader del neon (passa colore e alpha).
            let tunnel_program = link_program(&gl, NEON_VERTEX_SRC, TUNNEL_FRAGMENT_SRC)?;
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
            let tunnel_pos_loc = gl
                .get_attrib_location(tunnel_program, "position")
                .unwrap_or(0);
            let tunnel_col_loc = gl.get_attrib_location(tunnel_program, "color").unwrap_or(1);
            let solid = build_icosphere(SOLID_SUBDIV);
            let face_pop = vec![0.0; solid.faces.len()];
            Ok(Self {
                gl,
                program,
                glow_program,
                neon_program,
                tunnel_program,
                vao,
                vbo,
                pos_loc,
                col_loc,
                glow_pos_loc,
                glow_col_loc,
                neon_pos_loc,
                neon_col_loc,
                tunnel_pos_loc,
                tunnel_col_loc,
                particles: Vec::new(),
                rings: Vec::new(),
                stars: Vec::new(),
                ring_emit: 1.0,
                solid,
                face_pop,
                solid_spin: 0.0,
                solid_time: 0.0,
                bass_env: 0.0,
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
        imaging: &ImagingFrame,
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
            Effect::Tunnel => {
                let inv_aspect = height as f32 / width as f32;
                self.update_tunnel(left, right);

                // Stelle sotto gli anelli: entrambi additivi, l'ordine conta poco
                // ma così i tubi di luce restano in primo piano.
                let stars = build_star_vertices(&self.stars, palette, inv_aspect);
                unsafe {
                    self.gl.use_program(Some(self.glow_program));
                    self.gl.blend_func(glow::ONE, glow::ONE);
                }
                self.draw_arrays(&stars, glow::POINTS, self.glow_pos_loc, self.glow_col_loc);

                let rings = build_tunnel_rings(&self.rings, palette, inv_aspect);
                unsafe {
                    self.gl.use_program(Some(self.tunnel_program));
                    self.gl.blend_func(glow::ONE, glow::ONE);
                }
                self.draw_arrays(
                    &rings,
                    glow::TRIANGLE_STRIP,
                    self.tunnel_pos_loc,
                    self.tunnel_col_loc,
                );
            }
            Effect::Solid => {
                let inv_aspect = height as f32 / width as f32;
                let (yaw, pitch, dist) = self.update_solid(left, right);
                let vs = project_solid(&self.solid, left, right, yaw, pitch, dist);
                // Riferimento prospettico al centro: normalizza il fog a 1.
                let s0 = SOLID_FOCAL / SOLID_DIST;

                // Tutto additivo: il wireframe è order-independent, quindi non
                // serve alcun depth buffer (la GLArea non ne ha).
                unsafe {
                    self.gl.use_program(Some(self.program));
                    self.gl.blend_func(glow::ONE, glow::ONE);
                }
                let faces = build_solid_faces(
                    &self.solid,
                    &vs,
                    &self.face_pop,
                    palette,
                    inv_aspect,
                    s0,
                    dist,
                );
                self.draw_arrays(&faces, glow::TRIANGLES, self.pos_loc, self.col_loc);

                let edges = build_solid_edges(&self.solid, &vs, palette, inv_aspect, s0);
                unsafe {
                    self.gl.use_program(Some(self.tunnel_program));
                }
                self.draw_arrays(
                    &edges,
                    glow::TRIANGLE_STRIP,
                    self.tunnel_pos_loc,
                    self.tunnel_col_loc,
                );

                let pts = build_solid_points(&self.solid, &vs, palette, inv_aspect, s0);
                unsafe {
                    self.gl.use_program(Some(self.glow_program));
                }
                self.draw_arrays(&pts, glow::POINTS, self.glow_pos_loc, self.glow_col_loc);
            }
            Effect::Imaging => {
                let inv_aspect = height as f32 / width as f32;
                let (fill, ribbons, dots) = build_imaging(imaging, palette, inv_aspect);

                unsafe {
                    self.gl.use_program(Some(self.program));
                    self.gl.blend_func(glow::ONE, glow::ONE);
                }
                self.draw_arrays(&fill, glow::TRIANGLE_STRIP, self.pos_loc, self.col_loc);

                unsafe {
                    self.gl.use_program(Some(self.tunnel_program));
                }
                self.draw_arrays(
                    &ribbons,
                    glow::TRIANGLE_STRIP,
                    self.tunnel_pos_loc,
                    self.tunnel_col_loc,
                );

                unsafe {
                    self.gl.use_program(Some(self.glow_program));
                }
                self.draw_arrays(&dots, glow::POINTS, self.glow_pos_loc, self.glow_col_loc);
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

    /// Driver audio condivisi dagli effetti dinamici: livello dei bassi,
    /// transiente e gate di silenzio.
    ///
    /// Il `punch` è quanto i bassi superano la propria media lenta: il livello
    /// assoluto con la musica resta quasi costante e da solo non darebbe alcun
    /// contrasto, mentre il colpo di cassa emerge sempre sull'inviluppo.
    fn audio_drive(&mut self, left: &SpectrumFrame, right: &SpectrumFrame) -> (f32, f32, f32) {
        let bass = bass_level(left, right);
        let gate = silence_gate(peak_level(left, right));
        self.bass_env += (bass - self.bass_env) * 0.05;
        let punch = ((bass - self.bass_env) * 2.5).clamp(0.0, 1.0);
        (bass, punch, gate)
    }

    /// Aggiorna il tunnel: gli anelli esistenti si espandono e ruotano, ogni
    /// tanto ne nasce uno nuovo con la sagoma dello spettro corrente, e il
    /// campo di stelle scorre dal centro verso i bordi. Le basse frequenze
    /// accelerano espansione, rotazione ed emissione: sui colpi di cassa il
    /// tunnel "sfreccia".
    fn update_tunnel(&mut self, left: &SpectrumFrame, right: &SpectrumFrame) {
        const MAX_RINGS: usize = 26;
        const MAX_STARS: usize = 700;

        // Gate di silenzio: senza segnale il tunnel non emette nulla e si
        // limita a svuotare lo schermo (deriva minima), invece di correre a
        // vuoto.
        let (bass, punch, gate) = self.audio_drive(left, right);

        // Espansione esponenziale = prospettiva; ogni anello accumula la
        // propria rotazione, quindi i più vecchi sono più ruotati → vortice.
        // La deriva di base serve solo a far uscire di scena gli anelli
        // rimasti quando la musica si ferma.
        let growth = 1.0 + 0.006 + (0.015 + bass * 0.030 + punch * 0.15) * gate;
        let spin = (0.005 + bass * 0.016 + punch * 0.06) * gate;
        for r in &mut self.rings {
            r.scale *= growth;
            r.angle += spin;
        }
        self.rings.retain(|r| r.scale < RING_DEATH);

        // Emissione di un nuovo anello (sagoma congelata dello spettro),
        // interamente pilotata dall'audio: a silenzio non ne nasce nessuno.
        self.ring_emit += (0.075 + bass * 0.10 + punch * 0.35) * gate;
        if self.ring_emit >= 1.0 {
            self.ring_emit = 0.0;
            if self.rings.len() < MAX_RINGS {
                self.rings.push(Ring {
                    shape: build_ring_shape(left, right),
                    scale: RING_BIRTH,
                    angle: 0.0,
                    // Il colore registra la botta alla nascita: guardando in
                    // fondo al tunnel si legge la dinamica del brano.
                    tint: (bass + punch * 0.5).clamp(0.0, 1.0),
                });
            }
        }

        // Stelle: scalatura radiale della posizione = moto accelerato verso
        // i bordi (effetto warp).
        let star_growth = 1.0 + 0.004 + (0.026 + bass * 0.055 + punch * 0.10) * gate;
        for s in &mut self.stars {
            s.x *= star_growth;
            s.y *= star_growth;
        }
        self.stars.retain(|s| s.x.abs() < 1.7 && s.y.abs() < 1.7);

        let spawn = ((2.0 + bass * 6.0 + punch * 4.0) * gate) as usize;
        for _ in 0..spawn {
            if self.stars.len() >= MAX_STARS {
                break;
            }
            let ang = self.rand() * std::f32::consts::TAU;
            let rad = 0.04 + self.rand() * 0.05;
            let tint = self.rand();
            let (s, c) = ang.sin_cos();
            self.stars.push(Star {
                x: c * rad,
                y: s * rad,
                t: tint,
            });
        }
    }

    /// Aggiorna l'animazione del poliedro e ritorna (yaw, pitch, distanza).
    ///
    /// La musica accorcia la distanza dalla camera invece di scalare il
    /// solido: cambiando la divisione prospettica si ottiene parallasse vera
    /// ("viene verso di te"), mentre scalare sarebbe solo uno zoom piatto.
    fn update_solid(&mut self, left: &SpectrumFrame, right: &SpectrumFrame) -> (f32, f32, f32) {
        let (bass, punch, gate) = self.audio_drive(left, right);

        self.solid_time += 1.0 / 60.0;
        self.solid_spin += 0.002 + (bass * 0.012 + punch * 0.05) * gate;
        let pitch = (self.solid_time * 0.31).sin() * 0.35;
        let dist = SOLID_DIST - (bass * 0.5 + punch * 0.7) * gate;

        // Ogni faccia riceve un calcio proporzionale al transiente e alla
        // propria banda, poi rientra.
        for (i, pop) in self.face_pop.iter_mut().enumerate() {
            let sp = if self.solid.face_right[i] { right } else { left };
            let amp = sample_spectrum(sp, self.solid.face_t[i]);
            *pop = (*pop * 0.88 + punch * amp * 0.30 * gate).min(0.5);
        }

        (self.solid_spin, pitch, dist)
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_program(self.glow_program);
            self.gl.delete_program(self.neon_program);
            self.gl.delete_program(self.tunnel_program);
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

/// Suddivisioni per segmento nello smoothing spline (look più morbido).
const SMOOTH_SUBDIV: usize = 8;

/// Punto di una spline di Catmull-Rom tra `p1` e `p2` (controlli `p0`,`p3`).
fn catmull_rom(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32), t: f32) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let f = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * ((2.0 * b)
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1))
}

/// Interpola una polilinea con spline di Catmull-Rom, restituendo punti
/// suddivisi (curva morbida). `closed` tratta la curva come anello chiuso.
fn smooth_curve(pts: &[(f32, f32)], closed: bool) -> Vec<(f32, f32)> {
    smooth_curve_subdiv(pts, closed, SMOOTH_SUBDIV)
}

/// Come [`smooth_curve`] ma con suddivisione configurabile (il tunnel disegna
/// molte curve per frame e si accontenta di meno punti).
fn smooth_curve_subdiv(pts: &[(f32, f32)], closed: bool, subdiv: usize) -> Vec<(f32, f32)> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let get = |i: isize| -> (f32, f32) {
        if closed {
            pts[(i.rem_euclid(n as isize)) as usize]
        } else {
            pts[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let seg_count = if closed { n } else { n - 1 };
    let mut out = Vec::with_capacity(seg_count * subdiv + 1);
    for i in 0..seg_count {
        let p0 = get(i as isize - 1);
        let p1 = get(i as isize);
        let p2 = get(i as isize + 1);
        let p3 = get(i as isize + 2);
        for s in 0..subdiv {
            let t = s as f32 / subdiv as f32;
            out.push(catmull_rom(p0, p1, p2, p3, t));
        }
    }
    if !closed {
        out.push(pts[n - 1]);
    }
    out
}

/// Punti della curva speculare della linea: (x, y_cima), centro = basse.
fn line_curve_points(left: &SpectrumFrame, right: &SpectrumFrame) -> Vec<(f32, f32)> {
    let denom = (NUM_BANDS as f32 - 1.0).max(1.0);
    let mut pts = Vec::with_capacity(NUM_BANDS * 2);
    for i in (0..NUM_BANDS).rev() {
        pts.push((-(i as f32 / denom), -1.0 + 2.0 * left[i].clamp(0.0, 1.0)));
    }
    for i in 0..NUM_BANDS {
        pts.push((i as f32 / denom, -1.0 + 2.0 * right[i].clamp(0.0, 1.0)));
    }
    pts
}

/// Curva continua e speculare dello spettro come area riempita (TRIANGLE_STRIP),
/// con smoothing spline per un profilo morbido. Base trasparente, cima opaca.
fn build_line_vertices(left: &SpectrumFrame, right: &SpectrumFrame, palette: &Palette) -> Vec<f32> {
    let ca = palette.color_a;
    let cb = palette.color_b;
    let pts = smooth_curve(&line_curve_points(left, right), false);
    let mut v = Vec::with_capacity(pts.len() * 2 * VERT_FLOATS);
    for (x, y) in pts {
        let y_t = y.max(-1.0);
        v.extend_from_slice(&[x, -1.0, ca.r, ca.g, ca.b, 0.0]);
        v.extend_from_slice(&[x, y_t, cb.r, cb.g, cb.b, 1.0]);
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
    let pts = smooth_curve(&line_curve_points(left, right), false);
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
    let mut raw = Vec::with_capacity(NUM_BANDS * 2);

    let mut add = |ang: f32, h: f32| {
        let r = RADIAL_INNER + h.clamp(0.0, 1.0) * 0.62;
        let (s, c) = ang.sin_cos();
        raw.push((c * r * inv_aspect, s * r));
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

    let pts = smooth_curve(&raw, true);
    // Base = proiezione di ciascun punto sull'anello interno (direzione radiale).
    let bases: Vec<(f32, f32)> = pts
        .iter()
        .map(|&(px, py)| {
            let sx = px / inv_aspect;
            let len = (sx * sx + py * py).sqrt().max(1e-6);
            (sx / len * RADIAL_INNER * inv_aspect, py / len * RADIAL_INNER)
        })
        .collect();
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

// ---------------------------------------------------------------------------
// Effetto tunnel
// ---------------------------------------------------------------------------

/// Scala di un anello appena nato (praticamente il punto di fuga).
const RING_BIRTH: f32 = 0.05;
/// Scala oltre la quale l'anello è fuori schermo e viene eliminato.
const RING_DEATH: f32 = 1.85;
/// Una banda ogni `RING_STEP` entra nella sagoma (curve più leggere).
const RING_STEP: usize = 2;
/// Suddivisione della spline per gli anelli del tunnel.
const RING_SUBDIV: usize = 4;
/// Semi-larghezza base del tubo di luce, in spazio quadrato.
const RING_HALF_WIDTH: f32 = 0.012;

/// Livello delle basse frequenze (media delle prime bande dei due canali, in
/// pratica 30–95 Hz), usato per pilotare velocità, rotazione ed emissione.
fn bass_level(left: &SpectrumFrame, right: &SpectrumFrame) -> f32 {
    const N: usize = 12;
    let mut sum = 0.0;
    for i in 0..N.min(NUM_BANDS) {
        sum += left[i] + right[i];
    }
    (sum / (2.0 * N as f32)).clamp(0.0, 1.0)
}

/// Gate di silenzio: 0 quando non c'è segnale, 1 appena la traccia suona.
/// Sotto la soglia il tunnel smette di emettere anelli e stelle.
fn silence_gate(level: f32) -> f32 {
    ((level - 0.05) / 0.12).clamp(0.0, 1.0)
}

/// Sagoma di un nuovo anello: cerchio di raggio ~1 deformato dallo spettro
/// (semicerchio destro = `right`, sinistro = `left`, basse in alto), smussato
/// con una spline chiusa.
fn build_ring_shape(left: &SpectrumFrame, right: &SpectrumFrame) -> Vec<(f32, f32)> {
    let pi = std::f32::consts::PI;
    let denom = denom_bands();
    let mut raw = Vec::with_capacity(2 * NUM_BANDS / RING_STEP + 2);

    let mut add = |ang: f32, h: f32| {
        let r = 0.72 + h.clamp(0.0, 1.0) * 0.55;
        let (s, c) = ang.sin_cos();
        raw.push((c * r, s * r));
    };
    for i in (0..NUM_BANDS).step_by(RING_STEP) {
        let t = i as f32 / denom;
        add(std::f32::consts::FRAC_PI_2 - t * pi, right[i]);
    }
    for i in (0..NUM_BANDS).step_by(RING_STEP).rev() {
        let t = i as f32 / denom;
        add(std::f32::consts::FRAC_PI_2 + t * pi, left[i]);
    }
    smooth_curve_subdiv(&raw, true, RING_SUBDIV)
}

/// Luminosità di un anello in funzione della scala: appare dal punto di fuga,
/// resta pieno per la traversata e sfuma uscendo dallo schermo.
fn ring_brightness(scale: f32) -> f32 {
    let fade_in = (scale / 0.28).clamp(0.0, 1.0);
    let fade_out = ((RING_DEATH - scale) / 0.75).clamp(0.0, 1.0);
    fade_in * fade_out
}

/// Vertici di tutti gli anelli in un unico TRIANGLE_STRIP: ogni anello è un
/// ribbon (l'alpha porta la coordinata perpendicolare, letta dallo shader del
/// tunnel) e gli anelli sono cuciti tra loro con triangoli degeneri, così
/// basta una sola draw call. La luminosità è premoltiplicata nel colore
/// perché il blending è additivo.
fn build_tunnel_rings(rings: &[Ring], palette: &Palette, inv_aspect: f32) -> Vec<f32> {
    let mut v: Vec<f32> = Vec::new();
    let mut ring_v: Vec<f32> = Vec::new();

    for r in rings {
        let n = r.shape.len();
        let bright = ring_brightness(r.scale);
        if n < 3 || bright <= 0.002 {
            continue;
        }
        let c = mix(palette.color_a, palette.color_b, r.tint.clamp(0.0, 1.0));
        let (cr, cg, cb) = (c.r * bright, c.g * bright, c.b * bright);
        let (sa, ca) = r.angle.sin_cos();
        // Il tubo si ispessisce avvicinandosi (prospettiva).
        let w = RING_HALF_WIDTH * (0.35 + r.scale);
        let pt = |i: usize| -> (f32, f32) {
            let (x, y) = r.shape[i % n];
            ((x * ca - y * sa) * r.scale, (x * sa + y * ca) * r.scale)
        };

        ring_v.clear();
        for k in 0..=n {
            let i = k % n;
            let prev = pt((i + n - 1) % n);
            let next = pt(i + 1);
            let (px, py) = pt(i);
            let tx = next.0 - prev.0;
            let ty = next.1 - prev.1;
            let len = (tx * tx + ty * ty).sqrt().max(1e-6);
            let (nx, ny) = (-ty / len * w, tx / len * w);
            ring_v.extend_from_slice(&[(px + nx) * inv_aspect, py + ny, cr, cg, cb, 1.0]);
            ring_v.extend_from_slice(&[(px - nx) * inv_aspect, py - ny, cr, cg, cb, -1.0]);
        }

        // Cucitura con triangoli degeneri (area nulla → nessun frammento).
        if !v.is_empty() {
            let last: Vec<f32> = v[v.len() - VERT_FLOATS..].to_vec();
            v.extend_from_slice(&last);
            v.extend_from_slice(&ring_v[..VERT_FLOATS]);
        }
        v.extend_from_slice(&ring_v);
    }
    v
}

/// Vertici delle stelle del tunnel (POINTS): fioche vicino al punto di fuga,
/// piene a metà corsa, in dissolvenza ai bordi.
fn build_star_vertices(stars: &[Star], palette: &Palette, inv_aspect: f32) -> Vec<f32> {
    let mut v = Vec::with_capacity(stars.len() * VERT_FLOATS);
    for s in stars {
        let r = (s.x * s.x + s.y * s.y).sqrt();
        let a = (r / 0.35).clamp(0.0, 1.0) * ((1.6 - r) / 0.5).clamp(0.0, 1.0);
        let c = mix(palette.color_a, palette.color_b, s.t);
        v.extend_from_slice(&[s.x * inv_aspect, s.y, c.r, c.g, c.b, a]);
    }
    v
}

// ---------------------------------------------------------------------------
// Effetto poliedro 3D
// ---------------------------------------------------------------------------

/// Suddivisioni dell'icosaedro di base (2 → 162 vertici, 480 spigoli, 320 facce).
const SOLID_SUBDIV: usize = 2;
/// Raggio a riposo del solido.
const SOLID_R0: f32 = 0.56;
/// Spiazzamento radiale massimo dovuto allo spettro.
const SOLID_AMP: f32 = 0.40;
/// Lunghezza focale della proiezione prospettica.
const SOLID_FOCAL: f32 = 2.2;
/// Distanza a riposo del solido dalla camera (i bassi la accorciano: dolly).
const SOLID_DIST: f32 = 3.4;
/// Semi-spessore degli spigoli a distanza di riposo, in spazio quadrato.
const SOLID_EDGE_W: f32 = 0.009;
/// Intensità del bagliore delle facce (0 = solo wireframe).
const SOLID_FACE_GAIN: f32 = 0.11;

/// Geometria statica del solido: icosaedro geodetico sulla sfera unitaria.
///
/// Il mapping delle frequenze è precalcolato per vertice ed è il radiale
/// rivoluzionato in 3D: la latitudine dà la frequenza (polo nord = basse, polo
/// sud = alte) e l'emisfero dà il canale (x ≥ 0 = destro).
struct SolidMesh {
    verts: Vec<[f32; 3]>,
    /// Posizione nello spettro (0..1) di ogni vertice, dalla latitudine.
    vert_t: Vec<f32>,
    /// Canale del vertice: true = destro.
    vert_right: Vec<bool>,
    edges: Vec<(u32, u32)>,
    faces: Vec<[u32; 3]>,
    /// Posizione nello spettro e canale di ogni faccia (media dei vertici).
    face_t: Vec<f32>,
    face_right: Vec<bool>,
}

/// Un vertice del solido dopo spiazzamento, rotazione e proiezione.
struct SolidVert {
    /// Posizione 3D ruotata (serve per le normali delle facce).
    p: [f32; 3],
    /// Proiezione in spazio quadrato (inv_aspect applicato solo all'emissione).
    sx: f32,
    sy: f32,
    /// Fattore prospettico: grande = vicino.
    s: f32,
    /// Ampiezza spettrale campionata al vertice.
    amp: f32,
}

fn normalized(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / len, v[1] / len, v[2] / len]
}

/// Indice del punto medio (normalizzato sulla sfera) tra due vertici,
/// riusando quello già creato per lo spigolo condiviso.
fn midpoint(
    verts: &mut Vec<[f32; 3]>,
    cache: &mut std::collections::HashMap<(u32, u32), u32>,
    a: u32,
    b: u32,
) -> u32 {
    let key = (a.min(b), a.max(b));
    if let Some(&i) = cache.get(&key) {
        return i;
    }
    let (pa, pb) = (verts[a as usize], verts[b as usize]);
    let m = normalized([
        (pa[0] + pb[0]) * 0.5,
        (pa[1] + pb[1]) * 0.5,
        (pa[2] + pb[2]) * 0.5,
    ]);
    let i = verts.len() as u32;
    verts.push(m);
    cache.insert(key, i);
    i
}

/// Costruisce l'icosaedro geodetico e precalcola spigoli e mapping spettrale.
fn build_icosphere(subdiv: usize) -> SolidMesh {
    let phi = (1.0 + 5f32.sqrt()) / 2.0;
    let mut verts: Vec<[f32; 3]> = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ]
    .into_iter()
    .map(normalized)
    .collect();

    let mut faces: Vec<[u32; 3]> = vec![
        [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
        [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
        [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
    ];

    for _ in 0..subdiv {
        let mut cache = std::collections::HashMap::new();
        let mut next = Vec::with_capacity(faces.len() * 4);
        for f in &faces {
            let a = midpoint(&mut verts, &mut cache, f[0], f[1]);
            let b = midpoint(&mut verts, &mut cache, f[1], f[2]);
            let c = midpoint(&mut verts, &mut cache, f[2], f[0]);
            next.push([f[0], a, c]);
            next.push([f[1], b, a]);
            next.push([f[2], c, b]);
            next.push([a, b, c]);
        }
        faces = next;
    }

    // Spigoli deduplicati (ogni spigolo è condiviso da due facce).
    let mut seen = std::collections::HashSet::new();
    let mut edges = Vec::new();
    for f in &faces {
        for (i, j) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
            let key = (i.min(j), i.max(j));
            if seen.insert(key) {
                edges.push(key);
            }
        }
    }

    // Latitudine → frequenza, emisfero → canale.
    let vert_t: Vec<f32> = verts
        .iter()
        .map(|v| v[1].clamp(-1.0, 1.0).acos() / std::f32::consts::PI)
        .collect();
    let vert_right: Vec<bool> = verts.iter().map(|v| v[0] >= 0.0).collect();

    let face_t: Vec<f32> = faces
        .iter()
        .map(|f| (vert_t[f[0] as usize] + vert_t[f[1] as usize] + vert_t[f[2] as usize]) / 3.0)
        .collect();
    let face_right: Vec<bool> = faces
        .iter()
        .map(|f| {
            (verts[f[0] as usize][0] + verts[f[1] as usize][0] + verts[f[2] as usize][0]) >= 0.0
        })
        .collect();

    SolidMesh {
        verts,
        vert_t,
        vert_right,
        edges,
        faces,
        face_t,
        face_right,
    }
}

/// Campiona lo spettro a una posizione continua (0..1) interpolando tra bande:
/// le latitudini della mesh sono poche, l'interpolazione evita la scalinatura.
fn sample_spectrum(sp: &SpectrumFrame, t: f32) -> f32 {
    let f = t.clamp(0.0, 1.0) * (NUM_BANDS - 1) as f32;
    let i = f.floor() as usize;
    let j = (i + 1).min(NUM_BANDS - 1);
    sp[i] + (sp[j] - sp[i]) * (f - i as f32)
}

/// Rotazione yaw (attorno a Y) seguita da pitch (attorno a X).
fn rot_yx(p: [f32; 3], yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let x1 = p[0] * cy + p[2] * sy;
    let z1 = -p[0] * sy + p[2] * cy;
    let (sp, cp) = pitch.sin_cos();
    [x1, p[1] * cp - z1 * sp, p[1] * sp + z1 * cp]
}

/// Fattore prospettico a distanza `dist` dalla camera per un punto a quota `z`.
fn perspective(dist: f32, z: f32) -> f32 {
    SOLID_FOCAL / (dist + z).max(0.4)
}

/// Spiazza i vertici lungo il raggio secondo lo spettro, ruota il solido e lo
/// proietta in prospettiva.
fn project_solid(
    mesh: &SolidMesh,
    left: &SpectrumFrame,
    right: &SpectrumFrame,
    yaw: f32,
    pitch: f32,
    dist: f32,
) -> Vec<SolidVert> {
    mesh.verts
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let sp = if mesh.vert_right[i] { right } else { left };
            let amp = sample_spectrum(sp, mesh.vert_t[i]);
            let r = SOLID_R0 + amp * SOLID_AMP;
            let p = rot_yx([v[0] * r, v[1] * r, v[2] * r], yaw, pitch);
            let s = perspective(dist, p[2]);
            SolidVert {
                p,
                sx: p[0] * s,
                sy: p[1] * s,
                s,
                amp,
            }
        })
        .collect()
}

/// Attenuazione con la profondità. Senza z-buffer e con blending additivo gli
/// spigoli davanti e dietro sarebbero identici e il volume illeggibile: questo
/// fall-off è ciò che fa percepire la forma come solida.
///
/// Esponente 3 e tetto basso: serve separazione netta tra fronte e retro, e
/// impedire che la mesh densa al centro saturi a bianco sommandosi.
fn depth_fog(s: f32, s0: f32) -> f32 {
    let k = s / s0;
    (k * k * k).clamp(0.0, 1.25)
}

/// Spigoli come ribbon luminosi in screen-space, cuciti in un unico
/// TRIANGLE_STRIP con triangoli degeneri (una sola draw call).
fn build_solid_edges(
    mesh: &SolidMesh,
    vs: &[SolidVert],
    palette: &Palette,
    inv_aspect: f32,
    s0: f32,
) -> Vec<f32> {
    let mut v: Vec<f32> = Vec::with_capacity(mesh.edges.len() * 6 * VERT_FLOATS);
    let mut quad: Vec<f32> = Vec::with_capacity(4 * VERT_FLOATS);

    for &(ia, ib) in &mesh.edges {
        let (a, b) = (&vs[ia as usize], &vs[ib as usize]);
        let s_avg = (a.s + b.s) * 0.5;
        let fog = depth_fog(s_avg, s0);
        let amp = (a.amp + b.amp) * 0.5;
        let bright = fog * (0.18 + amp * 0.55);
        let t_col = (mesh.vert_t[ia as usize] + mesh.vert_t[ib as usize]) * 0.5;
        let c = mix(palette.color_a, palette.color_b, t_col);
        let (cr, cg, cb) = (c.r * bright, c.g * bright, c.b * bright);

        // Spessore prospettico: gli spigoli vicini sono più grossi.
        let w = SOLID_EDGE_W * (s_avg / s0);
        let (dx, dy) = (b.sx - a.sx, b.sy - a.sy);
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (nx, ny) = (-dy / len * w, dx / len * w);

        quad.clear();
        for (px, py, perp) in [
            (a.sx + nx, a.sy + ny, 1.0),
            (a.sx - nx, a.sy - ny, -1.0),
            (b.sx + nx, b.sy + ny, 1.0),
            (b.sx - nx, b.sy - ny, -1.0),
        ] {
            quad.extend_from_slice(&[px * inv_aspect, py, cr, cg, cb, perp]);
        }

        if !v.is_empty() {
            let last: Vec<f32> = v[v.len() - VERT_FLOATS..].to_vec();
            v.extend_from_slice(&last);
            v.extend_from_slice(&quad[..VERT_FLOATS]);
        }
        v.extend_from_slice(&quad);
    }
    v
}

/// Facce come triangoli additivi: brillano in proporzione all'ampiezza della
/// loro banda e si estrudono lungo la normale quando arriva un transiente,
/// staccandosi dalla gabbia di spigoli.
fn build_solid_faces(
    mesh: &SolidMesh,
    vs: &[SolidVert],
    pops: &[f32],
    palette: &Palette,
    inv_aspect: f32,
    s0: f32,
    dist: f32,
) -> Vec<f32> {
    let mut v = Vec::with_capacity(mesh.faces.len() * 3 * VERT_FLOATS);

    for (fi, f) in mesh.faces.iter().enumerate() {
        let pop = pops[fi];
        let idx = [f[0] as usize, f[1] as usize, f[2] as usize];
        let amp = (vs[idx[0]].amp + vs[idx[1]].amp + vs[idx[2]].amp) / 3.0;
        let bright_base = amp * 0.9 + pop * 1.2;
        if bright_base <= 0.004 {
            continue;
        }
        // Su una sfera la normale è la direzione del baricentro: niente
        // prodotti vettoriali e nessun problema di orientamento.
        let n = normalized([
            (vs[idx[0]].p[0] + vs[idx[1]].p[0] + vs[idx[2]].p[0]) / 3.0,
            (vs[idx[0]].p[1] + vs[idx[1]].p[1] + vs[idx[2]].p[1]) / 3.0,
            (vs[idx[0]].p[2] + vs[idx[1]].p[2] + vs[idx[2]].p[2]) / 3.0,
        ]);

        let mut tri = [(0.0f32, 0.0f32); 3];
        let mut s_sum = 0.0;
        for (k, &i) in idx.iter().enumerate() {
            let p = vs[i].p;
            let q = [
                p[0] + n[0] * pop,
                p[1] + n[1] * pop,
                p[2] + n[2] * pop,
            ];
            let s = perspective(dist, q[2]);
            tri[k] = (q[0] * s, q[1] * s);
            s_sum += s;
        }

        let bright = depth_fog(s_sum / 3.0, s0) * bright_base * SOLID_FACE_GAIN;
        let c = mix(palette.color_a, palette.color_b, mesh.face_t[fi]);
        for (x, y) in tri {
            v.extend_from_slice(&[
                x * inv_aspect,
                y,
                c.r * bright,
                c.g * bright,
                c.b * bright,
                1.0,
            ]);
        }
    }
    v
}

/// Vertici del solido come punti luminosi (POINTS): l'alpha fa da intensità.
fn build_solid_points(
    mesh: &SolidMesh,
    vs: &[SolidVert],
    palette: &Palette,
    inv_aspect: f32,
    s0: f32,
) -> Vec<f32> {
    let mut v = Vec::with_capacity(vs.len() * VERT_FLOATS);
    for (i, sv) in vs.iter().enumerate() {
        let a = ((0.06 + sv.amp * 0.5) * depth_fog(sv.s, s0)).clamp(0.0, 1.0);
        let c = mix(palette.color_a, palette.color_b, mesh.vert_t[i]);
        v.extend_from_slice(&[sv.sx * inv_aspect, sv.sy, c.r, c.g, c.b, a]);
    }
    v
}

// ---------------------------------------------------------------------------
// Effetto imaging stereo
// ---------------------------------------------------------------------------

/// Schiacciamento verticale del disco: vista dall'alto a 30° di elevazione,
/// quindi il semiasse verticale dell'ellisse è sin(30°) di quello orizzontale.
const IMG_TILT: f32 = 0.5;
/// Raggio del disco (il "palco"), in spazio quadrato.
const IMG_RADIUS: f32 = 0.70;
/// Focale della prospettiva: più bassa = disco più "sfondato".
const IMG_FOCAL: f32 = 3.6;
/// Traslazione verticale, per centrare il disco nel riquadro.
const IMG_Y_OFFSET: f32 = 0.10;
/// Punti campionati lungo il perimetro e lungo il lobo direzionale.
const IMG_STEPS: usize = 160;
/// Raggio minimo del lobo (non collassa mai del tutto).
const IMG_LOBE_MIN: f32 = 0.10;
/// Semi-spessore dei ribbon dell'imaging.
const IMG_RING_W: f32 = 0.006;
const IMG_LOBE_W: f32 = 0.010;

/// Proietta un punto del piano orizzontale nella vista dall'alto a 30°.
///
/// `x` = destra, `y` = profondità (positivo = lontano dall'osservatore).
/// Ritorna (schermo x, schermo y, fattore prospettico).
fn imaging_project(x: f32, y: f32) -> (f32, f32, f32) {
    let s = IMG_FOCAL / (IMG_FOCAL + y);
    (x * s, y * IMG_TILT * s + IMG_Y_OFFSET, s)
}

/// Peso della componente diffusa a un dato angolo: nullo davanti, massimo
/// dietro. L'energia decorrelata non ha una direzione, quindi la si distribuisce
/// su tutto il cerchio addensandola alle spalle — dove l'ascoltatore percepisce
/// l'ambienza. Non significa "il suono viene da dietro".
fn diffuse_weight(angle: f32) -> f32 {
    (1.0 - angle.cos()) * 0.5
}

/// Riflette un azimut frontale sull'arco posteriore secondo la stima
/// fronte/retro, seguendo il cono di confusione: la posizione "gemella" di
/// un azimut è il suo specchio rispetto all'asse laterale, cioè 45° ↔ 135°.
///
/// Con `rear = 0` l'angolo resta dov'è. Le posizioni puramente laterali (±90°)
/// sono punti fissi, giustamente: lì fronte e retro coincidono.
fn reflect_rear(azimuth: f32, rear: f32) -> f32 {
    let sign = if azimuth < 0.0 { -1.0 } else { 1.0 };
    let mag = azimuth.abs();
    sign * (mag + rear * (std::f32::consts::PI - 2.0 * mag))
}

/// Punto del piano a un dato angolo e raggio, già proiettato.
fn imaging_point(angle: f32, radius: f32) -> (f32, f32, f32) {
    let (s, c) = angle.sin_cos();
    imaging_project(s * radius, c * radius)
}

/// Differenza angolare minima tra due angoli, in -π..π.
fn angle_delta(a: f32, b: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut d = (a - b) % tau;
    if d > std::f32::consts::PI {
        d -= tau;
    } else if d < -std::f32::consts::PI {
        d += tau;
    }
    d
}

/// Aggiunge alla lista un ribbon luminoso lungo una polilinea, con colore e
/// spessore per punto, cucendolo al contenuto già presente con triangoli
/// degeneri (così tutti i ribbon stanno in una sola draw call).
fn push_glow_ribbon(
    out: &mut Vec<f32>,
    pts: &[(f32, f32)],
    cols: &[(f32, f32, f32)],
    widths: &[f32],
    closed: bool,
    inv_aspect: f32,
) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    let count = if closed { n + 1 } else { n };
    let mut strip: Vec<f32> = Vec::with_capacity(count * 2 * VERT_FLOATS);

    for k in 0..count {
        let i = k % n;
        let prev = if i == 0 {
            if closed {
                pts[n - 1]
            } else {
                pts[0]
            }
        } else {
            pts[i - 1]
        };
        let next = if i == n - 1 {
            if closed {
                pts[0]
            } else {
                pts[n - 1]
            }
        } else {
            pts[i + 1]
        };
        let (tx, ty) = (next.0 - prev.0, next.1 - prev.1);
        let len = (tx * tx + ty * ty).sqrt().max(1e-6);
        let w = widths[i];
        let (nx, ny) = (-ty / len * w, tx / len * w);
        let (px, py) = pts[i];
        let (r, g, b) = cols[i];
        strip.extend_from_slice(&[(px + nx) * inv_aspect, py + ny, r, g, b, 1.0]);
        strip.extend_from_slice(&[(px - nx) * inv_aspect, py - ny, r, g, b, -1.0]);
    }

    if !out.is_empty() {
        let last: Vec<f32> = out[out.len() - VERT_FLOATS..].to_vec();
        out.extend_from_slice(&last);
        out.extend_from_slice(&strip[..VERT_FLOATS]);
    }
    out.extend_from_slice(&strip);
}

/// Distribuzione angolare dell'energia (il "lobo") più la frequenza media che
/// arriva da ogni direzione.
///
/// Ogni banda viene scomposta in due parti, come nell'analisi direzionale dei
/// campi sonori: una **direzionale**, spalmata con un kernel gaussiano stretto
/// attorno al proprio azimut, e una **diffusa**, distribuita su tutto il
/// cerchio e addensata dietro. Il rapporto tra le due è la diffusività
/// misurata dal modulo della coerenza.
fn imaging_lobe(img: &ImagingFrame) -> ([f32; IMG_STEPS], [f32; IMG_STEPS]) {
    let mut energy = [0.0f32; IMG_STEPS];
    let mut tint = [0.0f32; IMG_STEPS];
    let denom = denom_bands();
    const DIR_SPREAD: f32 = 0.16;
    let inv2s2 = 1.0 / (2.0 * DIR_SPREAD * DIR_SPREAD);

    for b in 0..NUM_BANDS {
        let e = img.energy[b];
        if e <= 0.01 {
            continue;
        }
        let d = img.diffuseness[b].clamp(0.0, 1.0);
        let (e_dir, e_diff) = (e * (1.0 - d), e * d);
        let az = reflect_rear(img.azimuth[b], img.rear);
        let t = b as f32 / denom;

        for (i, slot) in energy.iter_mut().enumerate() {
            let a = i as f32 / IMG_STEPS as f32 * std::f32::consts::TAU;
            let delta = angle_delta(a, az);
            let w = e_dir * (-delta * delta * inv2s2).exp() + e_diff * diffuse_weight(a);
            *slot += w;
            tint[i] += w * t;
        }
    }

    for i in 0..IMG_STEPS {
        if energy[i] > 1e-6 {
            tint[i] /= energy[i];
        }
    }
    (energy, tint)
}

/// Costruisce l'effetto imaging: riempimento del lobo, ribbon (anello di
/// riferimento + tacche + lobo) e punti delle singole bande sul perimetro.
fn build_imaging(
    img: &ImagingFrame,
    palette: &Palette,
    inv_aspect: f32,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let tau = std::f32::consts::TAU;
    let (lobe, tint) = imaging_lobe(img);

    // --- Anello di riferimento: il perimetro del palco. La luminosità cresce
    // sul lato vicino, ed è questo a far leggere il disco come inclinato.
    let mut ring_pts = Vec::with_capacity(IMG_STEPS);
    let mut ring_cols = Vec::with_capacity(IMG_STEPS);
    let mut ring_w = Vec::with_capacity(IMG_STEPS);
    for i in 0..IMG_STEPS {
        let a = i as f32 / IMG_STEPS as f32 * tau;
        let (x, y, s) = imaging_point(a, IMG_RADIUS);
        ring_pts.push((x, y));
        let c = palette.color_a;
        let k = 0.22 * s * s;
        ring_cols.push((c.r * k, c.g * k, c.b * k));
        ring_w.push(IMG_RING_W * s);
    }

    // --- Lobo direzionale: raggio = energia che arriva da quella direzione.
    // Saturazione morbida, altrimenti il materiale denso lo sbatte al massimo.
    let mut lobe_pts = Vec::with_capacity(IMG_STEPS);
    let mut lobe_cols = Vec::with_capacity(IMG_STEPS);
    let mut lobe_w = Vec::with_capacity(IMG_STEPS);
    for i in 0..IMG_STEPS {
        let a = i as f32 / IMG_STEPS as f32 * tau;
        let v = 1.0 - (-lobe[i] * 0.55).exp();
        let r = IMG_LOBE_MIN + v * (IMG_RADIUS - IMG_LOBE_MIN);
        let (x, y, s) = imaging_point(a, r);
        lobe_pts.push((x, y));
        let c = mix(palette.color_a, palette.color_b, tint[i]);
        let k = (0.30 + v * 0.85) * s * s;
        lobe_cols.push((c.r * k, c.g * k, c.b * k));
        lobe_w.push(IMG_LOBE_W * s);
    }

    let mut ribbons = Vec::new();
    push_glow_ribbon(&mut ribbons, &ring_pts, &ring_cols, &ring_w, true, inv_aspect);
    push_glow_ribbon(&mut ribbons, &lobe_pts, &lobe_cols, &lobe_w, true, inv_aspect);

    // --- Tacche di orientamento a fronte / destra / retro / sinistra: senza
    // riferimenti fissi non si distingue davanti da dietro.
    for q in 0..4 {
        let a = q as f32 * std::f32::consts::FRAC_PI_2;
        let (x0, y0, s0) = imaging_point(a, IMG_RADIUS * 1.02);
        let (x1, y1, _) = imaging_point(a, IMG_RADIUS * (if q == 0 { 1.16 } else { 1.10 }));
        let c = palette.color_a;
        let k = 0.30 * s0 * s0;
        let col = (c.r * k, c.g * k, c.b * k);
        // Tre punti: `push_glow_ribbon` richiede una polilinea, non un segmento.
        let pts = [(x0, y0), ((x0 + x1) * 0.5, (y0 + y1) * 0.5), (x1, y1)];
        let cols = [col; 3];
        let w = [IMG_RING_W * s0; 3];
        push_glow_ribbon(&mut ribbons, &pts, &cols, &w, false, inv_aspect);
    }

    // --- Riempimento del lobo: ventaglio centro→curva (TRIANGLE_STRIP).
    let (cx, cy, _) = imaging_project(0.0, 0.0);
    let mut fill = Vec::with_capacity((IMG_STEPS + 1) * 2 * VERT_FLOATS);
    for k in 0..=IMG_STEPS {
        let i = k % IMG_STEPS;
        let (px, py) = lobe_pts[i];
        let c = mix(palette.color_a, palette.color_b, tint[i]);
        let f = 0.05 + (1.0 - (-lobe[i] * 0.55).exp()) * 0.10;
        fill.extend_from_slice(&[cx * inv_aspect, cy, 0.0, 0.0, 0.0, 1.0]);
        fill.extend_from_slice(&[px * inv_aspect, py, c.r * f, c.g * f, c.b * f, 1.0]);
    }

    // --- Una luce per banda sul perimetro, al proprio azimut. Le bande diffuse
    // non hanno una direzione, quindi il punto sbiadisce con la diffusività:
    // segnare un punto preciso per del riverbero sarebbe una bugia.
    let denom = denom_bands();
    let mut dots = Vec::with_capacity(NUM_BANDS * VERT_FLOATS);
    for b in 0..NUM_BANDS {
        let e = img.energy[b] * (1.0 - img.diffuseness[b].clamp(0.0, 1.0));
        if e <= 0.01 {
            continue;
        }
        let (x, y, s) = imaging_point(reflect_rear(img.azimuth[b], img.rear), IMG_RADIUS);
        let c = mix(palette.color_a, palette.color_b, b as f32 / denom);
        let a = (e * s * s).clamp(0.0, 1.0);
        dots.extend_from_slice(&[x * inv_aspect, y, c.r, c.g, c.b, a]);
    }

    (fill, ribbons, dots)
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
