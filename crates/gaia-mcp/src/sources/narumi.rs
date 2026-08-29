//! narumi 解決器。設定したコマンドを子プロセスとして起動し、MCP の `get_minutes` を呼んで markdown を返す。
//! 1 呼び出し = 1 子プロセス（起動 → initialize → get_minutes → cancel）。子の stdout は MCP 専用で、
//! gaia 自身の stdout には触れない。stderr は既定で破棄する。起動コマンドは設定ファイルからのみ読む。
use std::{
    process::Stdio,
    sync::{OnceLock, mpsc},
    time::{Duration, Instant},
};

use gaia_core::{
    config::{NarumiSourceConfig, NarumiStderr, SourcesConfig},
    sources::{Availability, Note, Reason, ResolveRequest, Resolved, SourceResolver, Unresolved},
};
use process_wrap::tokio::CommandWrap;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    transport::TokioChildProcess,
};
use serde_json::{Map, Value, json};
use tokio::runtime::Runtime;

use super::narumi_uri::{NarumiTarget, parse_narumi_uri};

const SETTING: &str = "[sources.narumi].command";
/// 呼び出し側が諦めるまでの猶予（子の終了処理を含む）。
const GRACE: Duration = Duration::from_secs(5);
/// narumi の initialize 応答で検証するサーバー名。
const NARUMI_SERVER_NAME: &str = "narumi";

pub struct NarumiResolver {
    runtime: OnceLock<Runtime>,
}

impl Default for NarumiResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl NarumiResolver {
    pub fn new() -> Self {
        Self {
            runtime: OnceLock::new(),
        }
    }

    /// 常駐 runtime。呼び出しごとに生成・破棄しない（rmcp の子プロセス kill は `tokio::spawn` で行われるため、
    /// runtime を直後に落とすと kill が実行されない）。破棄は `Drop` で待たずに行う。
    fn runtime(&self) -> Result<&Runtime, String> {
        if let Some(runtime) = self.runtime.get() {
            return Ok(runtime);
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("gaia-narumi")
            .enable_all()
            .build()
            .map_err(|e| format!("cannot build narumi runtime: {e}"))?;
        Ok(self.runtime.get_or_init(|| runtime))
    }
}

impl Drop for NarumiResolver {
    /// `gaia serve` は `Runtime::block_on` の内側で ToolService（→ この解決器）を drop する。tokio は async
    /// コンテキストでの「待つ drop」を panic にするため、待たずに停止する。進行中の解決があれば完了を待たない
    /// （子プロセスは `kill_on_drop` で消える）。
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl SourceResolver for NarumiResolver {
    fn system(&self) -> &'static str {
        "narumi"
    }

    fn availability(&self, settings: &SourcesConfig) -> Availability {
        if settings.narumi.is_some() {
            Availability::Ready
        } else {
            Availability::Unconfigured { setting: SETTING }
        }
    }

    fn max_concurrency(&self) -> usize {
        1
    }

    fn resolve(&self, request: ResolveRequest<'_>) -> Result<Resolved, Unresolved> {
        let cfg = request
            .settings
            .narumi
            .clone()
            .ok_or(Unresolved::Unavailable(Reason::NotConfigured {
                system: "narumi",
                setting: SETTING,
            }))?;
        // URI 規約違反は子プロセスを起動しない。
        let target = parse_narumi_uri(&request.reference.uri).map_err(Unresolved::Unavailable)?;
        let scope = request.reference.scope.clone();
        let timeout = Duration::from_secs(cfg.timeout_secs);
        let runtime = self.runtime().map_err(Unresolved::Internal)?;
        let (tx, rx) = mpsc::channel();
        runtime.spawn(async move {
            let _ = tx.send(fetch_minutes(cfg, target, scope).await);
        });
        // 呼び出し元が tokio ワーカー・spawn_blocking・runtime 無し（CLI）のどれでも block_on を使わない。
        match rx.recv_timeout(timeout + GRACE) {
            Ok(result) => result,
            Err(_) => Err(Unresolved::Unavailable(Reason::TimedOut {
                secs: timeout.as_secs(),
            })),
        }
    }
}

fn build_command(cfg: &NarumiSourceConfig) -> CommandWrap {
    let mut command = tokio::process::Command::new(&cfg.command);
    command.args(&cfg.args);
    // 親の環境を継承し、GAIA_ 接頭辞の変数だけ除去してから設定の env を重ねる。
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GAIA_") {
            command.env_remove(key);
        }
    }
    command.envs(&cfg.env);
    command.kill_on_drop(true);
    let mut wrap = CommandWrap::from(command);
    #[cfg(unix)]
    wrap.wrap(process_wrap::tokio::ProcessGroup::leader());
    wrap
}

/// cancel 後にプロセスグループが残っていれば（`uv run` の孫など）SIGKILL を送る補助。
#[cfg(unix)]
fn kill_process_group(pgid: u32) {
    if let Ok(pgid) = i32::try_from(pgid) {
        // SAFETY: killpg はシグナル送信のみで、メモリ安全性に影響しない。失敗（ESRCH など）は無視する。
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pgid: u32) {}

async fn fetch_minutes(
    cfg: NarumiSourceConfig,
    target: NarumiTarget,
    scope: String,
) -> Result<Resolved, Unresolved> {
    let timeout = Duration::from_secs(cfg.timeout_secs);
    let timed_out = || {
        Unresolved::Unavailable(Reason::TimedOut {
            secs: timeout.as_secs(),
        })
    };
    let deadline = tokio::time::Instant::from_std(Instant::now() + timeout);
    let stderr = match cfg.stderr {
        NarumiStderr::Discard => Stdio::null(),
        NarumiStderr::Inherit => Stdio::inherit(),
    };
    let (transport, _stderr) = match TokioChildProcess::builder(build_command(&cfg))
        .stderr(stderr)
        .spawn()
    {
        Ok(spawned) => spawned,
        Err(error) => {
            tracing::warn!(kind = ?error.kind(), "narumi resolver: cannot start the configured command");
            return Err(Unresolved::Unavailable(Reason::NarumiStartFailed));
        }
    };
    let pgid = transport.id();
    let running = match tokio::time::timeout_at(deadline, ().serve(transport)).await {
        Ok(Ok(running)) => running,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "narumi resolver: MCP initialize failed");
            if let Some(pgid) = pgid {
                kill_process_group(pgid);
            }
            return Err(Unresolved::Unavailable(Reason::NarumiHandshakeFailed));
        }
        Err(_) => {
            if let Some(pgid) = pgid {
                kill_process_group(pgid);
            }
            return Err(timed_out());
        }
    };
    let server_name = running
        .peer_info()
        .and_then(|info| info.server_info.as_ref().map(|s| s.name.clone()));
    let outcome = if server_name.as_deref() != Some(NARUMI_SERVER_NAME) {
        tracing::warn!("narumi resolver: the configured command is not a narumi server");
        Err(Unresolved::Unavailable(Reason::NarumiNotNarumi))
    } else {
        let mut arguments = Map::new();
        arguments.insert("meeting_id".into(), json!(target.meeting_id));
        if let Some(version) = target.version {
            arguments.insert("version".into(), json!(version));
        }
        // 参照行の scope を単一文字列で渡す（呼び出し側の実効 scope 集合は渡さない）。
        arguments.insert("scope".into(), json!(scope));
        let params = CallToolRequestParams::new("get_minutes").with_arguments(arguments);
        match tokio::time::timeout_at(deadline, running.peer().call_tool(params)).await {
            Ok(Ok(result)) => map_get_minutes(result, &target, cfg.max_bytes),
            Ok(Err(error)) => {
                tracing::warn!(error = %error, "narumi resolver: get_minutes call failed");
                Err(Unresolved::Unavailable(Reason::NarumiInvalidResponse))
            }
            Err(_) => Err(timed_out()),
        }
    };
    // 成功・失敗・タイムアウトの全経路で cancel（stdin close → 3 秒待って kill）を通す。
    let _ = running.cancel().await;
    if let Some(pgid) = pgid {
        kill_process_group(pgid);
    }
    outcome
}

/// 上流由来の短い文字列を注記に載せる前に、表示可能な ASCII だけに絞って長さを抑える。
fn sanitize_short(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(64)
        .collect()
}

/// narumi の error code を `[a-z_]{1,32}` に正規化する。
fn normalize_code(code: &str) -> Option<String> {
    let normalized: String = code
        .chars()
        .filter(|c| c.is_ascii_alphabetic() || *c == '_')
        .map(|c| c.to_ascii_lowercase())
        .take(32)
        .collect();
    (!normalized.is_empty()).then_some(normalized)
}

/// `get_minutes` の応答を `Resolved` に写す（純関数。JSON フィクスチャでテスト）。
/// `max_bytes` は markdown のバイト上限（`[sources.narumi].max_bytes`）。応答全体は rmcp が既に受信しているため、
/// ここでの検査は「超過分をツール応答へ流さない」ための上限で、受信量そのものは抑えない。
pub fn map_get_minutes(
    result: CallToolResult,
    target: &NarumiTarget,
    max_bytes: u64,
) -> Result<Resolved, Unresolved> {
    let invalid = || Unresolved::Unavailable(Reason::NarumiInvalidResponse);
    let value: Value = match result.structured_content {
        Some(value) => value,
        None => {
            let text = result
                .content
                .first()
                .and_then(|block| block.as_text())
                .map(|t| t.text.clone())
                .ok_or_else(invalid)?;
            serde_json::from_str(&text).map_err(|_| invalid())?
        }
    };
    if result.is_error == Some(true) {
        let error = &value["error"];
        let code = error["code"].as_str().and_then(normalize_code);
        tracing::warn!(
            code = ?code,
            message = ?error["message"].as_str().map(sanitize_short),
            "narumi resolver: get_minutes returned an error"
        );
        return Err(Unresolved::Unavailable(match code {
            Some(code) => Reason::NarumiError { code },
            None => Reason::NarumiInvalidResponse,
        }));
    }
    let markdown = value["markdown"].as_str().ok_or_else(invalid)?;
    if value["meeting_id"].as_str() != Some(target.meeting_id.as_str()) {
        return Err(invalid());
    }
    if markdown.len() as u64 > max_bytes {
        return Err(Unresolved::Unavailable(Reason::TooLarge));
    }
    let mut notes = Vec::new();
    if let Some(version) = value["version"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
    {
        let available = value["available_versions"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_u64().and_then(|v| u32::try_from(v).ok()))
                    .collect()
            })
            .unwrap_or_default();
        notes.push(Note::NarumiVersion {
            version,
            available,
            generated_at: value["generated_at"]
                .as_str()
                .map(sanitize_short)
                .unwrap_or_default(),
            provider: value["provider"]
                .as_str()
                .map(sanitize_short)
                .unwrap_or_default(),
        });
    }
    if let Some(count) = value["unresolved_speakers"]
        .as_array()
        .map(Vec::len)
        .filter(|count| *count > 0)
    {
        notes.push(Note::UnresolvedSpeakers { count });
    }
    Ok(Resolved {
        content: markdown.to_string(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ContentBlock;

    const ID: &str = "20260827T030500Z-a1b2c3d4";
    const MAX_BYTES: u64 = 1024 * 1024;

    fn target() -> NarumiTarget {
        NarumiTarget {
            meeting_id: ID.into(),
            version: None,
        }
    }

    fn ok_payload() -> Value {
        json!({
            "meeting_id": ID, "version": 2, "markdown": "# 議事録\n- 決定",
            "generated_at": "2026-08-27T03:05:00Z", "provider": "none",
            "unresolved_speakers": [], "available_versions": [1, 2]
        })
    }

    #[test]
    fn maps_structured_success_with_version_note() {
        let resolved = map_get_minutes(
            CallToolResult::structured(ok_payload()),
            &target(),
            MAX_BYTES,
        )
        .unwrap();
        assert_eq!(resolved.content, "# 議事録\n- 決定");
        assert_eq!(
            resolved.notes,
            vec![Note::NarumiVersion {
                version: 2,
                available: vec![1, 2],
                generated_at: "2026-08-27T03:05:00Z".into(),
                provider: "none".into(),
            }]
        );
    }

    #[test]
    fn falls_back_to_text_json_and_counts_unresolved_speakers() {
        let mut payload = ok_payload();
        payload["unresolved_speakers"] = json!(["Speaker 1", "Speaker 2"]);
        payload["generated_at"] = json!("2026-08-27T03:05:00Z\n<script>");
        let result = CallToolResult::success(vec![ContentBlock::text(payload.to_string())]);
        let resolved = map_get_minutes(result, &target(), MAX_BYTES).unwrap();
        assert_eq!(resolved.notes.len(), 2);
        assert_eq!(resolved.notes[1], Note::UnresolvedSpeakers { count: 2 });
        match &resolved.notes[0] {
            Note::NarumiVersion { generated_at, .. } => {
                assert_eq!(generated_at, "2026-08-27T03:05:00Z<script>")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn maps_error_envelopes_by_code_and_rejects_malformed_responses() {
        for code in ["not_found", "scope_denied", "invalid_argument", "internal"] {
            let result = CallToolResult::structured_error(
                json!({"error": {"code": code, "message": "secret detail"}}),
            );
            assert_eq!(
                map_get_minutes(result, &target(), MAX_BYTES).unwrap_err(),
                Unresolved::Unavailable(Reason::NarumiError { code: code.into() })
            );
        }
        let weird = CallToolResult::structured_error(json!({"error": {"code": "Not-Found 1"}}));
        assert_eq!(
            map_get_minutes(weird, &target(), MAX_BYTES).unwrap_err(),
            Unresolved::Unavailable(Reason::NarumiError {
                code: "notfound".into()
            })
        );
        let invalid = Unresolved::Unavailable(Reason::NarumiInvalidResponse);
        assert_eq!(
            map_get_minutes(
                CallToolResult::structured_error(json!({"error": {}})),
                &target(),
                MAX_BYTES
            )
            .unwrap_err(),
            invalid
        );
        let mut missing = ok_payload();
        missing.as_object_mut().unwrap().remove("markdown");
        assert_eq!(
            map_get_minutes(CallToolResult::structured(missing), &target(), MAX_BYTES).unwrap_err(),
            invalid
        );
        let mut mismatch = ok_payload();
        mismatch["meeting_id"] = json!("20260827T030500Z-ffffffff");
        assert_eq!(
            map_get_minutes(CallToolResult::structured(mismatch), &target(), MAX_BYTES)
                .unwrap_err(),
            invalid
        );
        assert_eq!(
            map_get_minutes(CallToolResult::success(vec![]), &target(), MAX_BYTES).unwrap_err(),
            invalid
        );
        assert_eq!(
            map_get_minutes(
                CallToolResult::success(vec![ContentBlock::text("not json")]),
                &target(),
                MAX_BYTES
            )
            .unwrap_err(),
            invalid
        );
    }

    #[test]
    fn markdown_over_max_bytes_is_too_large_and_exact_size_is_accepted() {
        let markdown = "字".repeat(100);
        let bytes = markdown.len() as u64;
        assert_eq!(bytes, 300);
        let mut payload = ok_payload();
        payload["markdown"] = json!(markdown);
        assert_eq!(
            map_get_minutes(
                CallToolResult::structured(payload.clone()),
                &target(),
                bytes - 1
            )
            .unwrap_err(),
            Unresolved::Unavailable(Reason::TooLarge)
        );
        let resolved =
            map_get_minutes(CallToolResult::structured(payload), &target(), bytes).unwrap();
        assert_eq!(resolved.content.len() as u64, bytes);
        // 上限はバイト数で数える（文字数ではない）
        let mut ascii = ok_payload();
        ascii["markdown"] = json!("a".repeat(100));
        assert!(map_get_minutes(CallToolResult::structured(ascii), &target(), 100).is_ok());
    }

    #[test]
    fn availability_and_concurrency() {
        let resolver = NarumiResolver::new();
        assert_eq!(resolver.system(), "narumi");
        assert_eq!(resolver.max_concurrency(), 1);
        let mut settings = SourcesConfig::default();
        assert_eq!(
            resolver.availability(&settings),
            Availability::Unconfigured { setting: SETTING }
        );
        settings.narumi = Some(NarumiSourceConfig {
            command: "/usr/bin/true".into(),
            args: vec![],
            timeout_secs: 1,
            max_bytes: MAX_BYTES,
            stderr: NarumiStderr::Discard,
            env: Default::default(),
        });
        assert_eq!(resolver.availability(&settings), Availability::Ready);
    }
}
