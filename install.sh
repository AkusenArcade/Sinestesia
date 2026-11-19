#!/usr/bin/env bash
# Installa Sinestesia a livello utente (~/.local), senza permessi di root.
# Serve genoma (0.1.21 o successivo) nel PATH.
set -euo pipefail

cd "$(dirname "$0")"

APP_ID="dev.akusen.sinestesia"
BIN_DIR="${HOME}/.local/bin"
SHARE_DIR="${HOME}/.local/share/sinestesia"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"

command -v genoma >/dev/null || { echo "Serve genoma nel PATH (pacchetto AUR genoma)."; exit 1; }

echo "==> Installazione file…"
rm -rf "${SHARE_DIR}"
mkdir -p "${SHARE_DIR}"
cp -r genoma/. "${SHARE_DIR}/"
# Il lanciatore del pacchetto, puntato alla copia utente.
mkdir -p "${BIN_DIR}"
sed "s|/usr/share/sinestesia|${SHARE_DIR}|" sinestesia > "${BIN_DIR}/sinestesia"
chmod 755 "${BIN_DIR}/sinestesia"
install -Dm644 "${APP_ID}.desktop" "${APP_DIR}/${APP_ID}.desktop"
install -Dm644 assets/sinestesia.svg "${ICON_DIR}/${APP_ID}.svg"

update-desktop-database "${APP_DIR}" 2>/dev/null || true
gtk-update-icon-cache -f -t "${HOME}/.local/share/icons/hicolor" 2>/dev/null || true

echo "==> Fatto."
case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "Nota: ${BIN_DIR} non è nel PATH; aggiungilo per lanciare 'sinestesia' da terminale." ;;
esac
