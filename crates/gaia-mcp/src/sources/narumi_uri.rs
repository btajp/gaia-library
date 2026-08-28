//! narumi 参照 URI の解析（純関数）。規約: `narumi://meeting/<meeting_id>[?version=<n>][#<fragment>]`。
//! meeting_id は narumi 契約の形式（8 桁数字 `T` 6 桁数字 `Z-` 16 進小文字 8 桁）。fragment は無視する。
use gaia_core::sources::Reason;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarumiTarget {
    pub meeting_id: String,
    pub version: Option<u32>,
}

fn invalid(rule: &'static str) -> Reason {
    Reason::InvalidUri {
        system: "narumi",
        rule,
    }
}

/// `YYYYMMDDThhmmssZ-xxxxxxxx`（25 文字）を長さと文字種で検査する。正規表現には依存しない。
pub fn is_meeting_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 25 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    digits(0..8)
        && bytes[8] == b'T'
        && digits(9..15)
        && bytes[15] == b'Z'
        && bytes[16] == b'-'
        && bytes[17..25]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
}

pub fn parse_narumi_uri(uri: &str) -> Result<NarumiTarget, Reason> {
    let url = Url::parse(uri).map_err(|_| invalid("parse"))?;
    if url.scheme() != "narumi" {
        return Err(invalid("scheme"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("userinfo"));
    }
    if url.host_str() != Some("meeting") || url.port().is_some() {
        return Err(invalid("host"));
    }
    let path = url.path();
    let meeting_id = path.strip_prefix('/').ok_or_else(|| invalid("path"))?;
    if meeting_id.is_empty() || meeting_id.contains('/') {
        return Err(invalid("path"));
    }
    if !is_meeting_id(meeting_id) {
        return Err(invalid("meeting_id"));
    }
    let mut version = None;
    for (key, value) in url.query_pairs() {
        if key != "version" || version.is_some() {
            return Err(invalid("query"));
        }
        let parsed: u32 = value.parse().map_err(|_| invalid("query"))?;
        if parsed == 0 {
            return Err(invalid("query"));
        }
        version = Some(parsed);
    }
    if url.query().is_some_and(|q| !q.is_empty()) && version.is_none() {
        return Err(invalid("query"));
    }
    Ok(NarumiTarget {
        meeting_id: meeting_id.to_string(),
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "20260827T030500Z-a1b2c3d4";

    fn rule(uri: &str) -> &'static str {
        match parse_narumi_uri(uri).unwrap_err() {
            Reason::InvalidUri {
                system: "narumi",
                rule,
            } => rule,
            other => panic!("unexpected reason {other:?}"),
        }
    }

    #[test]
    fn parses_meeting_id_version_and_ignores_fragment() {
        assert_eq!(
            parse_narumi_uri(&format!("narumi://meeting/{ID}")).unwrap(),
            NarumiTarget {
                meeting_id: ID.into(),
                version: None
            }
        );
        assert_eq!(
            parse_narumi_uri(&format!("narumi://meeting/{ID}?version=2#t=1200")).unwrap(),
            NarumiTarget {
                meeting_id: ID.into(),
                version: Some(2)
            }
        );
        assert_eq!(
            parse_narumi_uri(&format!("narumi://meeting/{ID}#t=1200"))
                .unwrap()
                .version,
            None
        );
        assert!(is_meeting_id("20260101T000000Z-00000000"));
        assert!(is_meeting_id("99991231T235959Z-ffffffff"));
    }

    #[test]
    fn rejects_malformed_uris_without_touching_the_meeting() {
        assert_eq!(rule("not a uri"), "parse");
        assert_eq!(rule(&format!("https://meeting/{ID}")), "scheme");
        assert_eq!(rule(&format!("narumi://user@meeting/{ID}")), "userinfo");
        assert_eq!(rule(&format!("narumi://meeting:1/{ID}")), "host");
        assert_eq!(rule(&format!("narumi://Meeting/{ID}")), "host");
        assert_eq!(rule(&format!("narumi://minutes/{ID}")), "host");
        assert_eq!(rule("narumi://meeting/"), "path");
        assert_eq!(rule("narumi://meeting"), "path");
        assert_eq!(rule(&format!("narumi://meeting/{ID}/")), "path");
        assert_eq!(rule("narumi://meeting/../x"), "meeting_id");
        assert_eq!(
            rule("narumi://meeting/20260827T030500Z-A1B2C3D4"),
            "meeting_id"
        );
        assert_eq!(
            rule("narumi://meeting/20260827T030500Z-a1b2c3d"),
            "meeting_id"
        );
        assert_eq!(
            rule("narumi://meeting/20260827T030500Z_a1b2c3d4"),
            "meeting_id"
        );
        assert_eq!(
            rule("narumi://meeting/20260827X030500Z-a1b2c3d4"),
            "meeting_id"
        );
        assert_eq!(
            rule("narumi://meeting/2026082T0305000Z-a1b2c3d4"),
            "meeting_id"
        );
        assert_eq!(rule(&format!("narumi://meeting/{ID}?version=0")), "query");
        assert_eq!(rule(&format!("narumi://meeting/{ID}?version=x")), "query");
        assert_eq!(
            rule(&format!("narumi://meeting/{ID}?version=1&version=2")),
            "query"
        );
        assert_eq!(rule(&format!("narumi://meeting/{ID}?scope=x")), "query");
        assert_eq!(
            rule(&format!("narumi://meeting/{ID}?version=1&scope=x")),
            "query"
        );
        assert_eq!(rule(&format!("narumi://meeting/{ID}?x")), "query");
    }
}
