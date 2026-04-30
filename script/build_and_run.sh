#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="Loomen"
APP_ID="dev.kohoj.loomen"
APP_VERSION="0.1.0"
PRODUCT_NAME="loomen"
EXECUTABLE="src-tauri/target/debug/${PRODUCT_NAME}"
BUNDLE_DIR="src-tauri/target/debug/bundle/macos/${APP_NAME}.app"
CONTENTS_DIR="${BUNDLE_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
LOG_FILE="${TMPDIR:-/tmp}/loomen.log"

pkill -x "${APP_NAME}" 2>/dev/null || true
pkill -f "src-tauri/target/debug/${PRODUCT_NAME}" 2>/dev/null || true
pkill -f "bun sidecar/index.ts" 2>/dev/null || true

cargo build --manifest-path src-tauri/Cargo.toml

rm -rf "${BUNDLE_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"
cp "${EXECUTABLE}" "${MACOS_DIR}/${APP_NAME}"
chmod +x "${MACOS_DIR}/${APP_NAME}"
cat >"${CONTENTS_DIR}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>${APP_NAME}</string>
  <key>CFBundleExecutable</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${APP_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${APP_VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${APP_VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST
printf 'APPL????' >"${CONTENTS_DIR}/PkgInfo"

if [[ "${1:-}" == "--verify" ]]; then
  /usr/bin/open -n "${BUNDLE_DIR}"
  sleep 3
  pid="$(pgrep -x "${APP_NAME}" | head -n 1 || true)"
  if [[ -n "${pid}" ]]; then
    echo "${APP_NAME} running with pid ${pid}"
    echo "bundle: ${BUNDLE_DIR}"
    echo "log: ${LOG_FILE}"
    exit 0
  fi
  echo "${APP_NAME} exited during launch"
  exit 1
fi

/usr/bin/open -n "${BUNDLE_DIR}"
