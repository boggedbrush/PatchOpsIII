#!/bin/bash
set -euo pipefail
SCRIPT_DIR="$(dirname "$(readlink -f "$0")")"
APPDIR="$(readlink -f "$SCRIPT_DIR/..")"
PYTHON_BIN="$APPDIR/bin/python3"
if [ ! -x "$PYTHON_BIN" ]; then
  echo "Python interpreter not found at $PYTHON_BIN" >&2
  exit 1
fi
PY_SITE=""
while IFS= read -r -d '' candidate; do
  PY_SITE="$candidate/site-packages"
  break
done < <(find "$APPDIR/lib" -maxdepth 1 -mindepth 1 -type d -name 'python3.*' -print0 | sort -z)
if [ -z "$PY_SITE" ]; then
  PY_SITE="$APPDIR/lib/python3.11/site-packages"
fi
SHARE_DIR="$APPDIR/share/patchopsiii"
export PYTHONHOME="$APPDIR"
export PYTHONPATH="$PY_SITE:$SHARE_DIR"
if [ -d "$PY_SITE/PySide6/Qt/plugins" ]; then
  export QT_QPA_PLATFORM_PLUGIN_PATH="$PY_SITE/PySide6/Qt/plugins"
  export QT_PLUGIN_PATH="$QT_QPA_PLATFORM_PLUGIN_PATH"
fi
LD_CANDIDATES=()
if [ -d "$APPDIR/lib" ]; then
  LD_CANDIDATES+=("$APPDIR/lib")
fi
if [ -d "$APPDIR/lib64" ]; then
  LD_CANDIDATES+=("$APPDIR/lib64")
fi
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
