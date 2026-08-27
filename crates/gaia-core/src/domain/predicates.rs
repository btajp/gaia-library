//! 構造化 predicate の初期レジストリ。仕様書 §8.6。頻出したものだけ後払いで昇格する。
use crate::error::ToolError;

pub const KNOWN_PREDICATES: &[&str] = &["role", "status", "interest", "decision"];

/// 承認時の規則: レジストリにある predicate は value 必須、レジストリ外は拒否（自由文のみで登録し直す）。
pub fn check(predicate: Option<&str>, value: Option<&str>) -> Result<(), ToolError> {
    match predicate {
        None => Ok(()),
        Some(p) if KNOWN_PREDICATES.contains(&p) => {
            if value.map(|v| !v.trim().is_empty()).unwrap_or(false) {
                Ok(())
            } else {
                Err(ToolError::invalid_params(format!(
                    "predicate `{p}` requires a non-empty value"
                )))
            }
        }
        Some(p) => Err(ToolError::invalid_params(format!(
            "unknown predicate `{p}`; allowed: {}. Register other facts with statement only",
            KNOWN_PREDICATES.join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn known_predicate_requires_value() {
        assert!(check(Some("role"), Some("CTO")).is_ok());
        assert_eq!(
            check(Some("role"), None).unwrap_err().code,
            ErrorCode::InvalidParams
        );
        assert_eq!(
            check(Some("role"), Some("  ")).unwrap_err().code,
            ErrorCode::InvalidParams
        );
    }

    #[test]
    fn unknown_predicate_is_rejected_and_none_is_free_text() {
        assert_eq!(
            check(Some("mood"), Some("x")).unwrap_err().code,
            ErrorCode::InvalidParams
        );
        assert!(check(None, None).is_ok());
    }
}
