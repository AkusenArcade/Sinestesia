# PRD — Sinestesia

**Visualizzatore audio per Linux**
Versione documento: 1.0 · Data: 2026-06-24 · Stato: Bozza approvata per sviluppo

---

## 1. Sommario

Sinestesia è un visualizzatore audio desktop per Linux, scritto in **Rust** con frontend **GNOME / Relm4 (GTK4)**. Cattura il segnale audio di sistema (output o input), ne analizza lo spettro in tempo reale e lo rende con effetti grafici fluidi su una finestra a sfondo nero. I colori sono personalizzabili manualmente o sincronizzati automaticamente con il tema di sistema generato da matugen/noctalia.

### 1.1 Obiettivi

- Visualizzazione audio in tempo reale, fluida (target 60 FPS) e a bassa latenza.
- Più effetti di visualizzazione selezionabili a runtime.
- Personalizzazione cromatica manuale (color picker) o automatica (tema di sistema).
- Switch tra audio in uscita (monitor) e audio in ingresso (microfono).
- Amplificazione del segnale tramite moltiplicatore regolabile.
- Persistenza delle preferenze tra le sessioni.

### 1.2 Non-obiettivi (fuori scope v1)

- Registrazione o esportazione di audio/video.
- Plugin di terze parti / scripting.
- Supporto Windows/macOS.
- Mixer audio o controllo del volume di sistema.
- Visualizzazioni basate su file audio caricati (solo stream live di sistema).

---

## 2. Stack tecnico (deciso)

| Area | Scelta | Note |
|---|---|---|
| Linguaggio | Rust (edition 2021, toolchain ≥ 1.95) | |
| UI framework | Relm4 + GTK4 + **libadwaita** | Componenti, message-passing; Adwaita per theming di sistema automatico |
| Audio backend | **PipeWire** | Cattura monitor (output) e sorgenti (input) native |
| Analisi spettro | FFT (crate `rustfft`) | Finestratura Hann, binning logaritmico |
| Rendering | **GtkGLArea (OpenGL)** | Shader per fluidità su particelle e barre |
| Colori automatici | Lettura file `~/.config/noctalia/colors.json` | Parser JSON + watch su modifica |
| Persistenza | File TOML in `~/.config/sinestesia/config.toml` | |

---

## 3. Utente e contesto d'uso

**Utente target:** utente Linux desktop (GNOME/Wayland, es. CachyOS) che usa matugen/noctalia per il theming di sistema e vuole un visualizzatore estetico per musica/ambient, da tenere in finestra o a schermo.

**Scenari principali:**
1. Avvio l'app mentre ascolto musica → vedo subito le barre reagire all'output audio.
2. Cambio effetto dal menu per variare l'estetica.
3. Attivo la modalità colori automatica → il visualizzatore segue la palette del wallpaper corrente.
4. Passo il microfono come sorgente per reagire alla voce/ambiente.
5. Il segnale è debole → alzo il moltiplicatore finché il grafico riempie l'area.

---

## 4. Requisiti funzionali

### FR-1 — Cattura audio (PipeWire)
- FR-1.1 L'app si collega a PipeWire e cattura un flusso PCM in tempo reale.
- FR-1.2 Switch sorgente **Output (monitor) ↔ Input (microfono)** a runtime, senza riavvio.
- FR-1.3 La sorgente di default è l'**output di sistema** (monitor del sink predefinito).
- FR-1.4 Gestione robusta di disconnessione/cambio device (nessun crash; ripristino o stato "nessun segnale").
- FR-1.5 (v1.1, opzionale) Selezione esplicita del device tra quelli disponibili.

### FR-2 — Analisi del segnale
- FR-2.1 Buffer audio → finestratura Hann → FFT → magnitudo per banda.
- FR-2.2 Binning su scala **logaritmica** in frequenza (percezione musicale).
- FR-2.3 Smoothing temporale (attack/decay) per evitare flicker tra i frame.
- FR-2.4 Numero di bande configurabile internamente per effetto (default ~64 barre).
- FR-2.5 Normalizzazione con compensazione su gamma dinamica.

### FR-3 — Effetti di visualizzazione
- FR-3.1 **Barre stile Cava** (MVP): barre verticali dello spettro, smoothing, gradiente colore. *Primo effetto end-to-end.*
- FR-3.2 **Linea spettro frequenze**: curva continua dello spettro.
- FR-3.3 **Radiale con particelle**: spettro disposto in cerchio con particelle reattive.
- FR-3.4 Architettura a "effetto plugin interno" (trait comune) per aggiungerne altri.
- FR-3.5 Selezione effetto a runtime; transizione senza riavvio.
- FR-3.6 **Layout speculare stereo**: il centro dell'area è l'origine; dal centro verso l'esterno si va dalle frequenze **basse** alle **alte**. Metà sinistra = canale **sinistro**, metà destra = canale **destro** (evita grafici sbilanciati). In modalità **input** lo stesso canale è specchiato sui due lati. Applicato a barre e linea (centro orizzontale) e al radiale (due semicerchi SX/DX, basse vicino all'asse verticale).

### FR-4 — Colori
- FR-4.1 **Modalità manuale**: due colori scelti via GTK color picker → gradiente A→B applicato all'effetto.
- FR-4.2 **Modalità automatica**: lettura `~/.config/noctalia/colors.json`; mappatura chiavi (`mPrimary`, `mSecondary`, `mTertiary`, `mSurface`, …) → colori dell'effetto.
- FR-4.3 In automatico, sfondo derivato da `mSurface`/nero; colori grafico da primary/secondary (configurabile).
- FR-4.4 **Live reload**: watch del file colori; aggiornamento immediato al cambio tema.
- FR-4.5 Fallback a palette di default se file assente o malformato.

### FR-5 — Moltiplicatore di ampiezza
- FR-5.1 Slider che amplifica l'ampiezza renderizzata (es. range 0.1×–10×, default 1×).
- FR-5.2 Applicato dopo l'analisi, prima del rendering; effetto visibile immediato.
- FR-5.3 Clamping per evitare overflow/clipping grafico fuori area.

### FR-6 — Interfaccia
- FR-6.1 Area principale: **sfondo nero** + visualizzatore a tutta area.
- FR-6.2 Controlli accessibili (header bar e/o pannello/popover): selettore effetto, switch sorgente, toggle modalità colore + due color picker, slider moltiplicatore.
- FR-6.3 Possibilità di nascondere i controlli per modalità "solo visualizzatore".
- FR-6.4 Ridimensionamento finestra fluido; il rendering si adatta.

### FR-7 — Persistenza
- FR-7.1 Salvataggio in `~/.config/sinestesia/config.toml`: effetto attivo, modalità colore, due colori manuali, sorgente, moltiplicatore, (eventuale) device.
- FR-7.2 Caricamento all'avvio; default sensati se file assente.
- FR-7.3 Salvataggio al cambio impostazione e/o alla chiusura.

### FR-8 — Theming dell'app conforme a noctalia/sistema
- FR-8.1 L'UI dell'app (header bar, controlli, popover, widget) deve seguire **automaticamente** il tema GTK di sistema generato da noctalia/matugen.
- FR-8.2 Implementazione tramite **libadwaita**: l'app usa i widget Adwaita e i *named color* standard (`accent_bg_color`, `window_bg_color`, `headerbar_bg_color`, ecc.), che GTK4 legge automaticamente da `~/.config/gtk-4.0/gtk.css` (generato da noctalia). Nessuna palette custom per la chrome.
- FR-8.3 Rispetto del `color-scheme` di sistema (es. `prefer-dark`) via `AdwStyleManager`.
- FR-8.4 Al cambio tema di noctalia, l'app riflette i nuovi colori della chrome (al riavvio è accettabile in v1; live-reload della chrome è nice-to-have). *Nota:* il live-reload riguarda i colori del **visualizzatore** (FR-4.4), distinto dalla chrome Adwaita.
- FR-8.5 Coerenza: lo sfondo nero dell'area visualizzatore resta nero indipendentemente dal tema chrome (l'area di rendering non eredita `window_bg_color`).

---

## 5. Requisiti non funzionali

- **NFR-1 Performance:** target 60 FPS; rendering GPU (OpenGL); thread audio separato dalla UI.
- **NFR-2 Latenza:** ritardo percepito audio→grafico < ~50 ms.
- **NFR-3 Robustezza:** nessun crash su cambio/perdita device o file colori malformato.
- **NFR-4 Risorse:** uso CPU contenuto a riposo/segnale assente; nessun busy-loop.
- **NFR-5 Threading:** audio capture + FFT su thread dedicato; comunicazione verso UI via canale; nessun blocco del main loop GTK.
- **NFR-6 Portabilità Linux:** Wayland e X11; testato su GNOME/CachyOS.
- **NFR-7 Manutenibilità:** moduli disaccoppiati (audio, dsp, render, ui, config, theme).

---

## 6. Architettura

```
┌─────────────────────────────────────────────────────────┐
│                      Relm4 App (UI)                       │
│  Header/controlli · GtkGLArea (render) · stato impostaz. │
└───────────────▲───────────────────────────▲─────────────┘
                │ frame data (canale)        │ config events
   ┌────────────┴───────────┐    ┌───────────┴────────────┐
   │   Audio + DSP thread   │    │   Theme watcher        │
   │ PipeWire → ring buffer │    │ inotify su colors.json │
   │ → Hann → rustfft → bins│    │ → palette update       │
   └────────────────────────┘    └────────────────────────┘
```

### 6.1 Moduli (crate singolo, moduli interni)

- `audio/` — connessione PipeWire, selezione sorgente output/input, ring buffer PCM.
- `dsp/` — finestratura, FFT (`rustfft`), binning log, smoothing, normalizzazione, moltiplicatore.
- `render/` — `GtkGLArea`, contesto OpenGL, shader, trait `Visualizer` + implementazioni (Bars, Line, Radial).
- `theme/` — palette manuale, parser `colors.json`, file watcher, mapping colori.
- `config/` — load/save TOML, struct `Settings`.
- `ui/` — componenti Relm4, header bar, controlli, wiring messaggi.
- `app.rs` / `main.rs` — bootstrap, canali, ciclo di vita.

### 6.2 Trait visualizzatore (concetto)

```rust
trait Visualizer {
    fn name(&self) -> &str;
    fn render(&mut self, gl: &GlContext, frame: &SpectrumFrame, palette: &Palette, size: (i32, i32));
}
```
Ogni effetto è un'implementazione; il selettore UI scambia l'implementazione attiva.

### 6.3 Dipendenze previste (indicative)

`relm4`, `gtk4` (con feature OpenGL), `pipewire`, `rustfft`, `serde` + `serde_json` + `toml`, `notify` (file watch), `glow`/`gl` per OpenGL, `anyhow`/`thiserror`.

---

## 7. Piano di sviluppo a milestone

### M0 — Bootstrap progetto
Cargo project, dipendenze base, finestra Relm4 vuota a sfondo nero, struttura moduli, config TOML load/save scheletro.

### M1 — MVP: barre Cava end-to-end ⭐
PipeWire cattura output → ring buffer → FFT → binning log → **GtkGLArea con barre verticali** colorate (gradiente da palette di default). Smoothing temporale. **Criterio di successo:** riproduco musica e vedo le barre reagire fluide a 60 FPS.

### M2 — Controlli base + moltiplicatore + persistenza
Header/pannello con switch sorgente (output/input), slider moltiplicatore, salvataggio/ripristino impostazioni.

### M3 — Colori
Color picker manuale (2 colori) + modalità automatica con parser `noctalia/colors.json` e live reload. Toggle modalità.

### M4 — Effetti aggiuntivi
Linea spettro + radiale con particelle; selettore effetto a runtime.

### M5 — Rifinitura
Gestione device hot-plug, modalità "solo visualizzatore", ottimizzazioni performance, packaging.

---

## 8. Rischi e mitigazioni

| Rischio | Mitigazione |
|---|---|
| Integrazione PipeWire complessa in Rust | Isolare in modulo `audio`; iniziare dal monitor del sink di default; fallback a pipewire-pulse se necessario |
| GtkGLArea + Relm4 + shader: curva ripida | MVP con shader minimi (barre); incapsulare GL dietro al trait `Visualizer` |
| Sincronizzazione thread audio↔UI | Canale lock-free / ring buffer; UI legge l'ultimo frame disponibile |
| Formato `colors.json` variabile tra versioni | Parser tollerante con chiavi opzionali + fallback palette |

---

## 9. Criteri di accettazione v1

- [ ] L'app si avvia mostrando una finestra a sfondo nero.
- [ ] Le barre reagiscono in tempo reale all'audio di output, fluide.
- [ ] Switch output/input funziona a runtime.
- [ ] Slider moltiplicatore scala visibilmente l'ampiezza.
- [ ] Modalità colore manuale (2 picker) e automatica (noctalia) funzionano, con live reload del tema.
- [ ] Almeno 3 effetti selezionabili (barre, linea, radiale).
- [ ] Le impostazioni persistono tra i riavvii.
- [ ] Nessun crash su perdita device o file colori mancante/malformato.

---

## 10. Domande risolte (decisioni di progetto)

- Backend audio → **PipeWire**.
- Rendering → **GtkGLArea / OpenGL**.
- Matugen → **lettura file** `~/.config/noctalia/colors.json`.
- Persistenza → **sì**, TOML in `~/.config/sinestesia/`.
- Primo effetto / MVP → **barre stile Cava**.
