# updater 実機 E2E

旧版の隔離コピーから `99.0.0` へ更新し、再起動後の HTTP サーバーまで確認する。
リリース前の必須確認だが、この手順を用意した時点では未実施である。
Developer ID の署名・公証・Gatekeeper の確認は別途リリース処理と配布後のスモークで行う。

## 隔離するもの

- `old.conf.json`: 旧版。E2E 専用 identifier と localhost の updater endpoint を使う。
- `new.conf.json`: 同じ E2E identifier で `99.0.0` と updater 生成物を有効にする。
- アプリ・ビルド先・設定・DB・配信ファイルは、新規の一時ディレクトリに限定する。
- HTTP はアプリ用 `127.0.0.1:4119`、更新配信用 `127.0.0.1:8930` を使う。使用中なら中止し、他のプロセスを停止しない。

`dangerousInsecureTransportProtocol` は、この二つの overlay だけで許可する。
本番の `tauri.conf.json` には入れない。配信先は `http://127.0.0.1:8930` に限定する。
E2E identifier により通常アプリとは single-instance の識別も分ける。
本番のアプリ、設定、DB、CLI のリンク、OS キーチェーンは変更しない。実クライアントのキー発行・CLI リンク作成は行わない。
E2E 内でのみランダムな一時キーを発行し、平文は破棄する。隔離した設定に保存するのはハッシュだけである。

## 1. 一時環境を準備する

Apple Silicon の macOS と、`toolchain.json` の Bun / Tauri CLI を使用する。
以下はリポジトリ直下から開始する専用の Bash セッションで、順に行う手順である。
既存の updater 署名鍵と対応する `.pub` が必要で、新しい署名鍵は生成しない。Apple の資格情報は読み込まない。

```bash
set -euo pipefail
set +x
umask 077
GAIA_REPO="$(pwd -P)"
test -f "$GAIA_REPO/desktop/src-tauri/tauri.conf.json"
E2E_ROOT="$(mktemp -d /private/tmp/gaia-updater-e2e.XXXXXX)"
export GAIA_CONFIG="$E2E_ROOT/config.toml"
export GAIA_DB="$E2E_ROOT/gaia.db"
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/gaia-library-updater.key"
test -f "$TAURI_SIGNING_PRIVATE_KEY"
test -f "$TAURI_SIGNING_PRIVATE_KEY.pub"
read -r -s -p 'updater 鍵のパスワード（なければ空欄）: ' TAURI_SIGNING_PRIVATE_KEY_PASSWORD
printf '\n'
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
unset APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD
unset TAURI_CONFIG CARGO_TARGET_DIR CARGO_BUILD_TARGET
if lsof -nP -iTCP:8930 -iTCP:4119 -sTCP:LISTEN; then
  printf '%s\n' 'E2E 用ポートが使用中のため中止します。' >&2
  exit 1
fi
"$GAIA_REPO/desktop/build-app.sh"
"$GAIA_REPO/desktop/src-tauri/binaries/gaia-aarch64-apple-darwin" init \
  --affiliation updater-e2e --client-name updater-e2e --db "$GAIA_DB"
"$GAIA_REPO/desktop/src-tauri/binaries/gaia-aarch64-apple-darwin" client keygen updater-e2e > /dev/null
bun -e '
  const path = process.argv[1];
  const text = await Bun.file(path).text();
  if (!/^\[server\]\r?$/m.test(text)) throw new Error("server section がありません");
  await Bun.write(path, text.replace(/^\[server\]\r?$/m, "[server]\nport = 4119"));
' "$GAIA_CONFIG"
```

`GAIA_CONFIG` / `GAIA_DB` を明示するのは、本番のデータを開かないためである。
HTTP サーバーは有効なキーが一つもないと起動しないため、隔離した `updater-e2e` クライアントに一時キーを発行する。
平文の stdout は `/dev/null` に破棄し、OS キーチェーンにも登録しない。実クライアントのキーには触れない。

## 2. 新版を用意し、旧版を隔離コピーする

新版の archive を保存してから、同じ出力先で旧版をビルドする。
updater の署名を本番と同じ検証コードで照合し、失敗した場合は起動しない。

```bash
cd "$GAIA_REPO/desktop/src-tauri"
CARGO_TARGET_DIR="$E2E_ROOT/build" cargo tauri build --bundles app --config ../e2e-updater/new.conf.json
E2E_BUNDLE="$E2E_ROOT/build/release/bundle/macos"
mkdir "$E2E_ROOT/updates" "$E2E_ROOT/app"
cp "$E2E_BUNDLE/gaia-library.app.tar.gz" "$E2E_BUNDLE/gaia-library.app.tar.gz.sig" "$E2E_ROOT/updates/"
E2E_PUBKEY="$(bun -e 'console.log((await Bun.file(process.argv[1]).json()).plugins.updater.pubkey)' tauri.conf.json)"
CARGO_TARGET_DIR="$E2E_ROOT/build" cargo run --locked --quiet --features updater-verifier --bin verify-updater-signature -- \
  "$E2E_ROOT/updates/gaia-library.app.tar.gz" "$E2E_ROOT/updates/gaia-library.app.tar.gz.sig" "$E2E_PUBKEY"
bun -e '
  const root = process.argv[1];
  const signature = (await Bun.file(`${root}/gaia-library.app.tar.gz.sig`).text()).trim();
  await Bun.write(`${root}/latest.json`, JSON.stringify({version:"99.0.0", platforms:{"darwin-aarch64":{
    signature, url:"http://127.0.0.1:8930/gaia-library.app.tar.gz"
  }}}, null, 2));
' "$E2E_ROOT/updates"
CARGO_TARGET_DIR="$E2E_ROOT/build" cargo tauri build --bundles app --config ../e2e-updater/old.conf.json
ditto "$E2E_BUNDLE/gaia-library.app" "$E2E_ROOT/app/gaia-library.app"
E2E_APP="$E2E_ROOT/app/gaia-library.app"
plutil -extract CFBundleShortVersionString raw -o - "$E2E_APP/Contents/Info.plist"
```

最後の版数が旧版であることを確認する。既に `99.0.0` なら中止する。
実行パスは `/private/tmp` 配下の実パスにする。`/tmp` は symlink のため使わない。
updater は実行パスに symlink を含む場合に差し替えを拒否することがある。

## 3. localhost で配信して更新する

配信対象を二つのファイルに限定し、外部インターフェースでは待ち受けない。
`GAIA_UPDATER_AUTO=1` は更新・再起動の確認を自動承認する E2E 専用指定である。

```bash
bun -e '
  const root = process.argv[1];
  const files = new Map([
    ["/latest.json", "latest.json"],
    ["/gaia-library.app.tar.gz", "gaia-library.app.tar.gz"]
  ]);
  Bun.serve({hostname:"127.0.0.1", port:8930, fetch(request) {
    const name = files.get(new URL(request.url).pathname);
    if (request.method !== "GET" || !name) return new Response(null, {status:404});
    return new Response(Bun.file(`${root}/${name}`), {headers:{"Cache-Control":"no-store"}});
  }});
' "$E2E_ROOT/updates" > "$E2E_ROOT/http.log" 2>&1 &
E2E_HTTP_PID=$!
curl --retry 5 --retry-connrefused --retry-delay 1 --fail --silent \
  http://127.0.0.1:8930/latest.json > /dev/null
GAIA_UPDATER_AUTO=1 "$E2E_APP/Contents/MacOS/gaia-desktop" > "$E2E_ROOT/app.log" 2>&1 &
E2E_OLD_PID=$!
printf 'E2E 記録先: %s\n旧アプリ PID: %s\n' "$E2E_ROOT" "$E2E_OLD_PID"
```

## 4. 合格条件を確認する

1. `app.log` で `downloading and installing v99.0.0`、適用完了、自動再起動の順を確認する。
2. 次のコマンドで app の版数が `99.0.0` であり、4119 の listener が一つであることを確認する。
3. listener の PID が旧アプリとは異なり、実行パスが今回の `E2E_APP` 配下であることを確認する。別アプリの 401 応答を成功と数えない。
4. 設定画面のクライアント・scope が `updater-e2e`、サーバー URL が `http://127.0.0.1:4119/mcp` のままであることを確認する。
5. アプリの終了操作後に 4119 が解放されることを確認する。通常アプリの終了操作と取り違えない。

```bash
plutil -extract CFBundleShortVersionString raw -o - "$E2E_APP/Contents/Info.plist"
lsof -nP -iTCP:4119 -sTCP:LISTEN
E2E_LISTENER_PID="$(lsof -t -nP -iTCP:4119 -sTCP:LISTEN)"
ps -p "$E2E_LISTENER_PID" -o pid=,command=
curl --silent --output /dev/null --write-out '%{http_code}\n' http://127.0.0.1:4119/mcp
```

認証なしの HTTP 応答は `401` が期待値である。設定や DB の初期化エラー、旧プロセスの残存、ポート競合は不合格とする。
確認ダイアログはこの自動承認テストの対象外であり、`GAIA_UPDATER_AUTO` なしの別途手動確認が必要である。

## 5. 後始末と記録

E2E アプリを終了した後、保存した PID が今回の配信プロセスである場合だけ停止する。
更新後のアプリ PID は変わるため、旧 PID を使って終了させない。広域の `pkill` は使用しない。

```bash
E2E_HTTP_COMMAND="$(ps -p "$E2E_HTTP_PID" -o command= || true)"
case "$E2E_HTTP_COMMAND" in
  *"$E2E_ROOT/updates"*) kill -TERM "$E2E_HTTP_PID" ;;
  *) printf '%s\n' '今回の配信プロセスと確認できないため、停止操作はしません。' ;;
esac
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
unset GAIA_CONFIG GAIA_DB
```

実施日、元の版数、確認したコミット、更新前後の PID、HTTP 応答、終了後のポート解放、合否を記録する。
秘密鍵・パスワード・接続キーは記録に含めない。一時ディレクトリは記録を保存してから、今回作成した一つだけを Finder でゴミ箱へ移す。
本番アプリの導入先や実データを削除する必要はない。
