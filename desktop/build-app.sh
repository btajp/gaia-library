#!/usr/bin/env bash
# UI と同梱 CLI をビルドする。生成物はコミットしない。
set -euo pipefail

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"
if [[ -z "$TRIPLE" ]]; then
  echo "Rust の host triple を取得できません" >&2
  exit 1
fi

echo "UI をビルドします" >&2
(cd "$REPO_DIR/desktop/ui" && bun install --frozen-lockfile && bun run build)

echo "同梱 CLI をビルドします ($TRIPLE)" >&2
(cd "$REPO_DIR" && cargo build --release -p gaia --target "$TRIPLE" --target-dir "$REPO_DIR/target")

mkdir -p "$REPO_DIR/desktop/src-tauri/binaries"
cp "$REPO_DIR/target/$TRIPLE/release/gaia" "$REPO_DIR/desktop/src-tauri/binaries/gaia-$TRIPLE"
echo "同梱 CLI を配置しました: gaia-$TRIPLE" >&2
