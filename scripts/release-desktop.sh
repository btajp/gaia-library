#!/usr/bin/env bash
# gaia-library の署名・公証・updater 生成物・GitHub Release を一括検証する。
# push 済みの clean main のみ。実行すると最後に公開するため、実機 E2E 完了後に使う。
# 既存の秘密鍵は生成・上書きしない。SBOM/provenance は v0.1 の対象外。
set +x
set +a
set -euo pipefail
# 呼び出し元で export 済みでも、最初の子プロセスより前に継承を止める。
export -n APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
export -n TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
umask 077

usage() {
  printf '%s\n' "使い方: $0 <X.Y.Z> [--allow-pubkey-rotation] [--env-file <absolute-path>]"
  printf '%s\n' 'release.env の既定: 環境変数 GAIA_LIBRARY_RELEASE_ENV または ~/.config/gaia-library/release.env'
}
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

VERSION=""
ALLOW_PUBKEY_ROTATION=false
ENV_OVERRIDE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-pubkey-rotation) ALLOW_PUBKEY_ROTATION=true; shift ;;
    --env-file)
      [[ $# -ge 2 && -n "$2" ]] || { usage >&2; exit 2; }
      ENV_OVERRIDE="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    --*) usage >&2; exit 2 ;;
    *)
      [[ -z "$VERSION" ]] || { usage >&2; exit 2; }
      VERSION="$1"; shift ;;
  esac
done
[[ "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] \
  || { usage >&2; exit 2; }
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] \
  || die "リリースは Apple Silicon の macOS で実行してください"

REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd -P)"
readonly REPO_DIR VERSION
ENV_FILE="${ENV_OVERRIDE:-${GAIA_LIBRARY_RELEASE_ENV:-$HOME/.config/gaia-library/release.env}}"
[[ "$ENV_FILE" == /* && "$ENV_FILE" != */ ]] || die "release.env はファイルの絶対パスで指定してください"
mkdir -p "$(dirname -- "$ENV_FILE")"
ENV_FILE="$(cd -- "$(dirname -- "$ENV_FILE")" && pwd -P)/$(basename -- "$ENV_FILE")"
case "$ENV_FILE" in "$REPO_DIR"/*) die "release.env はリポジトリの外に置いてください" ;; esac
readonly ENV_FILE
# shellcheck source=scripts/lib/release-env.sh
source "$REPO_DIR/scripts/lib/release-env.sh"
load_release_environment "$ENV_FILE"

# 長い検証や公開の前に、既存ツールと pin 版を確認する。自動インストールはしない。
for TOOL in git gh bun cargo rustc codesign xcrun spctl plutil shasum tar lipo; do
  command -v "$TOOL" >/dev/null 2>&1 || die "前提ツール $TOOL がありません。既存の導入方針と toolchain.json を確認してください"
done
METADATA="$REPO_DIR/scripts/lib/release-metadata.mjs"
PINS="$(bun "$METADATA" pins "$REPO_DIR")"
BUN_PIN="${PINS%%$'\n'*}"
TAURI_PIN="${PINS#*$'\n'}"
[[ "$(bun --version)" == "$BUN_PIN" ]] || die "Bun が toolchain.json の版と一致しません"
[[ "$(cargo tauri --version)" == "tauri-cli $TAURI_PIN" ]] || die "Tauri CLI が toolchain.json の版と一致しません"
[[ "$(rustc -vV | awk '/^host:/ {print $2}')" == aarch64-apple-darwin ]] || die "Rust の host は aarch64-apple-darwin が必要です"
gh auth status >/dev/null 2>&1 || die "GitHub CLI の認証を確認してください"
xcrun --find notarytool >/dev/null 2>&1 || die "notarytool がありません"
xcrun --find stapler >/dev/null 2>&1 || die "stapler がありません"

assert_clean_worktree() {
  local status
  status="$(git -C "$REPO_DIR" status --porcelain --untracked-files=all)" || die "作業ツリーを確認できません"
  [[ -z "$status" ]] \
    || die "作業ツリーに未コミットの変更があります（lockfile の変化も許可しません）"
}
assert_remote_head() {
  [[ "$(git -C "$REPO_DIR" branch --show-current)" == main ]] || die "リリースは main ブランチに限ります"
  assert_clean_worktree
  git -C "$REPO_DIR" fetch --quiet origin main:refs/remotes/origin/main --tags
  [[ "$(git -C "$REPO_DIR" rev-parse HEAD)" == "$HEAD_SHA" ]] || die "ビルド開始後に HEAD が変わりました"
  [[ "$HEAD_SHA" == "$(git -C "$REPO_DIR" rev-parse origin/main)" ]] || die "main が origin/main と一致しません"
  local remote_tag
  remote_tag="$(git -C "$REPO_DIR" ls-remote --tags origin "refs/tags/v$VERSION")" || die "リモートタグを確認できません"
  [[ -z "$remote_tag" ]] || die "対象タグは既にリモートに存在します"
  if git -C "$REPO_DIR" show-ref --verify --quiet "refs/tags/v$VERSION"; then
    die "対象タグは既にローカルに存在します。タグはリリース処理で作成します"
  fi
}

ORIGIN_URL="$(git -C "$REPO_DIR" remote get-url origin)"
case "$ORIGIN_URL" in
  https://github.com/btajp/gaia-library|https://github.com/btajp/gaia-library.git|git@github.com:btajp/gaia-library|git@github.com:btajp/gaia-library.git|ssh://git@github.com/btajp/gaia-library|ssh://git@github.com/btajp/gaia-library.git) ;;
  *) die "origin が btajp/gaia-library を指していません" ;;
esac
HEAD_SHA="$(git -C "$REPO_DIR" rev-parse HEAD)"
readonly HEAD_SHA
assert_remote_head
bun "$METADATA" verify "$REPO_DIR" "$VERSION"

KEY_ARGS=(--repo "$REPO_DIR" --private-key "$TAURI_SIGNING_PRIVATE_KEY")
if [[ "$ALLOW_PUBKEY_ROTATION" == true ]]; then KEY_ARGS+=(--allow-pubkey-rotation); fi
UPDATER_SIGNATURE_PUBKEY="$("$REPO_DIR/scripts/check-updater-key-policy.sh" "${KEY_ARGS[@]}")"
printf 'gaia-library v%s のリリース検証を開始します\n' "$VERSION"

# core / CLI と desktop の型・lint・テストを通す。同梱 CLI は build-app.sh が生成する。
(cd "$REPO_DIR" && cargo build --locked --workspace)
(cd "$REPO_DIR" && cargo fmt --all --check)
(cd "$REPO_DIR" && cargo clippy --locked --workspace --all-targets -- -D warnings)
(cd "$REPO_DIR" && cargo test --locked --workspace)
(cd "$REPO_DIR" && bun test scripts)
"$REPO_DIR/desktop/build-app.sh"
(cd "$REPO_DIR/desktop/ui" && bun test)
assert_clean_worktree
(cd "$REPO_DIR/desktop/src-tauri" && cargo build --all-features --locked)
(cd "$REPO_DIR/desktop/src-tauri" && cargo fmt --all --check)
(cd "$REPO_DIR/desktop/src-tauri" && cargo clippy --locked --all-targets --all-features -- -D warnings)
(cd "$REPO_DIR/desktop/src-tauri" && cargo test --all-features --locked)
assert_clean_worktree

# 通常の ad-hoc 署名設定より、Developer ID を優先する一時 overlay を明示的に渡す。
# updater artifact overlay を土台とし、本番設定・鍵ファイルは変更しない。
RELEASE_TEMP="$(mktemp -d /private/tmp/gaia-release.XXXXXX)"
RELEASE_OVERLAY="$RELEASE_TEMP/release.conf.json"
create_release_signing_overlay "$METADATA" "$REPO_DIR" "$VERSION" "$RELEASE_OVERLAY"
build_signed_desktop "$REPO_DIR/desktop/src-tauri" "$RELEASE_OVERLAY"
assert_clean_worktree

BUNDLE_DIR="$REPO_DIR/desktop/src-tauri/target/release/bundle"
APP="$BUNDLE_DIR/macos/gaia-library.app"
DMG="$BUNDLE_DIR/dmg/gaia-library_${VERSION}_aarch64.dmg"
TARGZ="$BUNDLE_DIR/macos/gaia-library.app.tar.gz"
SIG="$TARGZ.sig"
for ARTIFACT in "$APP" "$DMG" "$TARGZ" "$SIG"; do
  [[ -e "$ARTIFACT" ]] || die "必要な生成物がありません。Tauri build の結果を確認してください"
done
for EXECUTABLE in gaia-desktop gaia; do
  [[ -x "$APP/Contents/MacOS/$EXECUTABLE" ]] || die "主アプリまたは同梱 CLI がありません"
  [[ "$(lipo -archs "$APP/Contents/MacOS/$EXECUTABLE")" == arm64 ]] || die "配布バイナリが arm64 ではありません"
done
[[ ! -e "$APP/Contents/MacOS/verify-updater-signature" && ! -L "$APP/Contents/MacOS/verify-updater-signature" ]] \
  || die "検証用 CLI が配布アプリに混入しています。updater-verifier feature を無効にしてビルドしてください"
[[ "$("$APP/Contents/MacOS/gaia" --version)" == "gaia $VERSION" ]] || die "同梱 CLI の版数が一致しません"
[[ "$(plutil -extract CFBundleShortVersionString raw -o - "$APP/Contents/Info.plist")" == "$VERSION" ]] \
  || die "生成した .app の版数が一致しません"
[[ "$(tar -xOf "$TARGZ" gaia-library.app/Contents/Info.plist | plutil -extract CFBundleShortVersionString raw -o - -)" == "$VERSION" ]] \
  || die "updater archive の版数が一致しません"

printf '%s\n' 'Developer ID 署名・公証・updater 署名を検証します'
codesign --verify --deep --strict "$APP"
APP_AUTHORITY="$(codesign -dv --verbose=4 "$APP" 2>&1 | awk '/^Authority=/ && !seen++ {sub(/^Authority=/, ""); print}')"
[[ "$APP_AUTHORITY" == "$APPLE_SIGNING_IDENTITY" ]] || die "生成した .app の署名者が Developer ID の指定と一致しません"
xcrun stapler validate "$APP"
spctl -a -t exec -vv "$APP"
(cd "$REPO_DIR/desktop/src-tauri" && cargo run --locked --quiet --features updater-verifier --bin verify-updater-signature -- "$TARGZ" "$SIG" "$UPDATER_SIGNATURE_PUBKEY")

# DMG 自体も署名・公証・staple し、その後で checksum を計算する。
codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$DMG"
codesign --verify --strict "$DMG"
xcrun notarytool submit "$DMG" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
  --issuer "$APPLE_API_ISSUER" --wait --output-format json > "$RELEASE_TEMP/notary-result.json"
bun "$METADATA" notary "$REPO_DIR" "$VERSION" "$RELEASE_TEMP/notary-result.json"
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
spctl -a -t open --context context:primary-signature -vv "$DMG"

STAGING="$RELEASE_TEMP/assets"
mkdir "$STAGING"
cp "$DMG" "$TARGZ" "$SIG" "$STAGING/"
bun "$METADATA" assets "$REPO_DIR" "$VERSION" "$STAGING"
(cd "$STAGING" && shasum -a 256 "gaia-library_${VERSION}_aarch64.dmg" \
  gaia-library.app.tar.gz gaia-library.app.tar.gz.sig latest.json > checksums.txt)

# 検証中の変更やタグ作成を最後にも検出し、全添付がある draft だけを公開する。
assert_remote_head
[[ "$("$REPO_DIR/scripts/check-updater-key-policy.sh" "${KEY_ARGS[@]}")" == "$UPDATER_SIGNATURE_PUBKEY" ]] \
  || die "検証中に updater 鍵の前提が変わりました"
if ! gh release create "v$VERSION" --repo btajp/gaia-library --draft --target "$HEAD_SHA" \
  --title "v$VERSION" --notes-file "$STAGING/release-notes.md" \
  "$STAGING/gaia-library_${VERSION}_aarch64.dmg" "$STAGING/gaia-library.app.tar.gz" \
  "$STAGING/gaia-library.app.tar.gz.sig" "$STAGING/latest.json" "$STAGING/checksums.txt"; then
  die "draft 作成に失敗しました。途中の draft / タグを確認してください（自動削除はしません）"
fi
gh release view "v$VERSION" --repo btajp/gaia-library --json isDraft,targetCommitish,assets \
  | bun "$METADATA" draft "$REPO_DIR" "$VERSION" "$HEAD_SHA"
gh release edit "v$VERSION" --repo btajp/gaia-library --draft=false
printf '公開しました: https://github.com/btajp/gaia-library/releases/tag/v%s\n' "$VERSION"
printf '検証済み生成物と公証結果は %s に残しています。\n' "$RELEASE_TEMP"
printf '%s\n' '未実施の事後スモーク: DMG の実ダウンロード・導入・警告なしの起動、旧版からの更新・再起動・HTTP 再開。'
