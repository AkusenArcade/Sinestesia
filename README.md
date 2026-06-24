# Sinestesia

Visualizzatore audio per Linux scritto in **Rust** con **Relm4 / GTK4 + libadwaita**.
Cattura l'audio di sistema (output o input) via **PipeWire**, ne analizza lo spettro in
tempo reale (FFT) e lo rende con effetti fluidi su **OpenGL** (GtkGLArea).

![icona](assets/sinestesia.svg)

## Caratteristiche

- **Effetti** selezionabili a runtime:
  - *Barre* (stile Cava)
  - *Linea* (curva dello spettro)
  - *Radiale* (spettro ad anello con particelle)
  - *Linea Spettro* e *Radiale Spettro* (varianti "neon": riempimento trasparente,
    bordo luminoso da 1px, bagliore; outline a visibilità proporzionale al volume)
- **Layout speculare stereo**: centro = basse frequenze, bordi/lati = alte;
  metà sinistra = canale L, metà destra = canale R (in input: mirror mono).
- **Colori**: manuali (due color picker) o **automatici** dal tema di sistema
  (accent color di libadwaita, aggiornato live con matugen/noctalia).
- **Sorgente** audio commutabile tra uscita (monitor) e ingresso (microfono).
- **Gain** (moltiplicatore d'ampiezza) e **Scia** (motion blur) regolabili.
- Impostazioni persistenti in `~/.config/sinestesia/config.toml`.
- L'UI segue automaticamente il tema GTK di sistema (libadwaita).

## Requisiti

- Rust (edition 2021)
- PipeWire, GTK4, libadwaita, libepoxy (header di sviluppo)
- Un compositor con OpenGL/EGL (Wayland o X11)

## Build ed esecuzione

```sh
cargo run --release
```

## Installazione (menu applicazioni)

```sh
./install.sh
```

Installa il binario in `~/.local/bin`, la voce `.desktop` e l'icona in `~/.local/share`.

## Documentazione

Le specifiche complete sono in [`PRD.md`](PRD.md).

## Licenza

MIT
