#!/bin/bash
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
APPDIR="$(readlink -f "$SCRIPT_DIR/../..")"

# Prefer the bundled python-appimage runtime if present.
PYTHON_BIN="$APPDIR/opt/python3.11/bin/python3.11"
if [ ! -x "$PYTHON_BIN" ]; then
  PYTHON_BIN="$APPDIR/usr/bin/python3"
fi
if [ ! -x "$PYTHON_BIN" ]; then
  echo "Python interpreter not found inside AppDir" >&2
  exit 1
fi

PYTHON_ROOT="$(dirname "$(dirname "$PYTHON_BIN")")"
PYTHON_LIB_DIR="$PYTHON_ROOT/lib"
PY_SITE_DEFAULT="$PYTHON_LIB_DIR/python3.11/site-packages"

PY_SITE=""
if [ -d "$PYTHON_LIB_DIR" ]; then
  while IFS= read -r -d '' candidate; do
    PY_SITE="$candidate/site-packages"
    break
  done < <(find "$PYTHON_LIB_DIR" -maxdepth 1 -mindepth 1 -type d -name 'python3.*' -print0 | sort -z)
fi
if [ -z "$PY_SITE" ] && [ -d "$PY_SITE_DEFAULT" ]; then
  PY_SITE="$PY_SITE_DEFAULT"
fi

SHARE_DIR="$APPDIR/usr/share/patchopsiii"

export PYTHONHOME="$PYTHON_ROOT"
if [ -n "$PY_SITE" ]; then
  export PYTHONPATH="$PY_SITE:$SHARE_DIR"
else
  export PYTHONPATH="$SHARE_DIR"
fi

if [ -d "$PY_SITE/PySide6/Qt/plugins" ]; then
  export QT_QPA_PLATFORM_PLUGIN_PATH="$PY_SITE/PySide6/Qt/plugins"
  export QT_PLUGIN_PATH="$QT_QPA_PLATFORM_PLUGIN_PATH"
fi

if [ -f "$APPDIR/opt/_internal/certs.pem" ]; then
  export SSL_CERT_FILE="$APPDIR/opt/_internal/certs.pem"
fi

if [ -d "$APPDIR/usr/share/tcltk/tcl8.6" ]; then
  export TCL_LIBRARY="$APPDIR/usr/share/tcltk/tcl8.6"
fi
if [ -d "$APPDIR/usr/share/tcltk/tk8.6" ]; then
  export TK_LIBRARY="$APPDIR/usr/share/tcltk/tk8.6"
  export TKPATH="$TK_LIBRARY"
fi

LD_CANDIDATES=()
for candidate in "$APPDIR/lib" "$APPDIR/lib64" "$APPDIR/usr/lib" "$APPDIR/usr/lib64" "$PYTHON_ROOT/lib"; do
  if [ -d "$candidate" ]; then
    LD_CANDIDATES+=("$candidate")
  fi
done
if [ "${#LD_CANDIDATES[@]}" -gt 0 ]; then
  LD_JOINED="$(IFS=:; printf '%s' "${LD_CANDIDATES[*]}")"
  if [ -n "${LD_LIBRARY_PATH:-}" ]; then
    export LD_LIBRARY_PATH="$LD_JOINED:$LD_LIBRARY_PATH"
  else
    export LD_LIBRARY_PATH="$LD_JOINED"
  fi
fi

export PYTHONDONTWRITEBYTECODE=1
exec "$PYTHON_BIN" "$SHARE_DIR/main.py" "$@"
