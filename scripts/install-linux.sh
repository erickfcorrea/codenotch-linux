#!/usr/bin/env bash
# Installs Codenotch for Linux into ~/.local without requiring root privileges.
set -euo pipefail

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/codenotch"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps"

CODENOTCH_BIN="${CODENOTCH_BIN:-$ROOT/target/release/codenotch}"
HOOK_BIN="${HOOK_BIN:-$ROOT/target/release/codenotch-hook}"

if [ ! -x "$CODENOTCH_BIN" ] || [ ! -x "$HOOK_BIN" ]; then
  echo "Release binaries not found. Run: cargo build --release" >&2
  exit 1
fi

mkdir -p "$APP_DIR" "$BIN_DIR" "$DESKTOP_DIR" "$ICON_DIR"

install -m 0755 "$CODENOTCH_BIN" "$APP_DIR/codenotch"
install -m 0755 "$HOOK_BIN" "$APP_DIR/codenotch-hook"
install -m 0755 "$ROOT/scripts/run-linux.sh" "$APP_DIR/run-linux.sh"
if [ -f "$ROOT/crates/codenotch/qoder-usage.cjs" ]; then
  install -m 0644 "$ROOT/crates/codenotch/qoder-usage.cjs" "$APP_DIR/qoder-usage.cjs"
fi
if [ -f "$ROOT/crates/codenotch/icons/128x128.png" ]; then
  install -m 0644 "$ROOT/crates/codenotch/icons/128x128.png" "$ICON_DIR/codenotch.png"
fi

cat > "$BIN_DIR/codenotch" <<EOF
#!/bin/sh
exec "$APP_DIR/run-linux.sh" "\$@"
EOF
chmod 0755 "$BIN_DIR/codenotch"
ln -sf "$APP_DIR/codenotch-hook" "$BIN_DIR/codenotch-hook"

cat > "$DESKTOP_DIR/codenotch.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Codenotch
GenericName=AI Usage Notch
Comment=Floating Linux usage notch for Claude, Codex, Cursor, Antigravity, Copilot & local LLMs
Exec=$APP_DIR/run-linux.sh
Icon=codenotch
Terminal=false
Categories=Utility;Development;
StartupNotify=false
EOF

echo "Installed Codenotch for Linux:"
echo "  App dir:     $APP_DIR"
echo "  CLI launcher: $BIN_DIR/codenotch"
echo "  Hook binary:  $BIN_DIR/codenotch-hook"
echo "  Desktop entry: $DESKTOP_DIR/codenotch.desktop"
