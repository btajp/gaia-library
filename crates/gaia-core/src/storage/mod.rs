//! SQLite 接続・PRAGMA・マイグレーション。仕様書 §5。
//! 内容層への SELECT は必ず `scope IN (SELECT value FROM json_each(?))` を付ける（各リポジトリの責務）。
#[cfg(unix)]
use std::path::PathBuf;
use std::{fs, path::Path, sync::Mutex, time::Duration};

use rusqlite::{Connection, Transaction, TransactionBehavior};
use rusqlite_migration::{M, Migrations};

use crate::error::ToolError;

const MIGRATION_SLICE: &[M<'static>] = &[M::up(include_str!("../../migrations/0001_init.sql"))];
pub const MIGRATIONS: Migrations<'static> = Migrations::from_slice(MIGRATION_SLICE);

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Migration(#[from] rusqlite_migration::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0}")]
    Integrity(String),
    #[error("SQLite rejected journal_mode={expected}; actual mode is `{actual}`")]
    JournalMode {
        expected: &'static str,
        actual: String,
    },
}

impl From<StorageError> for ToolError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::Sqlite(e) => ToolError::from(e),
            StorageError::NotFound(what) => ToolError::not_found(format!("{what} not found")),
            StorageError::Integrity(msg) => ToolError::invalid_params(msg),
            other => ToolError::internal(other.to_string()),
        }
    }
}

/// `Connection` は `Sync` ではないので Mutex で直列化する。個人 CRM 規模では単一接続で足りる。
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        prepare_database_path(path)?;
        let mut conn = Connection::open(path)?;
        configure(&mut conn, "wal")?;
        MIGRATIONS.to_latest(&mut conn)?;
        secure_database_files(path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let mut conn = Connection::open_in_memory()?;
        configure(&mut conn, "memory")?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn with_conn<T, E: From<StorageError>>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, E>,
    ) -> Result<T, E> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| StorageError::Integrity("db mutex poisoned".into()))?;
        f(&guard)
    }

    /// `BEGIN IMMEDIATE` のトランザクション。閉包が `Err` を返すか panic すれば rollback される。
    pub fn with_tx<T, E: From<StorageError>>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| StorageError::Integrity("db mutex poisoned".into()))?;
        let tx = guard.transaction().map_err(StorageError::from)?;
        let out = f(&tx)?;
        tx.commit().map_err(StorageError::from)?;
        Ok(out)
    }
}

fn prepare_database_path(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_private_directories(parent)?;
    }

    #[cfg(unix)]
    {
        use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};

        // SQLite は WAL/SHM の作成 mode を本体 DB から導出するため、DB を先に 0600 で用意する。
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
        {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        secure_database_files(path)?;
    }

    Ok(())
}

fn create_private_directories(path: &Path) -> Result<(), StorageError> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }

    match create_private_directory(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if path.is_dir() {
                Ok(())
            } else {
                Err(e.into())
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                create_private_directories(parent)?;
            }
            match create_private_directory(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn create_private_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700).create(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

fn secure_database_files(path: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        // open 前は既存 sidecar、open 後は新規 sidecar を同じ関数で補正する。
        set_private_file_permissions(path)?;
        for suffix in ["-wal", "-shm"] {
            set_private_file_permissions_if_exists(&sqlite_sidecar_path(path, suffix))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(unix)]
fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(StorageError::Integrity(format!(
            "database file is not a regular file: {}",
            path.display()
        )));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions_if_exists(path: &Path) -> Result<(), StorageError> {
    match fs::metadata(path) {
        Ok(_) => set_private_file_permissions(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub mod affiliations;
pub mod audit;
pub mod engagements;
pub mod entities;
pub mod facts;
pub mod glossary;
pub mod interactions;
pub mod organizations;
pub mod people;
pub mod proposals;
pub mod refs;
pub mod targets;

/// insert 時の必須文字列。trim して空なら Integrity エラー。
pub(crate) fn required<'a>(value: Option<&'a str>, what: &str) -> Result<&'a str, StorageError> {
    match value.map(str::trim) {
        Some(v) if !v.is_empty() => Ok(v),
        _ => Err(StorageError::Integrity(format!("{what} is required"))),
    }
}

/// DB の TEXT 列を契約 enum（typify 生成の FromStr 実装）へ変換する。
pub(crate) fn parse_db_enum<T>(raw: &str, what: &str) -> Result<T, StorageError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    raw.parse()
        .map_err(|e: T::Err| StorageError::Integrity(format!("invalid {what} `{raw}` in db: {e}")))
}

fn configure(
    conn: &mut Connection,
    expected_journal_mode: &'static str,
) -> Result<(), StorageError> {
    let actual: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !actual.eq_ignore_ascii_case(expected_journal_mode) {
        return Err(StorageError::JournalMode {
            expected: expected_journal_mode,
            actual,
        });
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(Duration::from_millis(5000))?;
    conn.set_transaction_behavior(TransactionBehavior::Immediate);
    Ok(())
}

/// `LIKE ?n ESCAPE '\'` 用のパターン。`%` `_` `\` をエスケープして両端に `%` を付ける。
pub fn like_pattern(needle: &str) -> String {
    let mut out = String::with_capacity(needle.len() + 2);
    out.push('%');
    for ch in needle.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn migrations_are_valid() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn open_in_memory_applies_schema_and_pragmas() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let jm: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
            assert_eq!(jm, "memory");
            let fk: i64 = c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
            assert_eq!(fk, 1);
            let n: i64 = c.query_one(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('people','facts','proposals','audit_log','engagement_people')",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 5);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn configure_rejects_an_unexpected_journal_mode() {
        let mut conn = Connection::open_in_memory().unwrap();

        let err = configure(&mut conn, "wal").unwrap_err();

        assert!(matches!(
            err,
            StorageError::JournalMode {
                expected: "wal",
                ref actual,
            } if actual == "memory"
        ));
    }

    #[test]
    fn open_file_uses_wal_creates_parent_and_sets_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("sub").join("private").join("gaia.db");
        let db = Db::open(&db_path).unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let jm: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
            assert_eq!(jm, "wal");
            let uv: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(uv, 1);
            Ok(())
        })
        .unwrap();
        #[cfg(unix)]
        {
            assert_eq!(mode(db_path.parent().unwrap()), 0o700);
            assert_eq!(mode(&dir.path().join("sub")), 0o700);
            assert_eq!(mode(&db_path), 0o600);
            assert_eq!(mode(&sqlite_sidecar_path(&db_path, "-wal")), 0o600);
            assert_eq!(mode(&sqlite_sidecar_path(&db_path, "-shm")), 0o600);
        }
        // 2 回目の open は冪等
        drop(db);
        Db::open(&db_path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn open_file_secures_existing_database_without_changing_existing_parent() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("shared");
        fs::create_dir(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        let db_path = parent.join("gaia.db");
        fs::write(&db_path, []).unwrap();
        fs::set_permissions(&db_path, fs::Permissions::from_mode(0o644)).unwrap();
        for suffix in ["-wal", "-shm"] {
            let sidecar = sqlite_sidecar_path(&db_path, suffix);
            fs::write(&sidecar, []).unwrap();
            fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o644)).unwrap();
        }

        prepare_database_path(&db_path).unwrap();

        assert_eq!(mode(&parent), 0o755);
        assert_eq!(mode(&db_path), 0o600);
        for suffix in ["-wal", "-shm"] {
            let sidecar = sqlite_sidecar_path(&db_path, suffix);
            assert_eq!(mode(&sidecar), 0o600);
            fs::remove_file(sidecar).unwrap();
        }

        let db = Db::open(&db_path).unwrap();

        assert_eq!(mode(&parent), 0o755);
        assert_eq!(mode(&db_path), 0o600);
        assert_eq!(mode(&sqlite_sidecar_path(&db_path, "-wal")), 0o600);
        assert_eq!(mode(&sqlite_sidecar_path(&db_path, "-shm")), 0o600);
        drop(db);
    }

    #[test]
    fn fts_stays_in_sync_through_insert_update_delete() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
            c.execute_batch("INSERT INTO affiliations(name) VALUES ('t'); INSERT INTO people(name) VALUES ('p');")?;
            c.execute(
                "INSERT INTO facts(entity_type, entity_id, statement, kind, scope) VALUES ('person', 1, 'トライグラム検索の確認', 'fact', 't')",
                [],
            )?;
            let hit = |c: &Connection| -> Result<i64, StorageError> {
                Ok(c.query_one("SELECT count(*) FROM facts_fts WHERE facts_fts MATCH 'グラム'", [], |r| r.get(0))?)
            };
            assert_eq!(hit(c)?, 1);
            c.execute("UPDATE facts SET statement = '別の文' WHERE id = 1", [])?;
            assert_eq!(hit(c)?, 0);
            c.execute("DELETE FROM facts WHERE id = 1", [])?;
            // rank=1 の integrity-check は外部コンテンツ表と索引の不一致を検出する
            c.execute("INSERT INTO facts_fts(facts_fts, rank) VALUES ('integrity-check', 1)", [])?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn with_tx_rolls_back_on_error() {
        let db = Db::open_in_memory().unwrap();
        let r: Result<(), StorageError> = db.with_tx(|tx| {
            tx.execute("INSERT INTO organizations(name) VALUES ('x')", [])?;
            Err(StorageError::Integrity("boom".into()))
        });
        assert!(r.is_err());
        let n: i64 = db
            .with_conn::<_, StorageError>(|c| {
                Ok(c.query_one("SELECT count(*) FROM organizations", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("a%b_c\\"), "%a\\%b\\_c\\\\%");
        assert_eq!(like_pattern("岡村"), "%岡村%");
    }

    #[test]
    fn storage_error_maps_to_tool_error_codes() {
        use crate::error::ErrorCode;
        assert_eq!(
            ToolError::from(StorageError::NotFound("person 1".into())).code,
            ErrorCode::NotFound
        );
        assert_eq!(
            ToolError::from(StorageError::Integrity("x".into())).code,
            ErrorCode::InvalidParams
        );
    }
}
