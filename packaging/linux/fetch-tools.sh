#!/usr/bin/env bash
# Downloads the pinned AppImage packaging tools into a directory and verifies their SHA-256.
#   fetch-tools.sh <dir>   -> <dir>/appimagetool-x86_64.AppImage and <dir>/runtime-x86_64
# Bump a pin by changing the version AND the checksum together.
set -euo pipefail

APPIMAGETOOL_VERSION="1.9.1"
APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"
RUNTIME_VERSION="20251108"
RUNTIME_SHA256="2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"

dir="${1:?usage: fetch-tools.sh <dir>}"
mkdir -p "$dir"

fetch() { # url dest sha256
  local url="$1" dest="$2" sum="$3"
  if [ -f "$dest" ] && echo "$sum  $dest" | sha256sum --check --status; then return 0; fi
  curl --fail --location --retry 3 --silent --show-error "$url" --output "$dest.part"
  echo "$sum  $dest.part" | sha256sum --check --status || { echo "checksum mismatch for $url" >&2; rm -f "$dest.part"; exit 1; }
  mv "$dest.part" "$dest"
}

fetch "https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-x86_64.AppImage" \
      "$dir/appimagetool-x86_64.AppImage" "$APPIMAGETOOL_SHA256"
fetch "https://github.com/AppImage/type2-runtime/releases/download/${RUNTIME_VERSION}/runtime-x86_64" \
      "$dir/runtime-x86_64" "$RUNTIME_SHA256"
chmod +x "$dir/appimagetool-x86_64.AppImage" "$dir/runtime-x86_64"
