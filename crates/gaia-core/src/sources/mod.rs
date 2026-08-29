//! resolve_source の解決器レジストリ。参照の `system` 値で解決器を選ぶ。
//! ここは rmcp と tokio を知らない。解決器は DB に触れず（`Connection` を渡さない）、
//! 失敗は固定文言の `Reason` で返す（上流の文字列・パス・IP・コマンドは埋め込まない）。
pub mod file;
pub mod net;
pub mod url;

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::{
    config::{Config, SourcesConfig},
    contracts::types::Reference,
};

pub use file::FileResolver;
pub use url::UrlResolver;

/// 解決器への入力。参照は DB 行そのもの（解決器は必ず `reference.uri` を実体化する）。
pub struct ResolveRequest<'a> {
    pub reference: &'a Reference,
    /// 呼び出し時点の `[sources]`。
    pub settings: &'a SourcesConfig,
}

/// 取得できた本文と注記。注記は `resolved=true` でも `reason` に載る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub content: String,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    /// `resolved=false` ＋ reason。
    Unavailable(Reason),
    /// 解決器内部の予期しない失敗。詳細は stderr にのみ出し、クライアントには `Reason::ResolverFailed`。
    Internal(String),
}

/// 固定文言カタログ。`Display` が reason を生成する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// `system` は呼び出し側自身の参照データ。`available` は登録済み解決器名。
    NoResolver {
        system: String,
        available: Vec<String>,
    },
    NotConfigured {
        system: &'static str,
        setting: &'static str,
    },
    SettingsUnavailable,
    InvalidUri {
        system: &'static str,
        rule: &'static str,
    },
    /// 不在・root 外・通常ファイル以外・権限不足を畳む。
    FileUnavailable,
    BinaryContent,
    TooLarge,
    UrlNotAllowed {
        rule: UrlRule,
    },
    UpstreamStatus {
        status: u16,
    },
    UnsupportedContentType,
    TimedOut {
        secs: u64,
    },
    ConnectionFailed,
    ReadFailed,
    NarumiStartFailed,
    NarumiHandshakeFailed,
    NarumiNotNarumi,
    /// `[a-z_]{1,32}` に正規化した code のみ。
    NarumiError {
        code: String,
    },
    NarumiInvalidResponse,
    ResolverFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlRule {
    Scheme,
    Credentials,
    Host,
    Address,
    Redirects,
}

impl fmt::Display for UrlRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Scheme => "scheme",
            Self::Credentials => "credentials",
            Self::Host => "host",
            Self::Address => "address",
            Self::Redirects => "redirects",
        })
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoResolver { system, available } => {
                let available = if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                };
                write!(
                    f,
                    "no resolver for system `{system}` (available: {available})"
                )
            }
            Self::NotConfigured { system, setting } => {
                write!(f, "resolver `{system}` is not configured (set {setting})")
            }
            Self::SettingsUnavailable => f.write_str("source settings could not be loaded"),
            Self::InvalidUri { system, rule } => write!(
                f,
                "reference uri does not follow the `{system}` convention ({rule})"
            ),
            Self::FileUnavailable => {
                f.write_str("file is not available under the configured roots")
            }
            Self::BinaryContent => f.write_str("content is binary, not text"),
            Self::TooLarge => f.write_str("content exceeds the configured size limit"),
            Self::UrlNotAllowed { rule } => write!(f, "url is not allowed ({rule})"),
            Self::UpstreamStatus { status } => {
                write!(f, "upstream responded with HTTP {status}")
            }
            Self::UnsupportedContentType => f.write_str("upstream content type is not text"),
            Self::TimedOut { secs } => write!(f, "resolution timed out after {secs}s"),
            Self::ConnectionFailed => f.write_str("connection to upstream failed"),
            Self::ReadFailed => f.write_str("reading the content failed"),
            Self::NarumiStartFailed => f.write_str("narumi command could not be started"),
            Self::NarumiHandshakeFailed => f.write_str("narumi did not complete the MCP handshake"),
            Self::NarumiNotNarumi => f.write_str("configured command is not a narumi server"),
            Self::NarumiError { code } => write!(f, "narumi returned error `{code}`"),
            Self::NarumiInvalidResponse => f.write_str("narumi returned an unexpected response"),
            Self::ResolverFailed => f.write_str("resolver failed unexpectedly (see server log)"),
        }
    }
}

/// 本文に付ける注記。`reason` に `; ` 連結で載せる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    ContentTruncated {
        chars: usize,
        original: usize,
    },
    BodyTruncated {
        bytes: u64,
    },
    ControlCharsRemoved,
    NonUtf8Replaced,
    HtmlAsIs,
    NarumiVersion {
        version: u32,
        available: Vec<u32>,
        generated_at: String,
        provider: String,
    },
    UnresolvedSpeakers {
        count: usize,
    },
    /// `resolved=false` かつ `reference.snapshot` がある。
    SnapshotFallback,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentTruncated { chars, original } => {
                write!(f, "content truncated to {chars} of {original} chars")
            }
            Self::BodyTruncated { bytes } => write!(f, "body truncated at {bytes} bytes"),
            Self::ControlCharsRemoved => f.write_str("control characters removed"),
            Self::NonUtf8Replaced => f.write_str("invalid utf-8 replaced"),
            Self::HtmlAsIs => f.write_str("html returned as-is"),
            Self::NarumiVersion {
                version,
                available,
                generated_at,
                provider,
            } => {
                let available = available
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "narumi minutes v{version} (available: {available}; generated_at {generated_at}; provider {provider})"
                )
            }
            Self::UnresolvedSpeakers { count } => write!(f, "{count} unresolved speakers"),
            Self::SnapshotFallback => f.write_str("fallback: see reference.snapshot"),
        }
    }
}

pub fn join_notes(notes: &[Note]) -> Option<String> {
    if notes.is_empty() {
        None
    } else {
        Some(
            notes
                .iter()
                .map(Note::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Unconfigured { setting: &'static str },
}

pub trait SourceResolver: Send + Sync + 'static {
    /// 登録名（`file` / `url` / `narumi`）。
    fn system(&self) -> &'static str;
    fn availability(&self, settings: &SourcesConfig) -> Availability;
    /// 同時実行の上限。超過は `busy`。
    fn max_concurrency(&self) -> usize;
    /// 外部取得。panic せず、失敗は `Unresolved` で返す。DB には触れない。
    fn resolve(&self, request: ResolveRequest<'_>) -> Result<Resolved, Unresolved>;
}

/// 呼び出しごとに現在の `[sources]` を返す。失敗は fail-closed（全解決器が `resolved=false`）。
pub trait SourceSettings: Send + Sync + 'static {
    fn current(&self) -> Result<SourcesConfig, String>;
}

/// テスト・組込み用の固定設定。
pub struct StaticSettings(pub SourcesConfig);

impl SourceSettings for StaticSettings {
    fn current(&self) -> Result<SourcesConfig, String> {
        Ok(self.0.clone())
    }
}

/// 設定ファイルを呼び出しごとに読み直す（`AuthTable::from_path` と同じ思想）。
pub struct ConfigFileSettings {
    path: PathBuf,
}

impl ConfigFileSettings {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl SourceSettings for ConfigFileSettings {
    fn current(&self) -> Result<SourcesConfig, String> {
        Config::load(&self.path)
            .map(|config| config.sources)
            .map_err(|e| e.to_string())
    }
}

/// file 解決器が常時拒否する領域。呼び出し側（CLI / desktop）が組み立てる。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtectedPaths {
    pub config_dir: PathBuf,
    pub db_dir: PathBuf,
    pub extra: Vec<PathBuf>,
}

impl ProtectedPaths {
    pub fn new(config_dir: impl AsRef<Path>, db_dir: impl AsRef<Path>) -> Self {
        Self {
            config_dir: config_dir.as_ref().to_path_buf(),
            db_dir: db_dir.as_ref().to_path_buf(),
            extra: Vec::new(),
        }
    }

    pub fn with_extra(mut self, path: impl AsRef<Path>) -> Self {
        self.extra.push(path.as_ref().to_path_buf());
        self
    }

    pub fn all(&self) -> impl Iterator<Item = &Path> {
        [self.config_dir.as_path(), self.db_dir.as_path()]
            .into_iter()
            .chain(self.extra.iter().map(PathBuf::as_path))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("resolver for system `{0}` is already registered")]
    Duplicate(String),
}

/// 同時実行の単純なカウンタ。`try_acquire` が上限なら `None`。
struct Gate {
    limit: usize,
    active: Mutex<usize>,
}

impl Gate {
    fn try_acquire(&self) -> Option<Permit<'_>> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if *active >= self.limit {
            return None;
        }
        *active += 1;
        Some(Permit { gate: self })
    }
}

/// drop で減算する RAII。
pub struct Permit<'a> {
    gate: &'a Gate,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut active = self.gate.active.lock().unwrap_or_else(|e| e.into_inner());
        *active = active.saturating_sub(1);
    }
}

struct Entry {
    resolver: Arc<dyn SourceResolver>,
    gate: Gate,
}

pub struct SourceRegistry {
    entries: BTreeMap<&'static str, Entry>,
    settings: Arc<dyn SourceSettings>,
}

impl SourceRegistry {
    /// 何も解決しない。`ToolService::new` の既定。
    pub fn empty() -> Self {
        Self::new(Arc::new(StaticSettings(SourcesConfig::default())))
    }

    pub fn new(settings: Arc<dyn SourceSettings>) -> Self {
        Self {
            entries: BTreeMap::new(),
            settings,
        }
    }

    pub fn register(&mut self, resolver: Arc<dyn SourceResolver>) -> Result<(), SourceError> {
        let system = resolver.system();
        if self.entries.contains_key(system) {
            return Err(SourceError::Duplicate(system.to_string()));
        }
        let gate = Gate {
            limit: resolver.max_concurrency().max(1),
            active: Mutex::new(0),
        };
        self.entries.insert(system, Entry { resolver, gate });
        Ok(())
    }

    fn key(system: &str) -> String {
        system.trim().to_ascii_lowercase()
    }

    /// trim ＋ ASCII 小文字化 ＋ 完全一致。
    pub fn get(&self, system: &str) -> Option<&Arc<dyn SourceResolver>> {
        self.entries
            .get(Self::key(system).as_str())
            .map(|entry| &entry.resolver)
    }

    /// `None` = 上限到達（busy）または未登録。
    pub fn acquire(&self, system: &str) -> Option<Permit<'_>> {
        self.entries
            .get(Self::key(system).as_str())
            .and_then(|entry| entry.gate.try_acquire())
    }

    pub fn systems(&self) -> Vec<String> {
        self.entries.keys().map(|k| k.to_string()).collect()
    }

    pub fn settings(&self) -> Result<SourcesConfig, String> {
        self.settings.current()
    }

    /// `Availability::Ready` の解決器名のみ。
    pub fn ready_systems(&self, settings: &SourcesConfig) -> Vec<String> {
        self.entries
            .values()
            .filter(|entry| entry.resolver.availability(settings) == Availability::Ready)
            .map(|entry| entry.resolver.system().to_string())
            .collect()
    }
}

/// 本文の整形: 先頭 BOM の除去 → C0 制御文字（`\t` `\n` `\r` を除く）・DEL の除去 →
/// Unicode スカラー数 `max_chars` で切り詰め。gaia が生成した行は本文に混ぜない。
pub fn shape_content(text: String, max_chars: usize) -> (String, Vec<Note>) {
    let mut notes = Vec::new();
    let text = text
        .strip_prefix('\u{feff}')
        .map(str::to_owned)
        .unwrap_or(text);
    let mut cleaned = String::with_capacity(text.len());
    let mut removed = false;
    for ch in text.chars() {
        let control = (ch.is_control() && !matches!(ch, '\t' | '\n' | '\r')) || ch == '\u{7f}';
        if control {
            removed = true;
        } else {
            cleaned.push(ch);
        }
    }
    if removed {
        notes.push(Note::ControlCharsRemoved);
    }
    let original = cleaned.chars().count();
    if original > max_chars {
        let cut = cleaned
            .char_indices()
            .nth(max_chars)
            .map(|(index, _)| index)
            .unwrap_or(cleaned.len());
        cleaned.truncate(cut);
        notes.push(Note::ContentTruncated {
            chars: max_chars,
            original,
        });
    }
    (cleaned, notes)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::{Arc, Barrier};

    pub(crate) enum StubBehavior {
        Ok(String),
        Unavailable(Reason),
        Internal(String),
    }

    /// テスト用の解決器（system = `stub`）。挙動を切り替え、Barrier で待たせられる。
    pub(crate) struct StubResolver {
        pub(crate) behavior: Mutex<StubBehavior>,
        pub(crate) ready: bool,
        pub(crate) concurrency: usize,
        pub(crate) barrier: Mutex<Option<Arc<Barrier>>>,
        pub(crate) calls: Mutex<usize>,
    }

    impl StubResolver {
        pub(crate) fn new(behavior: StubBehavior) -> Self {
            Self {
                behavior: Mutex::new(behavior),
                ready: true,
                concurrency: 4,
                barrier: Mutex::new(None),
                calls: Mutex::new(0),
            }
        }

        pub(crate) fn calls(&self) -> usize {
            *self.calls.lock().unwrap()
        }
    }

    impl SourceResolver for StubResolver {
        fn system(&self) -> &'static str {
            "stub"
        }

        fn availability(&self, _settings: &SourcesConfig) -> Availability {
            if self.ready {
                Availability::Ready
            } else {
                Availability::Unconfigured {
                    setting: "[sources.stub]",
                }
            }
        }

        fn max_concurrency(&self) -> usize {
            self.concurrency
        }

        fn resolve(&self, request: ResolveRequest<'_>) -> Result<Resolved, Unresolved> {
            *self.calls.lock().unwrap() += 1;
            assert!(!request.reference.uri.is_empty());
            if let Some(barrier) = self.barrier.lock().unwrap().clone() {
                barrier.wait();
            }
            match &*self.behavior.lock().unwrap() {
                StubBehavior::Ok(content) => Ok(Resolved {
                    content: content.clone(),
                    notes: vec![],
                }),
                StubBehavior::Unavailable(reason) => Err(Unresolved::Unavailable(reason.clone())),
                StubBehavior::Internal(detail) => Err(Unresolved::Internal(detail.clone())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_content_strips_bom_and_control_chars_but_keeps_whitespace_controls() {
        let (text, notes) = shape_content("\u{feff}a\u{0}b\tc\nd\re\u{7f}f".into(), 100);
        assert_eq!(text, "ab\tc\nd\ref");
        assert_eq!(notes, vec![Note::ControlCharsRemoved]);
        let (text, notes) = shape_content("plain".into(), 100);
        assert_eq!(text, "plain");
        assert!(notes.is_empty());
        assert_eq!(join_notes(&notes), None);
    }

    #[test]
    fn shape_content_truncates_on_scalar_boundaries() {
        let original = "日本語のテキストと絵文字🙂と結合文字e\u{301}x";
        let count = original.chars().count();
        let (text, notes) = shape_content(original.into(), 13);
        assert_eq!(text.chars().count(), 13);
        assert_eq!(text, "日本語のテキストと絵文字🙂");
        assert_eq!(
            notes,
            vec![Note::ContentTruncated {
                chars: 13,
                original: count
            }]
        );
        assert_eq!(
            join_notes(&notes).unwrap(),
            format!("content truncated to 13 of {count} chars")
        );
        // ちょうど上限なら切らない
        let (_, notes) = shape_content("abc".into(), 3);
        assert!(notes.is_empty());
    }

    #[test]
    fn registry_rejects_duplicates_normalizes_lookups_and_limits_concurrency() {
        let mut registry = SourceRegistry::empty();
        let mut stub = test_support::StubResolver::new(test_support::StubBehavior::Ok("x".into()));
        stub.concurrency = 1;
        registry.register(Arc::new(stub)).unwrap();
        assert!(matches!(
            registry.register(Arc::new(test_support::StubResolver::new(
                test_support::StubBehavior::Ok("y".into())
            ))),
            Err(SourceError::Duplicate(s)) if s == "stub"
        ));
        assert!(registry.get("  Stub ").is_some());
        assert!(registry.get("stubs").is_none());
        assert_eq!(registry.systems(), vec!["stub".to_string()]);
        let first = registry.acquire("STUB").expect("first permit");
        assert!(registry.acquire("stub").is_none(), "limit reached");
        assert!(registry.acquire("unknown").is_none());
        drop(first);
        assert!(registry.acquire("stub").is_some());
    }

    #[test]
    fn ready_systems_reflects_availability() {
        let mut registry = SourceRegistry::empty();
        let mut unconfigured =
            test_support::StubResolver::new(test_support::StubBehavior::Ok("x".into()));
        unconfigured.ready = false;
        registry.register(Arc::new(unconfigured)).unwrap();
        let settings = registry.settings().unwrap();
        assert!(registry.ready_systems(&settings).is_empty());
        let mut registry = SourceRegistry::empty();
        registry
            .register(Arc::new(test_support::StubResolver::new(
                test_support::StubBehavior::Ok("x".into()),
            )))
            .unwrap();
        assert_eq!(registry.ready_systems(&settings), vec!["stub".to_string()]);
    }

    #[test]
    fn reason_and_note_display_are_fixed_phrases() {
        let cases = [
            (
                Reason::NoResolver {
                    system: "minutes".into(),
                    available: vec!["file".into(), "url".into()],
                },
                "no resolver for system `minutes` (available: file, url)",
            ),
            (
                Reason::NoResolver {
                    system: "x".into(),
                    available: vec![],
                },
                "no resolver for system `x` (available: none)",
            ),
            (
                Reason::NotConfigured {
                    system: "file",
                    setting: "[sources.file].roots",
                },
                "resolver `file` is not configured (set [sources.file].roots)",
            ),
            (
                Reason::SettingsUnavailable,
                "source settings could not be loaded",
            ),
            (
                Reason::InvalidUri {
                    system: "narumi",
                    rule: "meeting_id",
                },
                "reference uri does not follow the `narumi` convention (meeting_id)",
            ),
            (
                Reason::FileUnavailable,
                "file is not available under the configured roots",
            ),
            (Reason::BinaryContent, "content is binary, not text"),
            (
                Reason::TooLarge,
                "content exceeds the configured size limit",
            ),
            (
                Reason::UrlNotAllowed {
                    rule: UrlRule::Address,
                },
                "url is not allowed (address)",
            ),
            (
                Reason::UpstreamStatus { status: 404 },
                "upstream responded with HTTP 404",
            ),
            (
                Reason::UnsupportedContentType,
                "upstream content type is not text",
            ),
            (
                Reason::TimedOut { secs: 15 },
                "resolution timed out after 15s",
            ),
            (Reason::ConnectionFailed, "connection to upstream failed"),
            (Reason::ReadFailed, "reading the content failed"),
            (
                Reason::NarumiStartFailed,
                "narumi command could not be started",
            ),
            (
                Reason::NarumiHandshakeFailed,
                "narumi did not complete the MCP handshake",
            ),
            (
                Reason::NarumiNotNarumi,
                "configured command is not a narumi server",
            ),
            (
                Reason::NarumiError {
                    code: "scope_denied".into(),
                },
                "narumi returned error `scope_denied`",
            ),
            (
                Reason::NarumiInvalidResponse,
                "narumi returned an unexpected response",
            ),
            (
                Reason::ResolverFailed,
                "resolver failed unexpectedly (see server log)",
            ),
        ];
        for (reason, expected) in cases {
            assert_eq!(reason.to_string(), expected);
        }
        let notes = [
            Note::BodyTruncated { bytes: 1024 },
            Note::NonUtf8Replaced,
            Note::HtmlAsIs,
            Note::NarumiVersion {
                version: 2,
                available: vec![1, 2],
                generated_at: "2026-08-27T03:05:00Z".into(),
                provider: "none".into(),
            },
            Note::UnresolvedSpeakers { count: 3 },
            Note::SnapshotFallback,
        ];
        assert_eq!(
            join_notes(&notes).unwrap(),
            "body truncated at 1024 bytes; invalid utf-8 replaced; html returned as-is; \
             narumi minutes v2 (available: 1, 2; generated_at 2026-08-27T03:05:00Z; provider none); \
             3 unresolved speakers; fallback: see reference.snapshot"
        );
    }

    #[test]
    fn config_file_settings_reload_per_call_and_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let settings = ConfigFileSettings::new(path.clone());
        assert!(settings.current().is_err(), "missing file is fail-closed");
        let mut config = Config::default();
        config.sources.file.roots = vec![dir.path().to_path_buf()];
        config.save(&path).unwrap();
        assert_eq!(settings.current().unwrap().file.roots.len(), 1);
        Config::default().save(&path).unwrap();
        assert!(settings.current().unwrap().file.roots.is_empty());
        std::fs::write(&path, "[sources]\nmax_content_chars = 1\n").unwrap();
        assert!(settings.current().is_err());
    }
}
