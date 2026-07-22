# Sinestesia

Visualizzatore audio per Linux scritto in **Rust** con **Relm4 / GTK4 + libadwaita**.
Cattura l'audio di sistema (output o input) via **PipeWire**, ne analizza lo spettro in
tempo reale (FFT) e lo rende con effetti fluidi su **OpenGL** (GtkGLArea).

![icona](assets/sinestesia.svg)

## Screenshot

I cinque effetti, ciascuno con un tema (colori automatici) e uno sfondo diversi:

| Barre | Linea |
|:-:|:-:|
| ![Barre](assets/screenshots/bars.png) | ![Linea](assets/screenshots/line.png) |
| **Radiale** | **Radiale Spettro** |
| ![Radiale](assets/screenshots/radial.png) | ![Radiale Spettro](assets/screenshots/radialspectrum.png) |

**Linea Spettro** — variante neon, bordo luminoso a visibilità proporzionale al volume:

![Linea Spettro](assets/screenshots/linespectrum.png)

## Caratteristiche

- **Effetti** selezionabili a runtime:
  - *Barre* (stile Cava)
  - *Linea* (curva dello spettro)
  - *Radiale* (spettro ad anello con particelle)
  - *Linea Spettro* e *Radiale Spettro* (varianti "neon": riempimento trasparente,
    bordo luminoso da 1px, bagliore; outline a visibilità proporzionale al volume)
  - *Tunnel* (anelli che congelano la sagoma dello spettro e sfrecciano verso
    l'osservatore avvitandosi, più campo di stelle; le basse frequenze
    accelerano corsa, vortice ed emissione — ottimo con la Scia alzata)
  - *Poliedro* (solido geodetico 3D con spigoli luminosi: la latitudine dà la
    frequenza e l'emisfero il canale, i bassi lo avvicinano alla camera e i
    transienti fanno esplodere le facce verso l'esterno)
  - *Nebulosa* (campo di migliaia di particelle su gusci sferici concentrici,
    uno per fascia di frequenza: i bassi al centro, gli acuti in superficie. Le
    direzioni sono fisse (spirale di Fibonacci) e a muoversi è solo il raggio,
    spinto dall'energia di banda, così il campo respira invece di formicolare.
    I transienti lanciano onde d'urto radiali che si propagano dal centro verso
    l'esterno; ruota su due assi con parallasse vera e il bagliore cala con la
    profondità, che è ciò che fa leggere il volume come sfera e non come disco)
  - *Rilievo* (spettrogramma 3D: ogni frame nasce una cresta in primo piano con
    la sagoma dello spettro e le vecchie scorrono verso l'orizzonte sfumando
    nella foschia, così restano visibili ~2,5 s di storia — il picco di due
    secondi fa è ancora lì, come montagna in fondo. L'asse in profondità è il
    tempo e scorre a passo fisso: accelerarlo sui bassi vorrebbe dire deformare
    l'asse dei tempi. I bassi avvicinano la camera, i transienti restano
    impressi nella cresta che nasce in quel momento e si allontanano con lei)
  - *Fase* (vettorscopio esteso nel tempo: X = side, Y = mid, Z = tempo. La
    traiettoria mid/side della forma d'onda — l'unico effetto che disegna il
    segnale invece dello spettro — diventa un nastro che si avvita in
    profondità mentre la camera oscilla su due assi. Un crossover a due tagli
    (250 Hz, 2,5 kHz) la separa in tre nastri concentrici: i bassi stretti
    attorno all'asse, medi e alti via via più larghi e mossi, così la parte
    stereo risalta invece di sparire in un unico filo. Mono = fili verticali,
    stereo largo = spirali che si aprono, controfase = nastro rovesciato sulla
    diagonale. I bassi avvicinano la camera)
  - *Imaging* (immagine stereo frontale: semicerchio con l'ascoltatore al
    centro della corda, ogni banda posizionata nella direzione da cui è
    percepita, e tre lobi separati per bassi / medi / alti — sotto i 250 Hz la
    localizzazione è debole e il mix tiene quasi sempre i bassi al centro,
    quindi un lobo unico li lasciava coprire proprio la parte direzionale. L'azimut combina differenza di tempo e di ampiezza secondo la
    teoria duplex e passa per la legge della tangente; l'arco disegnato *è* il
    palco stereo, quindi i suoi estremi sono i due diffusori (±30° reali) e un
    pan tutto a destra finisce sul bordo destro. Niente metà posteriore:
    da due canali il fronte/retro non è ricostruibile, e l'energia decorrelata
    — riverbero, ambienza — non ha direzione, quindi allarga il lobo invece di
    spostarsi da un lato)
- **Layout speculare stereo**: centro = basse frequenze, bordi/lati = alte;
  metà sinistra = canale L, metà destra = canale R (in input: mirror mono).
- **Colori**: manuali (due color picker) o **automatici** dal tema di sistema
  (accent color di libadwaita, aggiornato live con matugen/noctalia).
- **Sorgente** audio commutabile tra uscita (monitor) e ingresso (microfono).
- **Gain** (moltiplicatore d'ampiezza) e **Scia** (motion blur) regolabili.
- **Modalità solo visualizzatore**: `F11` schermo intero senza barre, `H`
  nasconde/mostra header bar e pannello controlli restando in finestra, `Esc`
  ripristina tutto.
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
