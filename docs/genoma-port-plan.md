# Sinestesia on Genoma — port plan

Status: draft, 2026-11-18. Source: the Relm4/GTK4 + libadwaita version 0.6.2
(`src/main.rs` UI, `src/audio.rs` PipeWire, `src/dsp.rs` FFT,
`src/render.rs` ~2900 lines of OpenGL through `GtkGLArea`, `src/theme.rs`,
`src/config.rs`).

## 1. Goal

The same visualiser — eleven effects, output/input source, gain, trail,
manual or automatic colours, viewer-only mode — as a Genoma `g.window`
(`app_id = "dev.akusen.sinestesia"`), its effects drawn by Genoma's GPU
paths (shaders, simulations, particles, paths) instead of hand-written GL.

## 2. What already exists

Genoma's `spectrum` service **is** Sinestesia's analysis: `genoma spectrum`
was taken from `src/audio.rs` (by way of Bioma's `tools/sinestesia-bands`)
and gives 64 bands a channel, 30 Hz–16 kHz logarithmic, 0…1, captured only
while held, output or input (`spectrum:listen_to`). Same FFT size, same band
count, same range as `src/dsp.rs`.

What it does **not** give:

- **Gain before smoothing.** Sinestesia applies gain inside `analyze`; in
  Luau it can only be applied after. Close but not identical — compare by
  eye; if it matters, add a gain option to `genoma spectrum`.
- **Raw samples / mid-side trajectory** — needed by *Fase* (Phase), which is dropped (§8).
- **The stereo image** (`ImagingAnalyzer`: two extra FFTs, ITD/ILD
  crossover) — needed by *Imaging*.
- **Mirror in input mode** is trivial in Luau (one channel on both sides).

## 3. The rendering problem

Sinestesia builds vertices on the CPU every frame and draws them with
`glDrawArrays` (triangles, strips, lines, points), on top of a fade quad for
the trail. Genoma has no "draw these vertices" node. It has:

- `g.shader` — a fragment function over a rectangle, with up to 1024 bound
  numbers (`value(i)`, 256 × vec4) and the `g_` library (SDFs, noise);
- `g.shader { compute = … }` — a GPU simulation: a `grid` stepped from its
  neighbours, or `agents` stepped from each other, persistent across frames;
- `g.emitter` — GPU particles;
- `g.path` — any outline from points, filled or stroked;
- `pass = true` shaders and built-in `glow`.

128 band values fit in `value(i)` many times over, so every effect that
reads only the current spectrum is a shader. History and persistence go in a
compute grid used as a ring buffer.

### Effect by effect

| Effect | Genoma technique | Risk |
|---|---|---|
| Barre (Bars) | fragment shader: band index from x, `g_sd_box` per bar, gradient A→B | low |
| Linea (Line) | `g.path` with points smoothed (Catmull-Rom) in Luau each frame, or a shader's distance to 128 segments | low |
| Linea Spettro | the same + fill + `glow` pass, outline alpha from level | low |
| Radiale | polar shader (angle → band) + `g.emitter` whose `rate` follows the bass | low |
| Radiale Spettro | polar neon version of the above | low |
| Tunnel | compute grid as a ring of frozen spectrum shapes (64 × N rings); bass-driven travel integrated in Luau and bound as a value; stars procedural from `g_hash` with z = fract(seed − travelled) | medium |
| Poliedro (Solid) | icosphere projected in Luau, edges (≤ 256 segments) bound as values and drawn with `g_sd_segment`; faces as a second shader or `g.path` polygons | medium — check edge count and Luau time per frame |
| Nebulosa | compute `agents`: agent *i* has a fixed Fibonacci direction and a shell = band; radius from that band's value; shock waves as a value carrying their age; `agent_size` draws them | medium |
| Imaging | needs the imaging analysis from Genoma (§5), then three lobes drawn as polar SDFs | high — blocked |

**Scia (trail / motion blur).** Sinestesia draws a translucent black quad
over the previous frame. Genoma redraws each frame from nothing, so this
needs a persistent buffer: either a full-window compute grid holding the last
frame, or (better) a Genoma feature — a `pass` shader that can read its own
previous output (`g_previous(p)`). See §5.

**Fixed 60 Hz simulation step** (`VizState.steps`, catch-up capped at 4).
Effects with state (Tunnel, Nebula) step from `shader.time` / the
simulation's `steps` rate, not from frames drawn, so the result does not
depend on the refresh rate.

## 4. Theme: follows Bioma, as GTK apps do

GTK apps follow Bioma because GTK reads `gtk.css` by convention and Bioma
writes it (the gtk3/gtk4 matugen templates). A Genoma app needs the same
arrangement, and it belongs to the toolkit, not to each app:

- **Genoma** defines a well-known file, `$XDG_CONFIG_HOME/genoma/theme.json`
  (roles, light/dark, radius and the other kit sizes, fonts). The kit
  follows it by itself when it exists — `kit.theme` watches it with `g.file`
  and every change is a crossing, so the app retints live, animated — and
  falls back to its own tokens when it does not. An app can opt out.
- **Bioma** writes that file whenever its effective theme changes: the
  roles at rest from `core/theme.luau` (manual palette or matugen), the mode,
  and the Appearance settings that map onto kit tokens. Nothing in Genoma
  names Bioma; any shell built on Genoma can write the same file.
- The app does nothing beyond using the kit and `kit.theme.get(...)` for its
  own colours. The visual area of Sinestesia stays black by design.

This is a prerequisite for both apps (§6).

## 5. Interface

| GTK version | Genoma |
|---|---|
| `adw::ApplicationWindow`, header bar | `g.window` with the kit's title bar (`move_window`) |
| Controls panel/popover | a side panel: kit `dropdown` (effect), two-way switch (output/input), `slider` (gain 0.1×–10×, trail), switch (manual/auto colours) |
| Two `GtkColorDialogButton`s | kit `color_picker.button` (field, hue strip, hex, swatches) |
| Auto colours from `~/.config/gtk-4.0/gtk.css` accent | the shared theme file (§4): `kit.theme.get("accent")` and friends, live |
| F11 / H / Esc | `on_key` on the window; F11 needs Genoma's fullscreen request (§5) |
| `config.toml` | `config.json` through `g.file:write`; on first run read the old TOML's few keys by hand (flat `key = value` lines) and write JSON |
| Black visual area regardless of theme | the visual area's background is black, not a theme colour |

## 6. What Genoma needs first

| Need | For | Size |
|---|---|---|
| A window asking for fullscreen from Luau (`xdg_toplevel.set_fullscreen`) | F11, viewer-only mode | small |
| A pass that reads its own previous frame (`g_previous`) | Scia (trail) | medium |
| `spectrum` extended: optional imaging frame (per band azimuth + width) while held | Imaging | medium |
| Optional: gain applied inside `genoma spectrum` | exact parity of gain | small |
| The shared theme file, followed by the kit; Bioma writing it | chrome and automatic colours follow Bioma live (§4) | small–medium |

The imaging extension is the code already in `src/dsp.rs`
(`ImagingAnalyzer`) moved into `genoma spectrum`, computed only when someone
asks for it — the "only while held" rule extended to each extra analysis.
`PhaseFilter` and the mid/side samples stay out, with the Fase effect.

## 7. Phases

0. **Genoma prerequisites**: fullscreen request, the shared theme file,
   `g_previous`. (The
   spectrum extensions can wait for phase 5.)
1. **Skeleton**: window, controls panel, config JSON + TOML migration,
   `spectrum:hold()` while the window exists, source switch, gain.
2. **2D effects**: Bars, Line, Line Spectrum, Radial, Radial Spectrum, trail.
   Compare each side by side with the GTK version on the same music.
3. **Colours**: automatic from the shared theme file (live), the kit's
   `color_picker` for manual.
4. **State effects**: Tunnel, Nebula, Solid — each prototyped alone
   first in a scratch dir, measured with `genoma ipc genoma profile on`
   against the GTK version's CPU/GPU cost.
5. **Imaging**: extend `genoma spectrum` with the imaging frame, then the
   effect.
6. **Viewer-only mode + packaging**: F11/H/Esc, `.desktop` with
   `Exec=genoma <app dir>`, AUR package depending on `genoma`.

Verification: `genoma snapshot --mock @genoma/spectrum=<file>` with fixed
band arrays (silence, a sweep, a bass hit, hard left, hard right) gives a
picture per effect per case without audio — the same headless method used
for Bioma's cells.

## 8. Decisions (2026-11-18)

- Bioma's look, not Adwaita's, and it follows Bioma's theme live as GTK
  apps do (§4).
- Sinestesia stays a separate app; it is not merged into Bioma.
- Genoma prerequisites come first (fullscreen request first: both apps need it).
- 2026-11-20: Rilievo (Terrain) and Fase (Phase) are dropped from the port (they were deferred first): he does not like them. Eleven effects become nine on Genoma.

## 9. Later

- Effects that are only possible in Genoma (morph between effects, glass
  over the visual) — after parity, not before.
