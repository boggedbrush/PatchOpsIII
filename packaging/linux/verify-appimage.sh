#!/usr/bin/env bash
# Static checks on a built AppImage (what linux-build.yml used to do inline).
#   verify-appimage.sh <PatchOpsIII.AppImage> [expected update information]
# Fails when: a SquashFS directory is not 0755 (breaks Firejail), AppRun is not executable, the
# embedded update information differs, the bundled binary needs a glibc newer than MAX_GLIBC
# (default 2.35 = Ubuntu 22.04, the build image), or a shared library is unresolved.
set -euo pipefail
image="$(readlink -f "${1:?usage: verify-appimage.sh <AppImage> [update-information]}")"
expected_update="${2:-}"
max_glibc="${MAX_GLIBC:-2.35}"
test -s "$image"

if [ -n "$expected_update" ]; then
  actual="$("$image" --appimage-updateinformation)"
  [ "$actual" = "$expected_update" ] || { printf 'Expected update information: %s\nFound: %s\n' "$expected_update" "$actual" >&2; exit 1; }
fi

listing="$(mktemp)"
unsquashfs -lln -o "$("$image" --appimage-offset)" "$image" > "$listing"
awk '$1 ~ /^d/ { found = 1; if ($1 != "drwxr-xr-x") { print "AppImage directory must have mode 0755: " $0 > "/dev/stderr"; bad = 1 } } END { if (!found || bad) exit 1 }' "$listing"
rm -f "$listing"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
(cd "$tmp" && "$image" --appimage-extract >/dev/null)
root="$tmp/squashfs-root"
test -x "$root/AppRun"
test -x "$root/usr/bin/patchops"
test -d "$root/usr/bin/bo3-enhanced-proton"
test -f "$root/usr/share/applications/PatchOpsIII.desktop"
test -f "$root/usr/share/icons/hicolor/512x512/apps/PatchOpsIII.png"

# Weak references (Rust std probes pidfd_spawnp & co. at runtime) do not fail the loader on old glibc; skip them.
worst="$(objdump -T "$root/usr/bin/patchops" | awk '$2 != "w"' | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -n 1)"
echo "highest GLIBC symbol version required: ${worst:-none} (limit $max_glibc)"
if [ -n "$worst" ] && [ "$(printf '%s\n%s\n' "$worst" "$max_glibc" | sort -V | tail -n 1)" != "$max_glibc" ]; then
  echo "binary needs glibc $worst > $max_glibc" >&2; exit 1
fi
if ldd "$root/usr/bin/patchops" | grep -q 'not found'; then
  ldd "$root/usr/bin/patchops" >&2; echo "unresolved shared libraries" >&2; exit 1
fi
echo "AppImage OK: $(du -h "$image" | cut -f1)"
