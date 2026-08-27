use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use std::path::Path;

/// Tauri updater と同じ二重 base64 形式の公開鍵・署名を検証する。
///
/// `.sig` と `tauri.conf.json` の公開鍵は、minisign のテキスト形式をさらに
/// base64 化した値である。ここで実行時プラグインと同じ `minisign-verify` を使い、
/// 公開前の生成物が実際にインストール済みアプリから受理されることを確かめる。
pub fn verify_artifact(
    artifact_path: impl AsRef<Path>,
    signature_path: impl AsRef<Path>,
    encoded_public_key: &str,
) -> Result<(), String> {
    let artifact =
        std::fs::read(artifact_path).map_err(|_| "updater artifactを読み取れません".to_owned())?;
    let encoded_signature = std::fs::read_to_string(signature_path)
        .map_err(|_| "updater署名を読み取れません".to_owned())?;
    verify_bytes(&artifact, &encoded_signature, encoded_public_key)
}

pub fn verify_bytes(
    artifact: &[u8],
    encoded_signature: &str,
    encoded_public_key: &str,
) -> Result<(), String> {
    let public_key_text = decode_tauri_value(encoded_public_key, "updater公開鍵")?;
    let signature_text = decode_tauri_value(encoded_signature, "updater署名")?;
    let public_key = PublicKey::decode(&public_key_text)
        .map_err(|_| "updater公開鍵の形式が不正です".to_owned())?;
    let signature =
        Signature::decode(&signature_text).map_err(|_| "updater署名の形式が不正です".to_owned())?;

    public_key
        .verify(artifact, &signature, true)
        .map_err(|_| "updater署名が公開鍵または生成物と一致しません".to_owned())
}

fn decode_tauri_value(encoded: &str, label: &str) -> Result<String, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| format!("{label}がbase64形式ではありません"))?;
    String::from_utf8(decoded).map_err(|_| format!("{label}がUTF-8テキストではありません"))
}

#[cfg(test)]
mod tests {
    use super::verify_bytes;
    use base64::Engine;

    const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";

    fn encoded(value: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(value)
    }

    #[test]
    fn tauri形式の署名を生成物と公開鍵で検証する() {
        assert!(verify_bytes(b"test", &encoded(SIGNATURE), &encoded(PUBLIC_KEY)).is_ok());
    }

    #[test]
    fn 生成物が変われば署名検証を拒否する() {
        let result = verify_bytes(b"changed", &encoded(SIGNATURE), &encoded(PUBLIC_KEY));
        assert_eq!(
            result,
            Err("updater署名が公開鍵または生成物と一致しません".to_owned())
        );
    }

    #[test]
    fn 二重base64ではない公開鍵を拒否する() {
        let result = verify_bytes(b"test", &encoded(SIGNATURE), "not-base64");
        assert_eq!(
            result,
            Err("updater公開鍵がbase64形式ではありません".to_owned())
        );
    }
}
