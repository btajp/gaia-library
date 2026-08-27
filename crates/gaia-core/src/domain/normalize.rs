//! 表示名の正規化。仕様書 §8.3 resolve_speakers。
//! NFKC → 括弧内除去 → 小文字化 → 前後空白除去 → 敬称除去 → 空白除去。
use unicode_normalization::UnicodeNormalization;

/// 末尾の敬称。空白を含む形は空白除去より前に処理する。
const HONORIFICS: &[&str] = &["さん", "様", "氏", "くん", "ちゃん", "先生", "-san", " san"];

pub fn normalize_name(input: &str) -> String {
    let nfkc: String = input.nfkc().collect();
    let mut without_parens = String::with_capacity(nfkc.len());
    let mut depth = 0usize;
    for ch in nfkc.chars() {
        match ch {
            '(' | '[' | '<' | '{' | '「' | '【' => depth += 1,
            ')' | ']' | '>' | '}' | '」' | '】' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ => without_parens.push(ch),
        }
    }
    let mut trimmed = without_parens.to_lowercase().trim().to_string();
    for suffix in HONORIFICS {
        if let Some(rest) = trimmed.strip_suffix(suffix)
            && !rest.trim().is_empty()
        {
            trimmed = rest.trim().to_string();
            break;
        }
    }
    trimmed.chars().filter(|c| !c.is_whitespace()).collect()
}

#[cfg(test)]
mod tests {
    use super::normalize_name;

    #[test]
    fn strips_parenthesized_suffix_and_spaces() {
        assert_eq!(normalize_name("岡村 慎太郎 (CloudNative)"), "岡村慎太郎");
        assert_eq!(
            normalize_name("岡村　慎太郎（クラウドネイティブ）"),
            "岡村慎太郎"
        );
    }

    #[test]
    fn lowercases_and_folds_fullwidth_via_nfkc() {
        assert_eq!(normalize_name("Okamura Shintaro"), "okamurashintaro");
        assert_eq!(normalize_name("Ｔａｎａｋａ Ｔａｒｏ"), "tanakataro");
        assert_eq!(normalize_name("ｵｶﾑﾗ ｼﾝﾀﾛｳ"), "オカムラシンタロウ");
    }

    #[test]
    fn strips_honorifics_only_when_something_remains() {
        assert_eq!(normalize_name("田中さん"), "田中");
        assert_eq!(normalize_name("田中 様"), "田中");
        assert_eq!(normalize_name("Tanaka-san"), "tanaka");
        assert_eq!(normalize_name("さん"), "さん");
    }

    #[test]
    fn empty_when_nothing_is_left() {
        assert_eq!(normalize_name("（外部）"), "");
        assert_eq!(normalize_name("   "), "");
    }
}
