#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'linux package CI: %s\n' "$*" >&2
  exit 1
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

case "${1:-}" in
  smoke-gui)
    shift
    smoke_gui "$@"
    ;;
  *)
    fail "usage: $0 smoke-gui EXECUTABLE LOG"
    ;;
esac
