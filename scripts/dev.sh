#!/usr/bin/env bash
# narumi の HTTP server をサブプロセス起動してから gaia を実行する開発用スクリプト。
# narumi は任意依存: NARUMI_BIN が未設定または実行不可ならスキップして続行する。
set -euo pipefail
if [[ -n "${NARUMI_BIN:-}" && -x "${NARUMI_BIN}" ]]; then
  "${NARUMI_BIN}" --http --port "${NARUMI_PORT:-8765}" &
  NARUMI_PID=$!
  cleanup() {
    kill "${NARUMI_PID}" 2>/dev/null || true
    wait "${NARUMI_PID}" 2>/dev/null || true
  }
  trap cleanup EXIT
  echo "narumi HTTP server started (pid ${NARUMI_PID}, port ${NARUMI_PORT:-8765})" >&2
else
  echo "narumi not found (NARUMI_BIN unset); continuing without it" >&2
fi
cargo run -p gaia -- "$@"
