#!/usr/bin/env bash
set -euo pipefail

# Jednolinijkowy instalator SoundWave CLI (Linux x86_64)
# Użycie:
#   curl -fsSL https://github.com/KomodaOskara/Soundwave-Aplikacja/releases/latest/download/install.sh | bash
# lub z własnego adresu (np. serwer WWW):
#   SWCLI_URL="https://twojadomena.pl/downloads/soundwave-cli-linux-x86_64" bash install.sh

REPO="${SWCLI_REPO:-KomodaOskara/Soundwave-Aplikacja}"
BIN_URL="${SWCLI_URL:-}"
INSTALL_DIR="${HOME}/.local/bin"

echo "♪ Instalacja SoundWave CLI (Linux x86_64)..."

if [ -z "$BIN_URL" ]; then
  BIN_URL="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=20" \
    | grep -oE 'https://[^"]+soundwave-cli-linux-x86_64' | head -n 1 || true)"
fi

if [ -z "$BIN_URL" ]; then
  echo "Błąd: nie znaleziono binarki soundwave-cli-linux-x86_64 w wydaniach ${REPO}." >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
tmp_file="$(mktemp)"
trap 'rm -f "$tmp_file"' EXIT

curl -fL --progress-bar "$BIN_URL" -o "$tmp_file"
chmod +x "$tmp_file"
mv "$tmp_file" "${INSTALL_DIR}/soundwave-cli"
ln -sf "${INSTALL_DIR}/soundwave-cli" "${INSTALL_DIR}/swcli"

echo ""
echo "✓ Zainstalowano w ${INSTALL_DIR}/soundwave-cli oraz symlink swcli"

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo "⚠ Brak ffmpeg — zainstaluj go menedżerem pakietów (np. sudo dnf install ffmpeg / sudo apt install ffmpeg)."
fi
if ! command -v paplay >/dev/null 2>&1; then
  echo "⚠ Brak paplay — zainstaluj pulseaudio-utils / pipewire-pulseaudio."
fi

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo "⚠ Dodaj do PATH (np. w ~/.bashrc lub ~/.config/fish/config.fish):"
    echo "   export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac

echo ""
echo "Uruchom wpisując:  swcli   (albo soundwave-cli)"
