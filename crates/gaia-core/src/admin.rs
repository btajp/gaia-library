//! affiliations の管理。提案キュー原則の唯一の例外（機密境界そのものの定義のため）。
//! 必ず audit_log(admin_write) に残す。CLI / デスクトップの管理操作からのみ呼ぶ。
use serde_json::json;

use crate::{
    error::ToolError,
    storage::{Db, affiliations, audit},
};

pub fn add_affiliation(
    db: &Db,
    actor: &str,
    name: &str,
    identity: Option<&str>,
) -> Result<i64, ToolError> {
    db.with_tx(|tx| {
        let id = affiliations::insert(tx, name, identity)?;
        audit::record(
            tx,
            actor,
            "admin_write",
            &json!({"op": "add_affiliation", "name": name}),
        )?;
        Ok::<_, ToolError>(id)
    })
}

pub fn list_affiliations(db: &Db) -> Result<Vec<affiliations::Affiliation>, ToolError> {
    db.with_conn(|c| Ok::<_, ToolError>(affiliations::list(c)?))
}
