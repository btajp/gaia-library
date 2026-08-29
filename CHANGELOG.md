# Changelog

バージョンごとの変更一覧。リリースノートは対象バージョンの節から抽出する。
各版の日付は GitHub Release の公開日。署名・公証・配布確認の結果はリリース手順の記録に従う。

## [Unreleased]

今後の変更をこの節に追記する。

## [0.2.3] - 2026-08-29

### Changed

- デスクトップの各画面（検索・提案・手入力・設定）の冒頭に、その画面が何のためにあるかの説明文を追加した。scope（機密境界）・クライアント・提案キューという 3 つの概念に沿って、検索は選択中の scope の中だけから返ること、提案はエージェントが送ってきた書き込みの検品場所で承認するまでデータ本体に入らないこと、手入力は提案の作成と承認を 1 操作で行うこと、設定はクライアントと所属元（scope）の管理であることを明記した。
- 初回セットアップ画面に「エージェントは提案まで、承認はあなた（human）だけ」という役割分担の説明を追加し、所属元名が最初の機密境界（scope）の名前になること、ユーザー名が human クライアント名 `desktop:<名前>` になり承認・登録の履歴に記録されること（後から設定画面で変更可能）を入力欄の補足に明記した。

## [0.2.2] - 2026-08-29

### Added

- クライアント名の変更を追加した。CLI は `gaia client rename <旧名> <新名>`（`--json` 対応）、デスクトップは設定画面のクライアントカードの「名前を変更…」。役割・既定 scope・API キー（ハッシュ）を引き継ぎ、`[cli].default_client` が旧名なら新名へ追従する。HTTP のキーは有効なまま（接続中の HTTP セッションは旧名に結び付いているため一度無効になり（404）、クライアントが同じキーで initialize し直すと新名で再接続する）、stdio の接続設定は `--client <新名>` で出し直す。DB の履歴（提案の `proposed_by` / `decided_by`、監査ログの actor）は書き換えない。デスクトップは Keychain / 退避ファイルに保管中のキーも新しい名前へ移し、アプリ自身の human を改名した場合も以降の承認・却下を新名で記録する（設定を呼び出しごとに読み直す）。CLI の rename はデスクトップの保管キーを移さないため、デスクトップでキーを保管しているクライアントはデスクトップの「名前を変更…」を使うか、改名後にデスクトップでキーを再発行する。

### Changed

- README の `[sources.narumi]` を「narumi.app を使う場合（`--stdio-bridge`）」と「チェックアウトの開発サーバー（`--stdio`）を使う場合」の 2 例に整理した。narumi.app の bridge は `[sources.narumi.env]` に同梱の `narumi-keychain` ヘルパーと契約ディレクトリの指定が必須で、env 無しでは `authentication_required` で失敗する。失敗時の固定文言と切り分け（`stderr = "inherit"`、`RUST_LOG=warn`）を追記した。
- 設計書 `docs/superpowers/specs/2026-08-29-gaia-library-resolve-source-design.md` §15 に、`--stdio-bridge` の実機確認の記録（0.2.1 の README 推奨形は `authentication_required`、同梱ヘルパーの env 指定で `initialize` 成功、`serverInfo.name` は `narumi`）を追記した。
- AGENTS.md の「HTTP 接続とキー管理」に `gaia client rename` と履歴を書き換えない規則を追記した。

### Security

- デスクトップの改名で旧い名前の保管キー（Keychain 項目・退避ファイル）を削除できなかった場合に、成功として黙認せず警告を表示するようにした。有効なキーが旧名で残ることがあるため、警告が出た場合は Keychain の `gaia-library` 項目と退避ファイルを手動で確認・削除する（接続設定の表示は現在名とキーのハッシュ照合で行うため、旧名の残存キーは表示されない）。
- クライアント名に制御文字を使えないようにした（`gaia client add` / `gaia client rename`。デスクトップの入力検証と同じ基準）。既存の設定ファイルの読み込みは拒否しない。

## [0.2.1] - 2026-08-29

### Fixed

- デスクトップの参照カードで「内容を取得」を再実行した時に、前回の取得内容と URI コピーの通知を取得開始の時点で消すようにした。取得に失敗した場合に前回の内容が新しいエラーと並んで表示されなくなる。

### Changed

- README の `[sources.narumi]` 設定例を `uv --directory <narumi のチェックアウト> run narumi-server --stdio-bridge` にし、`narumi.app` を使う場合は `--stdio-bridge`（常駐サーバーへの橋渡し）を推奨、`--stdio` は `narumi.app` を起動していない開発用途向け（独立した開発用サーバー。接続管理・秘密入力は不可）と明記した。
- 設計書 `docs/superpowers/specs/2026-08-29-gaia-library-resolve-source-design.md` §15 に、narumi 0.3.0（契約 3.0.0）との実機確認の記録を追記した。`--stdio` 経路で handshake / `get_minutes` / `not_found` / `scope_denied` / 終了処理を確認済み。`--stdio-bridge` 経路は未検証。

## [0.2.0] - 2026-08-29

### Added

- `resolve_source` を登録した（契約 1.1.0）。`ref_id` または `uri`（実効 scope 内、最新 1 件）で登録済みの参照を特定し、参照の `system` に応じた解決器で本文を取得して返す。`file` は設定した許可ディレクトリ配下の通常ファイル、`url` は許可したホストへの http / https、`narumi` は設定したコマンドを子プロセスとして起動して MCP の `get_minutes` を呼ぶ。到達できない場合は `resolved=false` と理由を返し、参照と要点スナップショットをそのまま返す。DB は更新しない。
- 設定 `[sources]` を追加した。`file.roots`、`url.allow_hosts`、`narumi.command` などで解決器を有効にする。既定はすべて無効で、設定は呼び出しごとに読み直す。
- narumi 参照の規約を定めた: `system = "narumi"`, `uri = "narumi://meeting/<meeting_id>[?version=<n>]"`。現行の narumi 参照（`file://` の議事録）は `[sources.file].roots` に narumi の `meetings` ディレクトリを入れると解決できる。
- CLI に `gaia resolve --ref-id <id> | --uri <uri> [--content]` を追加した。デスクトップの参照カードに「内容を取得」を追加した。
- `get_server_info.capabilities.resolvers` に設定済みの解決器名を返すようにした。

### Changed

- MCP サーバーとデスクトップのツール呼び出しをブロッキング用スレッドで実行し、時間のかかる参照解決が JSON-RPC の応答や他のセッション・画面を止めないようにした。
- `[sources]` を含む設定ファイルは 0.1.x では読めない。戻す場合は該当節を削除する。既定値のままなら `[sources]` は書き出さない。

### Security

- `resolve_source` は入力の `uri` を取得先に使わず、承認済み参照の `uri` だけを実体化する。scope 外の参照と存在しない参照は同じ `not_found` を返す。
- `url` 解決は http / https のみ。userinfo 付き URL、`localhost`、ループバック・プライベート・リンクローカル・メタデータ・予約アドレスを DNS 解決後のアドレスでも拒否し、リダイレクトは上限付きで各段を再検査し、タイムアウトはリダイレクトを含む 1 参照あたりの合計とする。プロキシ環境変数と圧縮伸長を使わず、Cookie や認証ヘッダーを送らない。応答はテキスト系 Content-Type とサイズ上限に限る。
- `file` 解決は許可ディレクトリ配下の通常ファイルに限り、symlink を解決した実体で判定し、`O_NOFOLLOW` で開いたハンドルを検査する。設定ディレクトリ・DB ディレクトリ・キー退避ディレクトリは常に対象外。バイナリは返さない。
- `narumi` の起動コマンドは設定ファイルからのみ読み、ツール引数では指定できない。子プロセスの stdout は MCP 専用、stderr は既定で破棄し、タイムアウトで停止してプロセスグループごと終了させる。narumi へは参照行の scope 1 つだけを渡す。`get_minutes` 応答の本文は `[sources.narumi].max_bytes`（既定 1 MiB）を超えると返さない。解決器ごとに同時実行数を制限する。
- `resolve_source` の理由文言は固定文言のみで、上流のメッセージ・パス・IP・コマンドを含めない。URI と取得内容はログに残さない。

## [0.1.2] - 2026-08-29

### Changed

- `propose_update` の契約説明に、承認・却下済みの提案と同じ内容を再送した場合も duplicate として既存の提案を返すことを明記した。
- CI で同じブランチに新しい push があった場合、進行中の古い実行を自動でキャンセルするようにした。`main` への push は対象外で、desktop ジョブの差分検出（直前 push 基準）が変更を取りこぼさないようにした。

### Fixed

- 設定ファイルの保存先が symlink のとき、リンク自体を通常ファイルに置き換えず、リンク先のファイルを更新するようにした。ループしている symlink は明確なエラーで停止する。鎖は 40 段まで辿る。
- 既存パスへの `gaia init` や到達できない設定（dangling symlink など）への更新を拒否するとき、リンク先のディレクトリや `.lock` ファイルを作らないようにした。ディレクトリを指す symlink への保存は `.lock` を作る前にエラーで停止する。
- リリース補助スクリプト（release-metadata）の draft 確認で対象コミットの指定が抜けている場合に、使い方を示すエラーで停止するようにした。
- 設定検証が fail-closed であること（`[keys]` に不正な項目が 1 件でもあると HTTP 認証とキー発行が止まる）と復旧手順を README / AGENTS に記載した。重複ハッシュは両方の行を削除して両方のキーを再発行すること、未登録名は `gaia client add` で登録してから発行することを明記した。

### Security

- fact / ref の登録先（engagement / interaction / fact）が別 scope に存在する場合と存在しない場合で、同じ not_found エラーを返すようにした。エラー文言から他 scope の行の有無を推測できない。
- 設定ファイルの symlink を辿るとき、実効ユーザー以外が所有するリンクは辿らずエラーで停止するようにした。`.lock` ファイルが symlink の場合は開かず、リンク先の権限を変更しない。

## [0.1.1] - 2026-08-29

### Fixed

- リリース補助スクリプト（release-metadata）の overlay / assets / notary で出力先の指定が抜けている場合に、Node の内部エラーではなく使い方を示すエラーで停止するようにした。
- updater 署名検証のエラー文言で、英単語と日本語の間に空白が無い箇所を整え、同梱の検証ツールと表記を揃えた。

## [0.1.0] - 2026-08-29

### Added

- 契約駆動の MCP サーバー。stdio と Bearer 認証付き Streamable HTTP に対応する。
- 人物・組織・案件・ファクト・参照・用語集・活動記録の保存と検索。
- 明示的な scope による内容の分離、提案キュー、human に限定した承認・却下、監査ログ。
- CLI `gaia` による初期化、検索・閲覧、提案・承認、クライアント管理、キー発行、接続設定の出力。
- デスクトップアプリの検索・閲覧・手入力・提案承認・設定画面、内蔵 HTTP サーバー、同梱 CLI。
- minisign による自動更新の検証、Developer ID 署名・公証を確認してから公開するリリース処理。

### Security

- HTTP のクライアント識別をリクエスト単位で確認し、キー再発行後は旧キーを拒否する。
- 提案の承認・却下にも scope を適用し、異なる内容での request_id 再利用を拒否する。
- updater 公開鍵の継続性を確認し、鍵変更時は旧鍵で署名する橋渡しリリースを要求する。
