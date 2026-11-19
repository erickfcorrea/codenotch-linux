#!/bin/sh
# Codenotch for Linux launcher.
#
# Forces X11 (via XWayland on Wayland sessions) so the floating notch can anchor cleanly
# to any monitor edge, and strips snap-injected GTK/glibc variables when invoked from a snap.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -x "${CODENOTCH_BIN:-}" ]; then
  bin="$CODENOTCH_BIN"
elif [ -x "$here/codenotch" ]; then
  bin="$here/codenotch"
elif [ -x "$here/../target/release/codenotch" ]; then
  bin="$here/../target/release/codenotch"
elif [ -x "$here/../target/debug/codenotch" ]; then
  bin="$here/../target/debug/codenotch"
elif [ -x "$HOME/.local/share/codenotch/codenotch" ]; then
  bin="$HOME/.local/share/codenotch/codenotch"
else
  echo "Codenotch binary not found. Build first with: cargo build --release -p codenotch -p codenotch-hook" >&2
  exit 1
fi

exec env -u LD_LIBRARY_PATH -u GTK_PATH -u GIO_MODULE_DIR -u GSETTINGS_SCHEMA_DIR \
         -u LOCPATH -u GDK_PIXBUF_MODULE_FILE -u GDK_PIXBUF_MODULEDIR \
         GDK_BACKEND=x11 \
    "$bin" "$@"
