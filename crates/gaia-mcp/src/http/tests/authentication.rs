use serde_json::json;

use super::support::*;
use crate::http::serve_http;

#[tokio::test]
async fn bearer_scheme_is_case_insensitive_and_accepts_one_or_more_spaces() {
    let (auth, human, _) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let session = initialize(addr, &human).await;
    for scheme in ["Bearer", "bearer", "BEARER", "bEaReR"] {
        for separator in [" ", "  ", "    "] {
            let body = json!({"jsonrpc":"2.0", "id":2, "method":"tools/call", "params":{
                "name":"get_server_info", "arguments":{}
            }});
            let result = response(
                addr,
                request(
                    addr,
                    "POST",
                    None,
                    Some(&session),
                    &format!("Authorization: {scheme}{separator}{human}\r\n"),
                    Some(&body),
                ),
            )
            .await;
            assert_eq!(
                status(&result),
                200,
                "scheme={scheme}, spaces={}",
                separator.len()
            );
            assert_eq!(
                rpc(&result)["result"]["structuredContent"]["client"]["name"],
                "me"
            );
        }
    }
    server.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_authorization_and_modified_tokens_fail_closed() {
    let (auth, human, _) = auth();
    let server = serve_http(service(), auth, Some(0)).await.unwrap();
    let addr = server.local_addr();
    let mut changed_key = human.clone();
    let replacement = if human.ends_with('0') { "1" } else { "0" };
    changed_key.replace_range(changed_key.len() - 1.., replacement);
    let invalid = [
        String::new(),
        "Bearer".into(),
        "Bearer ".into(),
        "Bearer    ".into(),
        "bearer invalid-key".into(),
        format!("Bearer {}", human.to_ascii_uppercase()),
        format!("Bearer {changed_key}"),
        format!("Basic {human}"),
        format!("Bearer{human}"),
        format!("Bearer\t{human}"),
        format!("Bearer \t{human}"),
        format!("Bearer\t {human}"),
        format!("Bearer {human} extra"),
        format!("Bearer {human}\textra"),
        format!("Bearer {human}, bearer {human}"),
    ];
    for method in ["POST", "GET", "DELETE"] {
        for (index, value) in invalid.iter().enumerate() {
            let result = response(
                addr,
                request(
                    addr,
                    method,
                    None,
                    Some("unknown-session"),
                    &format!("Authorization: {value}\r\n"),
                    None,
                ),
            )
            .await;
            assert_eq!(status(&result), 401, "{method}, invalid case {index}");
        }
        // 有効なキーを二重に送っても、scheme やヘッダ名の大小文字によらず拒否する。
        let duplicated =
            format!("Authorization: bearer {human}\r\nauthorization: BEARER {human}\r\n");
        let result = response(
            addr,
            request(
                addr,
                method,
                None,
                Some("unknown-session"),
                &duplicated,
                None,
            ),
        )
        .await;
        assert_eq!(status(&result), 401, "{method}, duplicate Authorization");
    }
    server.shutdown().await.unwrap();
}
