//! 構造化 predicate の初期レジストリ。仕様書 §8.6。頻出したものだけ後払いで昇格する。
use crate::error::ToolError;

pub const KNOWN_PREDICATES: &[&str] = &["role", "status", "interest", "decision"];

/// 承認時の規則: レジストリにある predicate は value 必須、レジストリ外は拒否（自由文のみで登録し直す）。
pub fn check(predicate: Option<&str>, value: Option<&str>) -> Result<(), ToolError> {
    match predicate {
        None if value.is_none() => Ok(()),
        None => Err(ToolError::invalid_params(
            "value cannot be set without predicate",
        )),
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

/// update の部分 patch で、DB を見ずに確定できる違反だけを拒否する。
/// predicate/value の最終的な組み合わせは既存 fact と合成して `check` する。
pub fn check_update_patch(predicate: Option<&str>, value: Option<&str>) -> Result<(), ToolError> {
    if let Some(p) = predicate
        && !KNOWN_PREDICATES.contains(&p)
    {
        return Err(ToolError::invalid_params(format!(
            "unknown predicate `{p}`; allowed: {}. Register other facts with statement only",
            KNOWN_PREDICATES.join(", ")
        )));
    }
    if value.is_some_and(|v| v.trim().is_empty()) {
        return Err(ToolError::invalid_params(
            "predicate value must not be empty",
        ));
    }
    Ok(())
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
        assert_eq!(
            check(None, Some("orphan")).unwrap_err().code,
            ErrorCode::InvalidParams
        );
    }

    #[test]
    fn update_patch_defers_valid_partial_combinations() {
        assert!(check_update_patch(None, Some("director")).is_ok());
        assert!(check_update_patch(Some("role"), None).is_ok());
        assert_eq!(
            check_update_patch(Some("mood"), None).unwrap_err().code,
            ErrorCode::InvalidParams
        );
        assert_eq!(
            check_update_patch(None, Some("  ")).unwrap_err().code,
            ErrorCode::InvalidParams
        );
    }
}
