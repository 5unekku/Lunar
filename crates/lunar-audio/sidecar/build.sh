#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${SCRIPT_DIR}/build"
OUT_DIR="${1:-${SCRIPT_DIR}/dist}"

command -v emcmake >/dev/null || { echo "error: emcmake not on PATH (install emscripten)"; exit 1; }

emcmake cmake -B "${BUILD_DIR}" "${SCRIPT_DIR}" -DCMAKE_BUILD_TYPE=Release
cmake --build "${BUILD_DIR}" --config Release -j "$(nproc)"

mkdir -p "${OUT_DIR}"
cp "${BUILD_DIR}/audio_sidecar.js"   "${OUT_DIR}/"
cp "${BUILD_DIR}/audio_sidecar.wasm" "${OUT_DIR}/"
echo "done: ${OUT_DIR}/audio_sidecar.{js,wasm}"
