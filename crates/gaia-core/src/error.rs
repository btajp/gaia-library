//! ツール層の構造化エラー。仕様書 §8.2。
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    ScopeDenied,
    Unauthorized,
    InvalidParams,
    ContractMismatch,
    Conflict,
    Busy,
    NotImplemented,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::ScopeDenied => "scope_denied",
            Self::Unauthorized => "unauthorized",
            Self::InvalidParams => "invalid_params",
            Self::ContractMismatch => "contract_mismatch",
            Self::Conflict => "conflict",
            Self::Busy => "busy",
            Self::NotImplemented => "not_implemented",
            Self::Internal => "internal",
        }
    }

    /// JSON-RPC エラーとして返すべき「プロトコル違反」か（それ以外は isError の結果として返す）。
    pub fn is_protocol_error(self) -> bool {
        matches!(
            self,
            Self::Unauthorized | Self::InvalidParams | Self::ContractMismatch
        )
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<Value>,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
    pub fn not_found(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, m)
    }
    pub fn scope_denied(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::ScopeDenied, m)
    }
    pub fn unauthorized(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, m)
    }
    pub fn invalid_params(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, m)
    }
    pub fn conflict(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, m)
    }
    pub fn busy(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Busy, m)
    }
    pub fn not_implemented(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotImplemented, m)
    }
    pub fn internal(m: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, m)
    }

    /// 契約の ErrorObject 形 `{ code, message, details }`。
    pub fn to_json(&self) -> Value {
        json!({ "code": self.code.as_str(), "message": self.message, "details": self.details })
    }
}

impl From<rusqlite::Error> for ToolError {
    fn from(e: rusqlite::Error) -> Self {
        let busy = matches!(
            &e,
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::DatabaseBusy || f.code == rusqlite::ErrorCode::DatabaseLocked
        );
        if busy {
            Self::busy(e.to_string())
        } else {
            Self::internal(format!("sqlite: {e}"))
        }
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self::internal(format!("json: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_value(ErrorCode::ScopeDenied).unwrap(),
            "scope_denied"
        );
        assert_eq!(ErrorCode::NotFound.as_str(), "not_found");
    }

    #[test]
    fn protocol_errors_are_unauthorized_invalid_params_contract_mismatch() {
        assert!(ErrorCode::Unauthorized.is_protocol_error());
        assert!(ErrorCode::InvalidParams.is_protocol_error());
        assert!(ErrorCode::ContractMismatch.is_protocol_error());
        assert!(!ErrorCode::NotFound.is_protocol_error());
        assert!(!ErrorCode::ScopeDenied.is_protocol_error());
    }

    #[test]
    fn tool_error_to_json_includes_details() {
        let e = ToolError::invalid_params("bad").with_details(serde_json::json!({"path": "/x"}));
        let v = e.to_json();
        assert_eq!(v["code"], "invalid_params");
        assert_eq!(v["message"], "bad");
        assert_eq!(v["details"]["path"], "/x");
        assert_eq!(e.to_string(), "invalid_params: bad");
    }

    #[test]
    fn busy_sqlite_error_maps_to_busy() {
        let e = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked".into()),
        );
        assert_eq!(ToolError::from(e).code, ErrorCode::Busy);
    }
}
