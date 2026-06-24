#!/usr/bin/env bash
# Installa Sinestesia a livello utente (~/.local), senza permessi di root.
set -euo pipefail

cd "$(dirname "$0")"

APP_ID="dev.akusen.sinestesia"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

echo "==> Compilazione release…"
cargo build --release

echo "==> Installazione file…"
install -Dm755 target/release/sinestesia "${BIN_DIR}/sinestesia"
install -Dm644 "${APP_ID}.desktop" "${APP_DIR}/${APP_ID}.desktop"
install -Dm644 assets/sinestesia.svg "${ICON_DIR}/${APP_ID}.svg"

# Aggiorna i database (best-effort)
update-desktop-database "${APP_DIR}" 2>/dev/null || true
gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" 2>/dev/null || true

echo "==> Fatto."
case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "Nota: ${BIN_DIR} non è nel PATH; aggiungilo per lanciare 'sinestesia' da terminale." ;;
esac
echo "Sinestesia dovrebbe ora comparire nel menu applicazioni."
