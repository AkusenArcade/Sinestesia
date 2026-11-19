# Sinestesia

Visualizzatore audio per Linux. Dalla 1.0 è un'app **Genoma** (Rust + Luau,
`~/Documents/Development/Genoma`) nello stile di **Bioma**: gli effetti sono
shader e simulazioni sulla GPU, l'audio lo cattura Genoma (`genoma spectrum`,
PipeWire), l'interfaccia segue il tema del desktop.

![icona](assets/sinestesia.svg)

> **La versione GTK è deprecata.** Fino alla 0.6.2 Sinestesia era scritta in
> Rust con Relm4 / GTK4 + libadwaita e disegnava su OpenGL; i sorgenti restano
> in `src/` come riferimento, ma non vengono più compilati né pacchettizzati.

## Effetti

- *Barre*, *Linea*, *Radiale* (con le particelle che escono dai raggi che
  salgono), *Linea Spettro* e *Radiale Spettro* (riempimento tenue e bordo neon
  visibile quanto il volume).
- *Tunnel*: anelli che congelano la sagoma dello spettro e corrono verso chi
  guarda avvitandosi, con un campo di stelle; i bassi accelerano corsa e vortice.
- *Poliedro*: icosaedro geodetico gonfiato dallo spettro (la latitudine dà la
  frequenza, l'emisfero il canale); i bassi lo avvicinano, i colpi staccano le
  facce.
- *Nebulosa*: migliaia di luci su gusci sferici, i bassi al centro e gli acuti
  in superficie; i colpi lanciano onde d'urto, la profondità dà la luce.
- *Imaging*: l'immagine stereo davanti all'ascoltatore, in forme organiche: tre
  lobi (bassi, medi, alti) come cellule, la membrana nitida quanto il suono è
  localizzato e sfumata quanto è diffuso, e una spora per banda nella sua
  direzione. L'analisi stereo si chiede a Genoma solo mentre l'effetto è scelto.

Layout speculare: centro = basse, bordi = alte; metà sinistra = canale L. In
ingresso il microfono, mono, è specchiato sui due lati.

## Uso

- I controlli non si vedono finché non servono: la capsula in basso apre la
  cella con effetto, sorgente (uscita o ingresso), colori (dal tema o scelti),
  gain e scia (motion blur).
- `F11` schermo intero, `H` nasconde la capsula, `Esc` chiude la cella o torna
  in finestra. La finestra si chiude dal compositor.
- Colori automatici e caratteri vengono dal tema condiviso che Bioma scrive
  (`~/.config/genoma/theme.json`): l'accento e la sua tinta schiarita.
- Impostazioni in `~/.config/sinestesia/config.json`; al primo avvio quelle
  della versione GTK (`config.toml`) vengono migrate.

## Installazione

Dall'AUR: `sinestesia` (dipende da `genoma`).

A livello utente, con `genoma` già installato:

```sh
./install.sh
```

Copia l'app in `~/.local/share/sinestesia` e mette il lanciatore in
`~/.local/bin`, con la voce `.desktop` e l'icona.

## Sviluppo

```sh
genoma genoma/                 # dal checkout, ricaricata a ogni salvataggio
genoma/tools/effects           # rigenera gli shader da effects/source/
genoma check genoma/           # controlla Luau e WGSL
```

L'icona è generata con `tools/app-icon` di Bioma dal glifo `sinestesia.svg`.

## Documentazione

Il piano del passaggio a Genoma è in [`docs/genoma-port-plan.md`](docs/genoma-port-plan.md);
le specifiche originali in [`PRD.md`](PRD.md).

## Licenza

MIT
