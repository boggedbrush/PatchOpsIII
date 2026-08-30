#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'linux package CI: %s\n' "$*" >&2
  exit 1
}

debian_version() {
  local upstream="$1"

  if [[ "$upstream" =~ ^([0-9]+\.[0-9]+\.[0-9]+)-beta([1-9][0-9]*)$ ]]; then
    printf '%s~beta%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
  elif [[ "$upstream" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    printf '%s\n' "$upstream"
  else
    fail "unsupported upstream version: $upstream"
  fi
}

repack_deb() {
  local source="$1"
  local destination="$2"
  local version="$3"
  local work_root
  local control

  work_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/patchops-deb.XXXXXX")"
  dpkg-deb --raw-extract "$source" "$work_root/root"
  control="$work_root/root/DEBIAN/control"
  [[ "$(grep -c '^Version:' "$control")" -eq 1 ]] || fail "expected one Version field in $source"
  sed -i "s/^Version:.*/Version: $version/" "$control"
  grep -Fxq "Version: $version" "$control" || fail "could not rewrite Debian version"
  mkdir -p "$(dirname "$destination")"
  SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}" \
    dpkg-deb --root-owner-group --build "$work_root/root" "$destination" >/dev/null
}

canonicalize_deb() {
  [[ "$#" -eq 3 ]] || fail "usage: $0 canonicalize-deb INPUT OUTPUT UPSTREAM_VERSION"
  local source="$1"
  local destination="$2"
  local upstream="$3"
  local source_version
  local target_version
  local stable_version
  local beta_number

  [[ -f "$source" ]] || fail "Debian package not found: $source"
  source_version="$(dpkg-deb --field "$source" Version)"
  target_version="$(debian_version "$upstream")"
  if [[ "$source_version" != "$upstream" && "$source_version" != "$target_version" ]]; then
    fail "package version $source_version does not match upstream version $upstream"
  fi

  # Always rebuild the ar/tar containers under SOURCE_DATE_EPOCH. Copying an
  # already-canonical stable version would retain Tauri's build timestamps.
  repack_deb "$source" "$destination" "$target_version"

  [[ "$(dpkg-deb --field "$destination" Version)" == "$target_version" ]] ||
    fail "canonical package has the wrong version"

  if [[ "$target_version" =~ ^([0-9]+\.[0-9]+\.[0-9]+)~beta([1-9][0-9]*)$ ]]; then
    stable_version="${BASH_REMATCH[1]}"
    beta_number="${BASH_REMATCH[2]}"
    dpkg --compare-versions "$target_version" lt "$stable_version" ||
      fail "$target_version must sort before $stable_version"
    if ((beta_number > 1)); then
      dpkg --compare-versions "${stable_version}~beta$((beta_number - 1))" lt "$target_version" ||
        fail "beta versions do not sort in ascending order"
    fi
  fi
}

make_older_deb() {
  [[ "$#" -eq 2 ]] || fail "usage: $0 make-older-deb INPUT OUTPUT"
  local source="$1"
  local destination="$2"
  local current_version
  local older_version

  [[ -f "$source" ]] || fail "Debian package not found: $source"
  current_version="$(dpkg-deb --field "$source" Version)"
  older_version="${current_version}~ci1"
  dpkg --compare-versions "$older_version" lt "$current_version" ||
    fail "synthetic version $older_version does not sort before $current_version"
  repack_deb "$source" "$destination" "$older_version"
  [[ "$(dpkg-deb --field "$destination" Version)" == "$older_version" ]] ||
    fail "synthetic older package has the wrong version"
}

smoke_gui() {
  [[ "$#" -eq 2 ]] || fail "usage: $0 smoke-gui EXECUTABLE LOG"
  local executable="$1"
  local log="$2"
  local run_root
  local close_helper
  local child_pid=""
  local xvfb_pid=""
  local ready=0
  local status=0
  local -a command=("$executable")

  [[ -x "$executable" ]] || fail "executable not found: $executable"
  run_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/patchops-smoke.XXXXXX")"
  close_helper="$run_root/close-window"
  mkdir -p "$(dirname "$log")" "$run_root/runtime"
  chmod 0700 "$run_root/runtime"
  cc -O2 -Wall -Wextra -Werror benchmarks/close-window.c -lX11 -o "$close_helper"

  cleanup_smoke() {
    local exit_status=$?
    trap - EXIT INT TERM
    if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
      kill "$child_pid" 2>/dev/null || true
      wait "$child_pid" 2>/dev/null || true
    fi
    if [[ -n "$xvfb_pid" ]] && kill -0 "$xvfb_pid" 2>/dev/null; then
      kill "$xvfb_pid" 2>/dev/null || true
      wait "$xvfb_pid" 2>/dev/null || true
    fi
    rm -rf -- "$run_root"
    exit "$exit_status"
  }
  trap cleanup_smoke EXIT INT TERM

  if [[ -z "${DISPLAY:-}" ]]; then
    export DISPLAY=:99
    Xvfb "$DISPLAY" -screen 0 1280x800x24 -nolisten tcp >"${log}.xvfb" 2>&1 &
    xvfb_pid=$!
    for _ in {1..50}; do
      [[ -S /tmp/.X11-unix/X99 ]] && break
      kill -0 "$xvfb_pid" 2>/dev/null || fail "Xvfb exited before becoming ready"
      sleep 0.1
    done
    [[ -S /tmp/.X11-unix/X99 ]] || fail "Xvfb did not become ready"
  fi

  if [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && command -v dbus-run-session >/dev/null 2>&1; then
    command=(dbus-run-session -- "$executable")
  fi

  APPIMAGE_EXTRACT_AND_RUN=1 \
    GDK_BACKEND=x11 \
    PATCHOPSIII_BENCHMARK=1 \
    XDG_RUNTIME_DIR="$run_root/runtime" \
    "${command[@]}" >"$log" 2>&1 &
  child_pid=$!

  for _ in {1..200}; do
    if grep -Fq PATCHOPSIII_BENCHMARK_INTERACTIVE "$log"; then
      ready=1
      break
    fi
    if ! kill -0 "$child_pid" 2>/dev/null; then
      wait "$child_pid" || status=$?
      child_pid=""
      cat "$log"
      fail "application exited before becoming interactive (status $status)"
    fi
    sleep 0.1
  done
  [[ "$ready" -eq 1 ]] || {
    cat "$log"
    fail "interactive marker was not observed"
  }

  "$close_helper" PatchOpsIII
  for _ in {1..100}; do
    if ! kill -0 "$child_pid" 2>/dev/null; then
      wait "$child_pid" || status=$?
      child_pid=""
      cat "$log"
      [[ "$status" -eq 0 ]] || fail "application shutdown returned status $status"
      cleanup_smoke
    fi
    sleep 0.1
  done
  cat "$log"
  fail "application did not exit after WM_DELETE_WINDOW"
}

self_test() {
  [[ "$(debian_version 1.2.3)" == 1.2.3 ]]
  [[ "$(debian_version 1.2.3-beta4)" == 1.2.3~beta4 ]]
  if (debian_version 1.2.3-beta.4 >/dev/null 2>&1); then
    fail "invalid beta syntax was accepted"
  fi
  if command -v dpkg >/dev/null 2>&1; then
    dpkg --compare-versions 1.2.3~beta3 lt 1.2.3~beta4
    dpkg --compare-versions 1.2.3~beta4 lt 1.2.3
  fi
  if command -v dpkg-deb >/dev/null 2>&1; then
    local test_root
    local variant
    test_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/patchops-deb-self-test.XXXXXX")"
    for variant in first second; do
      mkdir -p "$test_root/$variant/DEBIAN" "$test_root/$variant/usr/share/patchopsiii"
      printf 'Package: patchopsiii-test\nVersion: 1.2.3\nArchitecture: amd64\nMaintainer: PatchOpsIII CI\nDescription: deterministic package fixture\n' \
        > "$test_root/$variant/DEBIAN/control"
      printf 'same payload\n' > "$test_root/$variant/usr/share/patchopsiii/payload.txt"
    done
    touch -d '@1000000000' "$test_root/first/DEBIAN/control" "$test_root/first/usr/share/patchopsiii/payload.txt"
    touch -d '@1100000000' "$test_root/second/DEBIAN/control" "$test_root/second/usr/share/patchopsiii/payload.txt"
    dpkg-deb --root-owner-group --build "$test_root/first" "$test_root/first.deb" >/dev/null
    dpkg-deb --root-owner-group --build "$test_root/second" "$test_root/second.deb" >/dev/null
    SOURCE_DATE_EPOCH=0 canonicalize_deb "$test_root/first.deb" "$test_root/first-canonical.deb" 1.2.3
    SOURCE_DATE_EPOCH=0 canonicalize_deb "$test_root/second.deb" "$test_root/second-canonical.deb" 1.2.3
    cmp -s "$test_root/first-canonical.deb" "$test_root/second-canonical.deb" ||
      fail "stable Debian canonicalization retained input timestamps"
    rm -rf -- "$test_root"
  fi
}

case "${1:-}" in
  canonicalize-deb)
    shift
    canonicalize_deb "$@"
    ;;
  make-older-deb)
    shift
    make_older_deb "$@"
    ;;
  smoke-gui)
    shift
    smoke_gui "$@"
    ;;
  self-test)
    shift
    [[ "$#" -eq 0 ]] || fail "usage: $0 self-test"
    self_test
    ;;
  *)
    fail "usage: $0 {canonicalize-deb|make-older-deb|smoke-gui|self-test} ..."
    ;;
esac
