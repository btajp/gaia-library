//! narumi 解決器の統合テスト。偽 narumi（`fake_narumi` バイナリ）を子プロセスとして起動し、
//! initialize → tools/call get_minutes の往復と、失敗経路・タイムアウト・子プロセスの後始末を検証する。
//! 実 narumi は不要。
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
    time::{Duration, Instant},
};

use gaia_core::{
    config::{NarumiSourceConfig, NarumiStderr, SourcesConfig},
    contracts::types::{RefTargetType, Reference},
    sources::{Note, Reason, ResolveRequest, SourceRegistry, SourceResolver, Unresolved},
};
use gaia_mcp::sources::NarumiResolver;

const ID: &str = "20260827T030500Z-a1b2c3d4";

fn fake_narumi() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_fake_narumi"))
}

fn settings(mode: &str, timeout_secs: u64, pid_file: Option<&std::path::Path>) -> SourcesConfig {
    let mut env = BTreeMap::from([("FAKE_NARUMI_MODE".to_string(), mode.to_string())]);
    if let Some(path) = pid_file {
        env.insert(
            "FAKE_NARUMI_PID_FILE".into(),
            path.to_string_lossy().into_owned(),
        );
    }
    SourcesConfig {
        narumi: Some(NarumiSourceConfig {
            command: fake_narumi(),
            args: vec![],
            timeout_secs,
            max_bytes: 1024 * 1024,
            stderr: NarumiStderr::Discard,
            env,
        }),
        ..SourcesConfig::default()
    }
}

fn reference(uri: &str, scope: &str) -> Reference {
    Reference {
        id: 1,
        target_type: RefTargetType::Fact,
        target_id: 1,
        system: "narumi".into(),
        uri: uri.into(),
        title: None,
        note: "n".into(),
        snapshot: Some("要点".into()),
        scope: scope.into(),
        last_verified: None,
        created_at: "2026-08-29T00:00:00Z".into(),
    }
}

fn resolve(
    resolver: &NarumiResolver,
    settings: &SourcesConfig,
    uri: &str,
    scope: &str,
) -> Result<gaia_core::sources::Resolved, Unresolved> {
    resolver.resolve(ResolveRequest {
        reference: &reference(uri, scope),
        settings,
    })
}

fn reason(result: Result<gaia_core::sources::Resolved, Unresolved>) -> Reason {
    match result {
        Err(Unresolved::Unavailable(reason)) => reason,
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

fn pid_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) は存在確認のみでシグナルを送らない。
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn wait_pid_gone(pid: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    !pid_alive(pid)
}

fn read_pid(path: &std::path::Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "pid file was not written");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn ok_round_trip_passes_meeting_id_version_and_single_scope() {
    let resolver = NarumiResolver::new();
    let settings = settings("ok", 10, None);
    let resolved = resolve(
        &resolver,
        &settings,
        &format!("narumi://meeting/{ID}?version=1#t=1200"),
        "cn",
    )
    .unwrap();
    assert!(resolved.content.starts_with("# minutes"));
    let json_start = resolved.content.find("{").unwrap();
    let json_end = resolved.content.rfind("}").unwrap();
    let args: serde_json::Value =
        serde_json::from_str(&resolved.content[json_start..=json_end]).unwrap();
    assert_eq!(args["meeting_id"], ID);
    assert_eq!(args["version"], 1);
    assert_eq!(args["scope"], "cn", "参照行の scope を単一文字列で渡す");
    assert!(matches!(
        resolved.notes[0],
        Note::NarumiVersion { version: 1, .. }
    ));
    // version 省略時は応答の版が注記に入る
    let resolved = resolve(
        &resolver,
        &settings,
        &format!("narumi://meeting/{ID}"),
        "cn",
    )
    .unwrap();
    assert_eq!(
        resolved.notes[0],
        Note::NarumiVersion {
            version: 2,
            available: vec![1, 2],
            generated_at: "2026-08-27T03:05:00Z".into(),
            provider: "none".into(),
        }
    );
    let args: serde_json::Value = serde_json::from_str(
        &resolved.content
            [resolved.content.find("{").unwrap()..=resolved.content.rfind("}").unwrap()],
    )
    .unwrap();
    assert!(args.get("version").is_none());
}

#[test]
fn error_envelopes_are_distinguished_by_code() {
    let resolver = NarumiResolver::new();
    for code in ["not_found", "scope_denied"] {
        let result = resolve(
            &resolver,
            &settings(code, 10, None),
            &format!("narumi://meeting/{ID}"),
            "cn",
        );
        assert_eq!(reason(result), Reason::NarumiError { code: code.into() });
    }
}

#[test]
fn text_only_and_unresolved_and_huge_responses() {
    let resolver = NarumiResolver::new();
    let resolved = resolve(
        &resolver,
        &settings("text_only", 10, None),
        &format!("narumi://meeting/{ID}"),
        "cn",
    )
    .unwrap();
    assert_eq!(resolved.content, "text-only minutes");
    let resolved = resolve(
        &resolver,
        &settings("unresolved", 10, None),
        &format!("narumi://meeting/{ID}"),
        "cn",
    )
    .unwrap();
    assert!(
        resolved
            .notes
            .contains(&Note::UnresolvedSpeakers { count: 2 })
    );
    // huge は解決器としては verbatim（切り詰めは ToolService 側の shape_content）
    let resolved = resolve(
        &resolver,
        &settings("huge", 10, None),
        &format!("narumi://meeting/{ID}"),
        "cn",
    )
    .unwrap();
    assert_eq!(resolved.content.chars().count(), 40_000);
    // markdown が [sources.narumi].max_bytes（バイト数）を超えると本文を返さず TooLarge
    let mut limited = settings("huge", 10, None);
    limited.narumi.as_mut().unwrap().max_bytes = 40_000 * 3 - 1;
    assert_eq!(
        reason(resolve(
            &resolver,
            &limited,
            &format!("narumi://meeting/{ID}"),
            "cn",
        )),
        Reason::TooLarge
    );
}

#[test]
fn hang_times_out_and_kills_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let resolver = NarumiResolver::new();
    let started = Instant::now();
    let result = resolve(
        &resolver,
        &settings("hang", 2, Some(&pid_file)),
        &format!("narumi://meeting/{ID}"),
        "cn",
    );
    assert_eq!(reason(result), Reason::TimedOut { secs: 2 });
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    let pid = read_pid(&pid_file);
    assert!(
        wait_pid_gone(pid),
        "child {pid} must not survive the timeout"
    );
}

#[test]
fn grandchildren_do_not_survive_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("grandchild-pid");
    let resolver = NarumiResolver::new();
    let result = resolve(
        &resolver,
        &settings("grandchild", 2, Some(&pid_file)),
        &format!("narumi://meeting/{ID}"),
        "cn",
    );
    assert_eq!(reason(result), Reason::TimedOut { secs: 2 });
    let grandchild = read_pid(&pid_file);
    assert!(
        wait_pid_gone(grandchild),
        "grandchild {grandchild} must be killed with the process group"
    );
}

#[test]
fn start_and_handshake_failures_and_wrong_server_name() {
    let resolver = NarumiResolver::new();
    let mut missing = settings("ok", 5, None);
    missing.narumi.as_mut().unwrap().command = PathBuf::from("/nonexistent/narumi-server");
    assert_eq!(
        reason(resolve(
            &resolver,
            &missing,
            &format!("narumi://meeting/{ID}"),
            "cn"
        )),
        Reason::NarumiStartFailed
    );
    assert_eq!(
        reason(resolve(
            &resolver,
            &settings("exit", 5, None),
            &format!("narumi://meeting/{ID}"),
            "cn"
        )),
        Reason::NarumiHandshakeFailed
    );
    assert_eq!(
        reason(resolve(
            &resolver,
            &settings("wrong_name", 5, None),
            &format!("narumi://meeting/{ID}"),
            "cn"
        )),
        Reason::NarumiNotNarumi
    );
    // stdout に JSON でない行が混ざっても rmcp は読み飛ばす（現行 3.1.4 の挙動）。壊れても固定文言で返ること。
    let junk = resolve(
        &resolver,
        &settings("junk_stdout", 5, None),
        &format!("narumi://meeting/{ID}"),
        "cn",
    );
    assert!(
        matches!(
            &junk,
            Ok(_)
                | Err(Unresolved::Unavailable(
                    Reason::NarumiHandshakeFailed
                        | Reason::NarumiInvalidResponse
                        | Reason::TimedOut { .. }
                ))
        ),
        "{junk:?}"
    );
}

#[test]
fn stderr_modes_do_not_affect_the_result() {
    let resolver = NarumiResolver::new();
    for stderr in [NarumiStderr::Discard, NarumiStderr::Inherit] {
        let mut settings = settings("stderr_noise", 10, None);
        settings.narumi.as_mut().unwrap().stderr = stderr;
        let resolved = resolve(
            &resolver,
            &settings,
            &format!("narumi://meeting/{ID}"),
            "cn",
        )
        .unwrap();
        assert!(resolved.content.starts_with("# minutes"));
    }
}

#[test]
fn invalid_uri_does_not_start_the_child() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let resolver = NarumiResolver::new();
    let result = resolve(
        &resolver,
        &settings("exit", 5, Some(&pid_file)),
        "narumi://meeting/not-a-meeting-id",
        "cn",
    );
    assert_eq!(
        reason(result),
        Reason::InvalidUri {
            system: "narumi",
            rule: "meeting_id"
        }
    );
    assert!(
        !pid_file.exists(),
        "child must not be spawned for invalid uris"
    );
    // 未設定なら NotConfigured
    let result = resolve(
        &resolver,
        &SourcesConfig::default(),
        &format!("narumi://meeting/{ID}"),
        "cn",
    );
    assert_eq!(
        reason(result),
        Reason::NotConfigured {
            system: "narumi",
            setting: "[sources.narumi].command"
        }
    );
}

#[test]
fn registry_serializes_narumi_calls_to_one_child() {
    let mut registry = SourceRegistry::empty();
    registry.register(Arc::new(NarumiResolver::new())).unwrap();
    let registry = Arc::new(registry);
    let barrier = Arc::new(Barrier::new(2));
    let worker = {
        let registry = registry.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            let _permit = registry.acquire("narumi").expect("first permit");
            barrier.wait();
            barrier.wait();
        })
    };
    barrier.wait();
    assert!(registry.acquire("narumi").is_none(), "second call is busy");
    barrier.wait();
    worker.join().unwrap();
    assert!(registry.acquire("narumi").is_some());
}

/// `gaia serve --stdio` / `--http` は `Runtime::block_on` の内側で ToolService（→ NarumiResolver）を drop する。
/// 常駐 runtime を async コンテキストで drop しても panic しないこと（tokio は待つ drop を禁止する）。
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn resolver_can_be_dropped_inside_an_async_context_after_use() {
    let resolver = Arc::new(NarumiResolver::new());
    let settings = settings("ok", 10, None);
    let resolved = tokio::task::spawn_blocking({
        let resolver = resolver.clone();
        move || {
            resolve(
                &resolver,
                &settings,
                &format!("narumi://meeting/{ID}"),
                "cn",
            )
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert!(resolved.content.starts_with("# minutes"));
    let resolver = Arc::try_unwrap(resolver).ok().expect("sole owner");
    drop(resolver);
}
