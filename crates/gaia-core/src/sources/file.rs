//! file 解決器。`[sources.file].roots` 配下の通常ファイルだけを返す。
//! 手順: 字句検査 → canonicalize した実体パスと canonicalize した roots の包含 → `O_NOFOLLOW` で open →
//! 開いたハンドルの metadata で通常ファイル確認 → サイズ → NUL / 非 UTF-8 はバイナリとして拒否。
//! 不在・root 外・通常ファイル以外・権限不足はすべて同一文言（存在オラクルを作らない）。
use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::{Component, PathBuf},
};

use url::Url;

use crate::config::{FileSourceConfig, SourcesConfig};

use super::{
    Availability, ProtectedPaths, Reason, ResolveRequest, Resolved, SourceResolver, Unresolved,
};

const SETTING: &str = "[sources.file].roots";

pub struct FileResolver {
    protected: ProtectedPaths,
}

impl FileResolver {
    pub fn new(protected: ProtectedPaths) -> Self {
        Self { protected }
    }
}

impl SourceResolver for FileResolver {
    fn system(&self) -> &'static str {
        "file"
    }

    fn availability(&self, settings: &SourcesConfig) -> Availability {
        if settings.file.roots.is_empty() {
            Availability::Unconfigured { setting: SETTING }
        } else {
            Availability::Ready
        }
    }

    fn max_concurrency(&self) -> usize {
        4
    }

    fn resolve(&self, request: ResolveRequest<'_>) -> Result<Resolved, Unresolved> {
        let settings = &request.settings.file;
        if settings.roots.is_empty() {
            return Err(Unresolved::Unavailable(Reason::NotConfigured {
                system: "file",
                setting: SETTING,
            }));
        }
        read(&request.reference.uri, settings, &self.protected)
            .map(|content| Resolved {
                content,
                notes: Vec::new(),
            })
            .map_err(Unresolved::Unavailable)
    }
}

/// `file:///absolute/path`（RFC 8089。host は空か `localhost` のみ）。パーセントデコードは `to_file_path`。
pub fn parse_file_uri(uri: &str) -> Result<PathBuf, Reason> {
    let invalid = |rule: &'static str| Reason::InvalidUri {
        system: "file",
        rule,
    };
    let url = Url::parse(uri).map_err(|_| invalid("parse"))?;
    if url.scheme() != "file" {
        return Err(invalid("scheme"));
    }
    if !matches!(url.host_str(), None | Some("") | Some("localhost")) {
        return Err(invalid("host"));
    }
    let path = url.to_file_path().map_err(|_| invalid("path"))?;
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(invalid("path"));
    }
    Ok(path)
}

/// 呼び出しごとに roots を実効化する。失敗・非ディレクトリ・`/`・保護領域と祖先/子孫関係にある root は無視する。
fn effective_roots(settings: &FileSourceConfig, protected: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for root in &settings.roots {
        let Ok(canonical) = fs::canonicalize(root) else {
            tracing::warn!("file resolver: a configured root cannot be resolved; ignoring it");
            continue;
        };
        if !canonical.is_dir() || canonical.parent().is_none() || root.parent().is_none() {
            tracing::warn!(
                "file resolver: a configured root is not a usable directory; ignoring it"
            );
            continue;
        }
        if protected
            .iter()
            .any(|p| canonical.starts_with(p) || p.starts_with(&canonical))
        {
            tracing::warn!(
                "file resolver: a configured root overlaps the config / database / key directories; ignoring it"
            );
            continue;
        }
        roots.push(canonical);
    }
    roots
}

fn protected_paths(protected: &ProtectedPaths) -> Vec<PathBuf> {
    protected
        .all()
        .filter(|p| !p.as_os_str().is_empty())
        .flat_map(|p| {
            let canonical = fs::canonicalize(p).ok();
            std::iter::once(p.to_path_buf()).chain(canonical)
        })
        .collect()
}

fn read(
    uri: &str,
    settings: &FileSourceConfig,
    protected: &ProtectedPaths,
) -> Result<String, Reason> {
    let requested = parse_file_uri(uri)?;
    // 1. 字句検査: `..` / `.` / prefix を含むパスは正規化を試みずに拒否
    if requested.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::CurDir | Component::Prefix(_)
        )
    }) {
        return Err(Reason::FileUnavailable);
    }
    // 2. roots の実効化
    let protected = protected_paths(protected);
    let roots = effective_roots(settings, &protected);
    if roots.is_empty() {
        return Err(Reason::NotConfigured {
            system: "file",
            setting: SETTING,
        });
    }
    // 3. 字句包含: 設定値の root か実効 root のどれかに含まれなければファイルシステムに触れない
    let lexically_inside = settings
        .roots
        .iter()
        .chain(roots.iter())
        .any(|root| requested.starts_with(root));
    if !lexically_inside {
        return Err(Reason::FileUnavailable);
    }
    // 4. 実体パス（symlink を全部辿る）
    let canonical = fs::canonicalize(&requested).map_err(|_| Reason::FileUnavailable)?;
    // 5. 実体が実効 root 内にあり、保護領域の外であること
    if !roots.iter().any(|root| canonical.starts_with(root)) {
        return Err(Reason::FileUnavailable);
    }
    if protected.iter().any(|p| canonical.starts_with(p)) {
        return Err(Reason::FileUnavailable);
    }
    // 6. open（最終要素の symlink 差し替えに追従しない。FIFO で永久ブロックしない）
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let mut file = options
        .open(&canonical)
        .map_err(|_| Reason::FileUnavailable)?;
    let metadata = file.metadata().map_err(|_| Reason::FileUnavailable)?;
    if !metadata.is_file() {
        return Err(Reason::FileUnavailable);
    }
    // 7. サイズ
    if metadata.len() > settings.max_bytes {
        return Err(Reason::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if let Err(error) = (&mut file)
        .take(settings.max_bytes + 1)
        .read_to_end(&mut bytes)
    {
        tracing::warn!(kind = ?error.kind(), "file resolver: read failed");
        return Err(Reason::ReadFailed);
    }
    if bytes.len() as u64 > settings.max_bytes {
        return Err(Reason::TooLarge);
    }
    // 8. 内容: NUL を含む、または UTF-8 として不正ならバイナリ
    if bytes.contains(&0) {
        return Err(Reason::BinaryContent);
    }
    String::from_utf8(bytes).map_err(|_| Reason::BinaryContent)
}

#[cfg(test)]
pub(crate) fn path_to_file_uri(path: &std::path::Path) -> String {
    Url::from_file_path(path)
        .expect("absolute path converts to a file URL")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::types::{RefTargetType, Reference};
    use std::path::Path;

    fn settings(roots: &[&Path]) -> FileSourceConfig {
        FileSourceConfig {
            roots: roots.iter().map(|r| r.to_path_buf()).collect(),
            max_bytes: 64,
        }
    }

    fn no_protection() -> ProtectedPaths {
        ProtectedPaths::new("/nonexistent/gaia-config", "/nonexistent/gaia-data")
    }

    fn read_path(path: &Path, s: &FileSourceConfig, p: &ProtectedPaths) -> Result<String, Reason> {
        read(&path_to_file_uri(path), s, p)
    }

    #[test]
    fn reads_regular_files_and_decodes_percent_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let file = root.join("議事 録 v1.md");
        fs::write(&file, "# 議事録\n決定事項").unwrap();
        let s = settings(&[&root]);
        let uri = path_to_file_uri(&file);
        assert!(uri.contains("%20"), "{uri}");
        assert_eq!(
            read(&uri, &s, &no_protection()).unwrap(),
            "# 議事録\n決定事項"
        );
        // symlink を辿った root（macOS の /var → /private/var）でも同じ
        let canonical_root = fs::canonicalize(&root).unwrap();
        assert_eq!(
            read_path(&canonical_root.join("議事 録 v1.md"), &s, &no_protection()).unwrap(),
            "# 議事録\n決定事項"
        );
    }

    #[test]
    fn invalid_uris_are_reported_as_convention_violations() {
        let s = settings(&[Path::new("/tmp")]);
        for (uri, rule) in [
            ("file://otherhost/etc/passwd", "host"),
            ("relative/path.md", "parse"),
            ("file:///etc/pass%00wd", "path"),
            ("https://example.com/x.md", "scheme"),
        ] {
            assert_eq!(
                read(uri, &s, &no_protection()).unwrap_err(),
                Reason::InvalidUri {
                    system: "file",
                    rule
                },
                "{uri}"
            );
        }
    }

    #[test]
    fn rejects_traversal_outside_roots_directories_and_binary_with_one_phrase() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        fs::write(root.join("bin.dat"), b"ab\0cd").unwrap();
        fs::write(root.join("latin1.txt"), b"caf\xe9").unwrap();
        fs::write(root.join("big.txt"), [b'x'; 65]).unwrap();
        fs::create_dir(root.join("subdir")).unwrap();
        let s = settings(&[&root]);
        let p = no_protection();
        let unavailable = Reason::FileUnavailable;
        // `..` を含む
        assert_eq!(
            read(
                &format!("{}/../outside/secret.txt", path_to_file_uri(&root)),
                &s,
                &p
            )
            .unwrap_err(),
            unavailable
        );
        // root 外（存在する）と root 外（存在しない）が同一文言
        assert_eq!(
            read_path(&outside.join("secret.txt"), &s, &p).unwrap_err(),
            unavailable
        );
        assert_eq!(
            read_path(&outside.join("missing.txt"), &s, &p).unwrap_err(),
            unavailable
        );
        // root 内の不在
        assert_eq!(
            read_path(&root.join("missing.txt"), &s, &p).unwrap_err(),
            unavailable
        );
        // ディレクトリ
        assert_eq!(
            read_path(&root.join("subdir"), &s, &p).unwrap_err(),
            unavailable
        );
        assert_eq!(read_path(&root, &s, &p).unwrap_err(), unavailable);
        // バイナリ
        assert_eq!(
            read_path(&root.join("bin.dat"), &s, &p).unwrap_err(),
            Reason::BinaryContent
        );
        assert_eq!(
            read_path(&root.join("latin1.txt"), &s, &p).unwrap_err(),
            Reason::BinaryContent
        );
        // サイズ
        assert_eq!(
            read_path(&root.join("big.txt"), &s, &p).unwrap_err(),
            Reason::TooLarge
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_fifos_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        fs::write(root.join("inside.txt"), "inside").unwrap();
        symlink(outside.join("secret.txt"), root.join("escape.txt")).unwrap();
        symlink(&outside, root.join("escape-dir")).unwrap();
        symlink(root.join("inside.txt"), root.join("alias.txt")).unwrap();
        let s = settings(&[&root]);
        let p = no_protection();
        // root 外を指す symlink（ファイル・ディレクトリ）は拒否、root 内 → root 内は許容
        assert_eq!(
            read_path(&root.join("escape.txt"), &s, &p).unwrap_err(),
            Reason::FileUnavailable
        );
        assert_eq!(
            read_path(&root.join("escape-dir").join("secret.txt"), &s, &p).unwrap_err(),
            Reason::FileUnavailable
        );
        assert_eq!(
            read_path(&root.join("alias.txt"), &s, &p).unwrap(),
            "inside"
        );
        // FIFO は即座に拒否（ブロックしない）
        let fifo = root.join("pipe");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: 有効な NUL 終端パスを渡す。
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        assert_eq!(
            read_path(&fifo, &s, &p).unwrap_err(),
            Reason::FileUnavailable
        );
        // 権限不足も同一文言（root ユーザーでは読めてしまうので、その場合はスキップ）
        let locked = root.join("locked.txt");
        fs::write(&locked, "locked").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        // SAFETY: geteuid は引数を取らず常に成功する。
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(
                read_path(&locked, &s, &p).unwrap_err(),
                Reason::FileUnavailable
            );
        }
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn protected_and_unusable_roots_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let db_dir = dir.path().join("data");
        let keys = db_dir.join("keys");
        let root = dir.path().join("root");
        for d in [&config_dir, &db_dir, &keys, &root] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(config_dir.join("config.toml"), "x = 1").unwrap();
        fs::write(db_dir.join("gaia.db"), "sqlite").unwrap();
        fs::write(keys.join("k.key"), "gaia_secret").unwrap();
        fs::write(root.join("ok.txt"), "ok").unwrap();
        let protected = ProtectedPaths::new(&config_dir, &db_dir).with_extra(&keys);
        // 保護領域そのもの、その祖先（tempdir）、`/` を root にしても無視される
        let s = settings(&[&config_dir, &db_dir, dir.path(), Path::new("/")]);
        assert_eq!(
            read_path(&config_dir.join("config.toml"), &s, &protected).unwrap_err(),
            Reason::NotConfigured {
                system: "file",
                setting: SETTING
            }
        );
        // 有効な root があっても保護領域内のファイルは読めない
        let s = settings(&[&root, &db_dir]);
        assert_eq!(
            read_path(&root.join("ok.txt"), &s, &protected).unwrap(),
            "ok"
        );
        assert_eq!(
            read_path(&db_dir.join("gaia.db"), &s, &protected).unwrap_err(),
            Reason::FileUnavailable
        );
        assert_eq!(
            read_path(&keys.join("k.key"), &s, &protected).unwrap_err(),
            Reason::FileUnavailable
        );
        // 存在しない root は無視され、空になれば NotConfigured
        let s = settings(&[&dir.path().join("nope")]);
        assert_eq!(
            read_path(&root.join("ok.txt"), &s, &protected).unwrap_err(),
            Reason::NotConfigured {
                system: "file",
                setting: SETTING
            }
        );
    }

    #[test]
    fn resolver_availability_and_request_path() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "body").unwrap();
        let resolver = FileResolver::new(no_protection());
        assert_eq!(resolver.system(), "file");
        assert_eq!(resolver.max_concurrency(), 4);
        let mut config = SourcesConfig::default();
        assert_eq!(
            resolver.availability(&config),
            Availability::Unconfigured { setting: SETTING }
        );
        let reference = Reference {
            id: 1,
            target_type: RefTargetType::Fact,
            target_id: 1,
            system: "file".into(),
            uri: path_to_file_uri(&dir.path().join("a.md")),
            title: None,
            note: "n".into(),
            snapshot: None,
            scope: "cn".into(),
            last_verified: None,
            created_at: "now".into(),
        };
        assert_eq!(
            resolver
                .resolve(ResolveRequest {
                    reference: &reference,
                    settings: &config
                })
                .unwrap_err(),
            Unresolved::Unavailable(Reason::NotConfigured {
                system: "file",
                setting: SETTING
            })
        );
        config.file.roots = vec![dir.path().to_path_buf()];
        assert_eq!(resolver.availability(&config), Availability::Ready);
        let resolved = resolver
            .resolve(ResolveRequest {
                reference: &reference,
                settings: &config,
            })
            .unwrap();
        assert_eq!(resolved.content, "body");
    }
}
