#!/usr/bin/env bash
# narumi をサブプロセス起動してから gaia を実行する開発用スクリプト。
# narumi は任意依存: NARUMI_BIN が未設定または実行不可ならスキップして続行する。
set -euo pipefail
if [[ -n "${NARUMI_BIN:-}" && -x "${NARUMI_BIN}" ]]; then
  "${NARUMI_BIN}" serve --stdio &
  NARUMI_PID=$!
  trap 'kill "${NARUMI_PID}" 2>/dev/null || true' EXIT
  echo "narumi started (pid ${NARUMI_PID})" >&2
else
  echo "narumi not found (NARUMI_BIN unset); continuing without it" >&2
fi
exec cargo run -p gaia -- "$@"
