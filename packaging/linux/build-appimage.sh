#!/usr/bin/env bash
# Builds PatchOpsIII.AppImage from the release binary.
#
#   cargo build --release --locked -p patchops
#   packaging/linux/build-appimage.sh
#
# Output (default dist/packages): PatchOpsIII.AppImage, plus PatchOpsIII.AppImage.zsync.
# Environment:
#   BINARY              release binary                         (default target/release/patchops)
#   OUT_DIR             output directory                       (default dist/packages)
#   WORK_DIR            scratch directory (AppDir lives here)  (default target/appimage)
#   UPDATE_INFORMATION  AppImageUpdate string to embed, e.g.
#                       gh-releases-zsync|boggedbrush|PatchOpsIII|latest|PatchOpsIII.AppImage.zsync
#                       (empty: no update information, and a standalone .zsync is generated instead)
#   APPIMAGETOOL / APPIMAGE_RUNTIME   paths to the tools; default: downloaded + checksum-verified
#                       into $WORK_DIR/tools by fetch-tools.sh
#   APPDIR_ONLY=1       stop after assembling the AppDir (no appimagetool; used to test offline)
# Needs: patchelf, and for the final image squashfs-tools (mksquashfs) + zsync (zsyncmake).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
cd "$repo"

BINARY="${BINARY:-target/release/patchops}"
OUT_DIR="${OUT_DIR:-dist/packages}"
WORK_DIR="${WORK_DIR:-target/appimage}"
UPDATE_INFORMATION="${UPDATE_INFORMATION:-}"
APPDIR="$WORK_DIR/PatchOpsIII.AppDir"
NAME="PatchOpsIII"

[ -x "$BINARY" ] || { echo "release binary not found: $BINARY (cargo build --release --locked -p patchops)" >&2; exit 1; }
command -v patchelf >/dev/null || { echo "patchelf is required (apt-get install patchelf)" >&2; exit 1; }

# Shared libraries that are bundled. Policy:
#   * glibc/libgcc/libm and the X protocol stack (libxcb, libXau, libXdmcp, libbsd, libmd) come from the host
#     (they are on the AppImage "excludelist" for the same reason: the host must match its own display server).
#   * the xkbcommon trio is bundled: tiny, ABI-stable, and the one thing a minimal / immutable image
#     (Steam Deck, containers) can plausibly lack without having a display stack problem.
#   * libvulkan, GPU drivers / ICD files, Wayland client libs are NOT bundled. GPUI's renderer dlopens the
#     host's libvulkan.so.1 and must pair it with the host's driver; a bundled loader could not find the
#     host's ICD manifests reliably. Steam Deck (RADV) and every desktop distro ship it.
BUNDLE_LIBS=(libxkbcommon.so.0 libxkbcommon-x11.so.0 libxcb-xkb.so.1)

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" "$APPDIR/usr/share/applications"

install -m 755 "$BINARY" "$APPDIR/usr/bin/patchops"

# Bundled libs: resolve through ldd (the soname -> path the build machine uses).
for lib in "${BUNDLE_LIBS[@]}"; do
  path="$(ldd "$BINARY" | awk -v l="$lib" '$1 == l { print $3 }')"
  if [ -z "$path" ]; then
    # Indirect dependency (not in the binary's NEEDED list): look it up the way the loader would.
    path="$(ldconfig -p | awk -v l="$lib" '$1 == l { print $NF; exit }')"
  fi
  [ -n "$path" ] && [ -f "$path" ] || { echo "cannot locate $lib on the build machine" >&2; exit 1; }
  install -m 644 "$(readlink -f "$path")" "$APPDIR/usr/lib/$lib"
  patchelf --set-rpath '$ORIGIN' "$APPDIR/usr/lib/$lib"
done
patchelf --set-rpath '$ORIGIN/../lib' "$APPDIR/usr/bin/patchops"

# The core looks for the Enhanced Proton tool "next to the executable": usr/bin/bo3-enhanced-proton/BO3 Enhanced
# (crates/patchops-core/src/enhanced_net.rs::existing_tool_source). The repo only carries metadata; an
# optional local payload (bo3-enhanced-proton/BO3 Enhanced, gitignored, development/offline builds) is
# copied along when present, otherwise the app downloads the GDK-Proton tool into its cache on demand.
mkdir -p "$APPDIR/usr/bin/bo3-enhanced-proton"
cp -a bo3-enhanced-proton/. "$APPDIR/usr/bin/bo3-enhanced-proton/"

# Desktop entry + icons. The icon name, desktop file name and StartupWMClass all equal the window app_id.
install -m 644 "$here/PatchOpsIII.desktop" "$APPDIR/usr/share/applications/$NAME.desktop"
install -m 644 "$here/PatchOpsIII.desktop" "$APPDIR/$NAME.desktop"
for size in 512 256 128 64 48; do
  d="$APPDIR/usr/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$d"
  if [ "$size" = 512 ]; then
    install -m 644 packaging/assets/PatchOpsIII-512.png "$d/$NAME.png"
  elif command -v convert >/dev/null; then
    convert packaging/assets/PatchOpsIII-512.png -resize "${size}x${size}" "$d/$NAME.png"
    chmod 644 "$d/$NAME.png"
  else
    rmdir "$d"
  fi
done
install -m 644 packaging/assets/PatchOpsIII-512.png "$APPDIR/$NAME.png"
ln -s "$NAME.png" "$APPDIR/.DirIcon"
install -m 755 "$here/AppRun" "$APPDIR/AppRun"

# Firejail (v1.3.4 fix): every directory in the image must be world-readable/traversable. Normalise the
# AppDir before squashing so the stored SquashFS directories are 0755 whatever the build umask was.
find "$APPDIR" -type d -exec chmod 755 {} +
find "$APPDIR" -type f ! -perm -u+x -exec chmod 644 {} +
find "$APPDIR" -type f -perm -u+x -exec chmod 755 {} +

echo "AppDir ready: $APPDIR ($(du -sh "$APPDIR" | cut -f1))"
if [ "${APPDIR_ONLY:-0}" = 1 ]; then exit 0; fi

if [ -z "${APPIMAGETOOL:-}" ] || [ -z "${APPIMAGE_RUNTIME:-}" ]; then
  "$here/fetch-tools.sh" "$WORK_DIR/tools"
  APPIMAGETOOL="${APPIMAGETOOL:-$PWD/$WORK_DIR/tools/appimagetool-x86_64.AppImage}"
  APPIMAGE_RUNTIME="${APPIMAGE_RUNTIME:-$PWD/$WORK_DIR/tools/runtime-x86_64}"
fi

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
APPDIR="$(cd "$APPDIR" && pwd)"
out="$OUT_DIR/$NAME.AppImage"
rm -f "$out" "$out.zsync"
args=(--runtime-file "$APPIMAGE_RUNTIME")
[ -n "$UPDATE_INFORMATION" ] && args+=(--updateinformation "$UPDATE_INFORMATION")
# APPIMAGE_EXTRACT_AND_RUN: appimagetool is itself an AppImage; CI runners and containers have no FUSE.
# Run inside OUT_DIR: with --updateinformation appimagetool writes the .zsync into its working directory.
(cd "$OUT_DIR" && ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" "${args[@]}" "$APPDIR" "$out")
chmod +x "$out"
# With update information appimagetool writes the .zsync itself; otherwise produce one (legacy behaviour).
if [ ! -f "$out.zsync" ]; then
  (cd "$OUT_DIR" && zsyncmake -u "$NAME.AppImage" -o "$NAME.AppImage.zsync" "$NAME.AppImage")
fi
ls -l "$OUT_DIR"
