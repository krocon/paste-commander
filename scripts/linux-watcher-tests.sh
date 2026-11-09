#!/usr/bin/env bash
# Runs the ignored clipboard-watcher integration tests against real display
# servers: Xvfb (X11) and a headless sway (Wayland data-control).
# Needs: xvfb, xclip, sway. Used by CI and the local Docker check.
set -euo pipefail
cd "$(dirname "$0")/../src-tauri"

echo "== X11 (Xvfb)"
xvfb-run -a cargo test --locked -- --ignored x11_

echo "== Wayland (headless sway)"
XDG_RUNTIME_DIR="$(mktemp -d)"
export XDG_RUNTIME_DIR
chmod 700 "$XDG_RUNTIME_DIR"
SWAY_ENV=(env XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman)
if [ "$(id -u)" = 0 ]; then
  # sway refuses to run as root (e.g. inside Docker); the root test process can
  # still connect to the socket of an unprivileged compositor.
  chown nobody "$XDG_RUNTIME_DIR"
  runuser -u nobody -- "${SWAY_ENV[@]}" sway -c /dev/null >/tmp/sway.log 2>&1 &
else
  "${SWAY_ENV[@]}" sway -c /dev/null >/tmp/sway.log 2>&1 &
fi
SWAY_PID=$!
trap 'kill $SWAY_PID 2>/dev/null || true' EXIT
for _ in $(seq 100); do
  socket="$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' | head -1)"
  [ -n "$socket" ] && break
  sleep 0.1
done
if [ -z "${socket:-}" ]; then
  echo "sway did not start:"; cat /tmp/sway.log; exit 1
fi
export WAYLAND_DISPLAY="$(basename "$socket")"
unset DISPLAY
cargo test --locked -- --ignored wayland_
