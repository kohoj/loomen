#!/usr/bin/env bash
set -euo pipefail

WORKSPACE_PATH="${PWD}"
ROOT_PATH="${LOOMEN_ROOT_PATH:-}"
INTERVAL="${LOOMEN_SPOTLIGHTER_INTERVAL:-2}"

if [[ -z "${ROOT_PATH}" ]]; then
  echo "LOOMEN_ROOT_PATH is required" >&2
  exit 2
fi

if [[ ! -d "${ROOT_PATH}/.git" ]]; then
  echo "LOOMEN_ROOT_PATH must point at a git repository" >&2
  exit 2
fi

copy_file() {
  local rel="$1"
  local src="${WORKSPACE_PATH}/${rel}"
  local dst="${ROOT_PATH}/${rel}"
  mkdir -p "$(dirname "${dst}")"
  if command -v rsync >/dev/null 2>&1; then
    rsync -a "${src}" "${dst}"
  else
    cp -p "${src}" "${dst}"
  fi
}

mirror_once() {
  git -C "${WORKSPACE_PATH}" ls-files -m -o -d --exclude-standard | sort -u | while IFS= read -r rel; do
    case "${rel}" in
      ""|/*|../*|*/../*|.git|.git/*)
        continue
        ;;
    esac

    if [[ -e "${WORKSPACE_PATH}/${rel}" ]]; then
      copy_file "${rel}"
    else
      rm -f "${ROOT_PATH}/${rel}"
    fi
  done
}

mirror_once
while true; do
  sleep "${INTERVAL}"
  mirror_once
done
