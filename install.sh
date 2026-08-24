#!/bin/sh
set -eu

REPO="AkaruiYami/Console-Note"
INSTALL_DIR="${CONSOLE_NOTE_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -m)" in
    x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
    *) echo "Unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
else
    echo "curl or wget is required" >&2
    exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ASSET="console_note-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download"

fetch "https://api.github.com/repos/${REPO}/releases/latest" "${TMP_DIR}/release.json"
TAG="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${TMP_DIR}/release.json" | head -n 1)"
if [ -z "$TAG" ]; then
    echo "Failed to determine latest release" >&2
    exit 1
fi

echo "Installing console_note ${TAG} -> ${INSTALL_DIR}"
fetch "${BASE_URL}/${TAG}/${ASSET}" "${TMP_DIR}/${ASSET}"
fetch "${BASE_URL}/${TAG}/${ASSET}.sha256" "${TMP_DIR}/${ASSET}.sha256"

EXPECTED="$(awk '{print $1}' "${TMP_DIR}/${ASSET}.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL="$(sha256sum "${TMP_DIR}/${ASSET}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    ACTUAL="$(shasum -a 256 "${TMP_DIR}/${ASSET}" | awk '{print $1}')"
else
    echo "sha256sum or shasum is required" >&2
    exit 1
fi
if [ "$EXPECTED" != "$ACTUAL" ]; then
    echo "Checksum mismatch: expected ${EXPECTED}, got ${ACTUAL}" >&2
    exit 1
fi

tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
mkdir -p "$INSTALL_DIR"
mv "${TMP_DIR}/console_note" "${INSTALL_DIR}/console_note"
chmod +x "${INSTALL_DIR}/console_note"

case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        echo ""
        echo "NOTE: ${INSTALL_DIR} is not on your PATH."
        echo 'Add this to your ~/.bashrc or ~/.zshrc:'
        echo "  export PATH=\"\$PATH:${INSTALL_DIR}\""
        ;;
esac

"${INSTALL_DIR}/console_note" --help
