#!/usr/bin/env bash
# 設定公開鍵・署名鍵・直前タグの公開鍵の継続性を確認する。
set +x
set -euo pipefail

usage() {
  printf '%s\n' '使い方: check-updater-key-policy.sh --repo <repository> --private-key <path> [--allow-pubkey-rotation]' >&2
}

die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

REPO_DIR=""
PRIVATE_KEY=""
ALLOW_PUBKEY_ROTATION=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo|--private-key)
      [[ $# -ge 2 && -n "$2" ]] || { usage; exit 2; }
      if [[ "$1" == --repo ]]; then REPO_DIR="$2"; else PRIVATE_KEY="$2"; fi
      shift 2 ;;
    --allow-pubkey-rotation) ALLOW_PUBKEY_ROTATION=true; shift ;;
    -h|--help) usage; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done
[[ -n "$REPO_DIR" && -d "$REPO_DIR" ]] || die "--repo にリポジトリを指定してください"
[[ -n "$PRIVATE_KEY" && -f "$PRIVATE_KEY" && -s "$PRIVATE_KEY" && -r "$PRIVATE_KEY" ]] \
  || die "署名秘密鍵のファイルがないか、空または読み取り不可です"
command -v bun >/dev/null 2>&1 || die "Bun が必要です（toolchain.json を確認してください）"

normalize_key() { printf %s "$1" | tr -d '[:space:]'; }
same_key() { [[ "$(normalize_key "$1")" == "$(normalize_key "$2")" ]]; }

extract_pubkey() {
  bun -e '
    try {
      const config = JSON.parse(await Bun.stdin.text());
      const key = config.plugins?.updater?.pubkey;
      if (typeof key !== "string" || !key.trim()) process.exit(2);
      console.log(key);
    } catch { process.exit(1); }
  '
}

CONF_PATH="$REPO_DIR/desktop/src-tauri/tauri.conf.json"
[[ -f "$CONF_PATH" ]] || die "tauri.conf.json がありません"
CONF_PUBKEY="$(extract_pubkey < "$CONF_PATH")" || die "tauri.conf.json に updater 公開鍵がありません"
[[ -f "${PRIVATE_KEY}.pub" && -r "${PRIVATE_KEY}.pub" ]] || die "署名鍵に対応する .pub ファイルがありません"
SIGNING_PUBKEY="$(<"${PRIVATE_KEY}.pub")"
[[ -n "$(normalize_key "$SIGNING_PUBKEY")" ]] || die "署名鍵に対応する .pub ファイルが空です"

# リリース側で origin/main とタグを fetch 済みであること。
PREVIOUS_TAGS="$(git -C "$REPO_DIR" tag --merged origin/main --sort=-version:refname --list 'v[0-9]*')" \
  || die "過去のリリースタグを取得できません"
PREVIOUS_TAG="${PREVIOUS_TAGS%%$'\n'*}"
if [[ -z "$PREVIOUS_TAG" ]]; then
  same_key "$SIGNING_PUBKEY" "$CONF_PUBKEY" || die "署名鍵と設定の updater 公開鍵が一致しません"
  printf '%s\n' "$CONF_PUBKEY"
  exit 0
fi

PREVIOUS_CONFIG="$(git -C "$REPO_DIR" show "$PREVIOUS_TAG:desktop/src-tauri/tauri.conf.json" 2>/dev/null)" \
  || die "直前リリースの tauri.conf.json を読めません"
if PREVIOUS_PUBKEY="$(printf %s "$PREVIOUS_CONFIG" | extract_pubkey)"; then
  :
else
  RESULT=$?
  [[ "$RESULT" == 2 ]] || die "直前リリースの tauri.conf.json が不正です"
  printf '%s\n' 'INFO: 直前リリースには updater 公開鍵がないため、初回鍵として扱います。' >&2
  same_key "$SIGNING_PUBKEY" "$CONF_PUBKEY" || die "署名鍵と設定の updater 公開鍵が一致しません"
  printf '%s\n' "$CONF_PUBKEY"
  exit 0
fi

if same_key "$CONF_PUBKEY" "$PREVIOUS_PUBKEY"; then
  same_key "$SIGNING_PUBKEY" "$CONF_PUBKEY" || die "署名鍵と設定の updater 公開鍵が一致しません"
  printf '%s\n' "$CONF_PUBKEY"
  exit 0
fi
[[ "$ALLOW_PUBKEY_ROTATION" == true ]] \
  || die "直前リリースと公開鍵が異なります。橋渡しリリースのみ --allow-pubkey-rotation を指定できます"
same_key "$SIGNING_PUBKEY" "$PREVIOUS_PUBKEY" \
  || die "鍵ローテーションの橋渡しリリースは直前リリースの署名鍵で署名する必要があります"
printf '%s\n' 'INFO: 公開鍵ローテーションの橋渡し版です。生成物は旧公開鍵で検証します。' >&2
printf '%s\n' "$PREVIOUS_PUBKEY"
