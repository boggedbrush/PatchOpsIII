#!/usr/bin/env bash
set -euo pipefail

cleanup() {
  rm -rf linuxdeploy plugin-qt plugin-python plugin-appimage
}
trap cleanup EXIT

# 1) Nuitka standalone build
python -m pip install -U nuitka pyside6

NUITKA_ARGS=(
  "main.py"
  "--standalone"
  "--follow-imports"
  "--enable-plugin=pyside6"
  "--include-qt-plugins=sensible,platforms,platformthemes,iconengines,imageformats,tls"
  "--output-dir=build/nuitka"
  "--output-filename=__main__"
)

if [ -d "assets" ]; then
  NUITKA_ARGS+=("--include-data-dir=assets=assets")
fi
if [ -d "icons" ]; then
  NUITKA_ARGS+=("--include-data-dir=icons=icons")
fi
if [ -f "presets.json" ]; then
  NUITKA_ARGS+=("--include-data-files=presets.json=presets.json")
fi

python -m nuitka "${NUITKA_ARGS[@]}"

# 2) Construct AppDir
APPDIR=build/AppDir
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"

install -m755 packaging/appimage/AppRun "$APPDIR/AppRun"
install -m644 packaging/appimage/PatchOpsIII.desktop "$APPDIR/PatchOpsIII.desktop"
install -m644 packaging/appimage/PatchOpsIII.desktop "$APPDIR/usr/share/applications/PatchOpsIII.desktop"
install -m644 packaging/appimage/icons/patchopsiii.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/patchopsiii.png"
install -m644 packaging/appimage/icons/patchopsiii.png "$APPDIR/patchopsiii.png"

# Copy Nuitka output
cp -a build/nuitka/main.dist/* "$APPDIR/usr/"

# Prepare output directory
OUTPUT_DIR=dist/appimage
mkdir -p "$OUTPUT_DIR"
rm -f "$OUTPUT_DIR/PatchOpsIII.AppImage" "$OUTPUT_DIR/PatchOpsIII.AppImage.zsync"
rm -f PatchOpsIII.AppImage PatchOpsIII.AppImage.zsync
# Remove previous raw artifacts to avoid stale matches
rm -f PatchOpsIII-*.AppImage PatchOpsIII-*.AppImage.zsync

# 3) Fetch linuxdeploy toolchain
TOOLS_DIR=build/linuxdeploy-tools
mkdir -p "$TOOLS_DIR"

wget_if_missing() {
  local url="$1"
  local dest="$2"
  if [ ! -f "$dest" ]; then
    wget -q "$url" -O "$dest"
  fi
}

wget_if_missing "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" "$TOOLS_DIR/linuxdeploy-x86_64.AppImage"
wget_if_missing "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage" "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage"
wget_if_missing "https://github.com/niess/linuxdeploy-plugin-python/releases/download/continuous/linuxdeploy-plugin-python-x86_64.AppImage" "$TOOLS_DIR/linuxdeploy-plugin-python-x86_64.AppImage"
wget_if_missing "https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage" "$TOOLS_DIR/linuxdeploy-plugin-appimage-x86_64.AppImage"
chmod +x "$TOOLS_DIR"/linuxdeploy-*.AppImage
export PATH="$TOOLS_DIR:$PATH"
export APPIMAGE_EXTRACT_AND_RUN=1

# 4) Embed update metadata + build
export UPDATE_INFORMATION="gh-releases-zsync|boggedbrush|PatchOpsIII|latest|PatchOpsIII.AppImage.zsync"
export LDAI_UPDATE_INFORMATION="$UPDATE_INFORMATION"

LINUXDEPLOY_CMD=("$TOOLS_DIR/linuxdeploy-x86_64.AppImage" --appdir "$APPDIR")

if command -v qmake >/dev/null 2>&1; then
  LINUXDEPLOY_CMD+=(--plugin qt)
elif command -v qmake6 >/dev/null 2>&1; then
  export QMAKE="$(command -v qmake6)"
  LINUXDEPLOY_CMD+=(--plugin qt)
else
  echo "Warning: qmake not found; skipping linuxdeploy qt plugin." >&2
fi

LINUXDEPLOY_CMD+=(--output appimage)

"${LINUXDEPLOY_CMD[@]}"

RAW_APPIMAGE=$(ls -1t PatchOpsIII-*.AppImage 2>/dev/null | head -n1 || true)
if [ -z "$RAW_APPIMAGE" ] || [ ! -f "$RAW_APPIMAGE" ]; then
  echo "Failed to locate linuxdeploy output AppImage." >&2
  exit 1
fi
mv "$RAW_APPIMAGE" "$OUTPUT_DIR/PatchOpsIII.AppImage"
chmod +x "$OUTPUT_DIR/PatchOpsIII.AppImage"

RAW_ZSYNC="${RAW_APPIMAGE}.zsync"
if [ -f "$RAW_ZSYNC" ]; then
  mv "$RAW_ZSYNC" "$OUTPUT_DIR/PatchOpsIII.AppImage.zsync"
fi

echo "AppImage created at $OUTPUT_DIR/PatchOpsIII.AppImage"
if [ -f "$OUTPUT_DIR/PatchOpsIII.AppImage.zsync" ]; then
  echo "zsync created at $OUTPUT_DIR/PatchOpsIII.AppImage.zsync"
fi

rm -rf linuxdeploy plugin-qt plugin-python plugin-appimage
