#!/usr/bin/env bash
# release-desktop.sh 専用。資格情報の内容は表示しない。

keep_release_credentials_local() {
  # 呼び出し元の export と、release.env の set -a / export の両方を解除する。
  set +a
  export -n APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
  export -n TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
}

create_release_signing_overlay() {
  # overlay の生成に必要なのは非秘密の Developer ID 名だけ。
  APPLE_SIGNING_IDENTITY="$APPLE_SIGNING_IDENTITY" bun "$1" overlay "$2" "$3" "$4"
}

build_signed_desktop() (
  # このサブシェル内の Tauri ビルドだけに署名・公証の資格情報を渡す。
  export APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
  export TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  cd "$1" || exit
  CI=true cargo tauri build --config "$2"
)

assert_private_file() {
  local path="$1" label="$2" mode owner
  [[ -f "$path" && -r "$path" && ! -L "$path" ]] \
    || die "$label は読み取り可能な通常ファイルを指定してください（symlink は不可）"
  mode="$(stat -f '%Lp' "$path" 2>/dev/null)" || die "$label の権限を確認できません"
  owner="$(stat -f '%u' "$path" 2>/dev/null)" || die "$label の所有者を確認できません"
  [[ "$owner" == "$(id -u)" ]] || die "$label の所有者が実行ユーザーではありません"
  [[ "$mode" =~ ^[0-7]{3,4}$ ]] || die "$label の権限を確認できません"
  (( (8#$mode & 077) == 0 )) || die "$label は実行ユーザー以外が読めない権限にしてください（0600 推奨）"
}

load_release_environment() {
  set +x
  keep_release_credentials_local
  local env_file="$1" name source_status=0
  if [[ ! -e "$env_file" && ! -L "$env_file" ]]; then
    # noclobber と umask を生成時に指定し、既存ファイルや鍵を上書きしない。
    (umask 077; set -o noclobber; cat > "$env_file" <<'TEMPLATE'
# gaia-library リリース用。リポジトリ外に置き、0600 を維持する。
# security find-identity に表示される Developer ID Application の名前。
APPLE_SIGNING_IDENTITY=""
# App Store Connect API の Key ID、Issuer ID、既存の .p8 ファイル。
APPLE_API_KEY=""
APPLE_API_ISSUER=""
APPLE_API_KEY_PATH=""
# 既存の updater 秘密鍵。隣に同名の .pub ファイルも必要。
# このスクリプトは鍵を生成しない。既存鍵の上書きは禁止。
TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/gaia-library-updater.key"
# 鍵にパスワードがなければ空欄のままでよい。
TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
TEMPLATE
    ) || die "release.env のテンプレートを安全に作成できません"
    printf 'release.env のテンプレートを 0600 で作成しました: %s\n' "$env_file" >&2
    die "資格情報の設定が必要です。リリース処理は実行していません"
  fi
  assert_private_file "$env_file" release.env
  # 信頼済みのローカル設定だけを読み込む。echo や xtrace による漏えいも抑制する。
  {
    # shellcheck disable=SC1090
    source "$env_file" || source_status=$?
    set +x
    keep_release_credentials_local
  } >/dev/null 2>&1
  set -euo pipefail
  [[ "$source_status" == 0 ]] \
    || die "release.env を読み込めません。値を表示せずに設定内容を確認してください"
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  for name in APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH TAURI_SIGNING_PRIVATE_KEY; do
    [[ -n "${!name:-}" ]] || die "release.env の $name が未設定です"
  done
  [[ "$APPLE_SIGNING_IDENTITY" == 'Developer ID Application: '* ]] \
    || die "APPLE_SIGNING_IDENTITY は Developer ID Application を指定してください"
  [[ "$APPLE_API_KEY" =~ ^[A-Za-z0-9]{10}$ ]] || die "APPLE_API_KEY の形式が不正です"
  [[ "$APPLE_API_ISSUER" =~ ^[A-Fa-f0-9]{8}(-[A-Fa-f0-9]{4}){3}-[A-Fa-f0-9]{12}$ ]] \
    || die "APPLE_API_ISSUER の形式が不正です"
  [[ "$APPLE_API_KEY_PATH" == /* && "$TAURI_SIGNING_PRIVATE_KEY" == /* ]] \
    || die "Apple API キーと updater 秘密鍵はファイルの絶対パスで指定してください"
  assert_private_file "$APPLE_API_KEY_PATH" APPLE_API_KEY_PATH
  assert_private_file "$TAURI_SIGNING_PRIVATE_KEY" TAURI_SIGNING_PRIVATE_KEY
  # API キー方式以外の公証・証明書設定や、外部からの Tauri 設定の混入を防ぐ。
  unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD
  unset TAURI_CONFIG CARGO_TARGET_DIR CARGO_BUILD_TARGET
}
