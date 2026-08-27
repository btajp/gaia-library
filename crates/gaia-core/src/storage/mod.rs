//! SQLite 接続・PRAGMA・マイグレーション。仕様書 §5。
//! 内容層への SELECT は必ず `scope IN (SELECT value FROM json_each(?))` を付ける（各リポジトリの責務）。
use std::{path::Path, sync::Mutex, time::Duration};

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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        configure(&mut conn)?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        let mut conn = Connection::open_in_memory()?;
        configure(&mut conn)?;
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

fn configure(conn: &mut Connection) -> Result<(), StorageError> {
    // in-memory では "memory" が返るので戻り値は見ない
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
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

    #[test]
    fn migrations_are_valid() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn open_in_memory_applies_schema_and_pragmas() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn::<_, StorageError>(|c| {
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
    fn open_file_uses_wal_creates_parent_and_sets_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("sub").join("gaia.db")).unwrap();
        db.with_conn::<_, StorageError>(|c| {
            let jm: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
            assert_eq!(jm, "wal");
            let uv: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(uv, 1);
            Ok(())
        })
        .unwrap();
        // 2 回目の open は冪等
        drop(db);
        Db::open(&dir.path().join("sub").join("gaia.db")).unwrap();
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
