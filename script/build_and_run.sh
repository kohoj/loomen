#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="loomen"
LOG_FILE="${TMPDIR:-/tmp}/loomen.log"

pkill -f "src-tauri/target/debug/${APP_NAME}" 2>/dev/null || true
pkill -f "bun sidecar/index.ts" 2>/dev/null || true

cargo build --manifest-path src-tauri/Cargo.toml

if [[ "${1:-}" == "--verify" ]]; then
  "src-tauri/target/debug/${APP_NAME}" >"$LOG_FILE" 2>&1 &
  pid=$!
  sleep 3
  if ps -p "$pid" >/dev/null 2>&1; then
    echo "${APP_NAME} running with pid ${pid}"
    echo "log: ${LOG_FILE}"
    exit 0
  fi
  echo "${APP_NAME} exited during launch"
  sed -n '1,160p' "$LOG_FILE"
  exit 1
fi

exec "src-tauri/target/debug/${APP_NAME}"
