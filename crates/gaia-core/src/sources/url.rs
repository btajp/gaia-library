//! url 解決器。http / https の GET のみ。ホスト allow ＋ DNS 解決後のアドレス検査 ＋ 自前のリダイレクト追従。
//! プロキシ環境変数と圧縮伸長は使わず、Cookie / Authorization は送らない。応答はテキスト系 Content-Type と
//! サイズ上限に限る。上流の文字列は reason に入れない（詳細は stderr の warn のみ）。
use std::{
    io::Read,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ureq::{
    Agent,
    config::Config,
    http::{Uri, header},
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{DefaultConnector, NextTimeout},
    },
};
use url::{Host, Url};

use crate::config::{SourcesConfig, UrlSourceConfig};

use super::{
    Availability, Note, Reason, ResolveRequest, Resolved, SourceResolver, Unresolved, UrlRule,
    net::{host_is_allowed, ip_is_public},
};

const SETTING: &str = "[sources.url].allow_hosts";
const ACCEPT: &str = "text/markdown, text/plain, application/json, text/html;q=0.8, */*;q=0.1";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// 接続先アドレスの方針。本番は `PublicOnly`。`AllowLoopback` はテストからしか到達できない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressPolicy {
    PublicOnly,
    #[cfg(test)]
    AllowLoopback,
}

impl AddressPolicy {
    fn allows(self, ip: IpAddr) -> bool {
        match self {
            Self::PublicOnly => ip_is_public(ip),
            #[cfg(test)]
            Self::AllowLoopback => ip.is_loopback() || ip_is_public(ip),
        }
    }
}

pub struct UrlResolver {
    policy: AddressPolicy,
}

impl UrlResolver {
    pub fn public_only() -> Self {
        Self {
            policy: AddressPolicy::PublicOnly,
        }
    }
}

impl SourceResolver for UrlResolver {
    fn system(&self) -> &'static str {
        "url"
    }

    fn availability(&self, settings: &SourcesConfig) -> Availability {
        if settings.url.allow_hosts.is_empty() {
            Availability::Unconfigured { setting: SETTING }
        } else {
            Availability::Ready
        }
    }

    fn max_concurrency(&self) -> usize {
        2
    }

    fn resolve(&self, request: ResolveRequest<'_>) -> Result<Resolved, Unresolved> {
        let settings = &request.settings.url;
        if settings.allow_hosts.is_empty() {
            return Err(Unresolved::Unavailable(Reason::NotConfigured {
                system: "url",
                setting: SETTING,
            }));
        }
        fetch(&request.reference.uri, settings, self.policy).map_err(Unresolved::Unavailable)
    }
}

/// URL の字句検査（各リダイレクトのホップでも同じ関数を通す）。
pub fn check_url(
    url: &Url,
    settings: &UrlSourceConfig,
    policy: AddressPolicy,
) -> Result<(), Reason> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Reason::UrlNotAllowed {
            rule: UrlRule::Scheme,
        });
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Reason::UrlNotAllowed {
            rule: UrlRule::Credentials,
        });
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => check_ip_literal(IpAddr::V4(ip), settings, policy),
        Some(Host::Ipv6(ip)) => check_ip_literal(IpAddr::V6(ip), settings, policy),
        Some(Host::Domain(domain)) => {
            let host = domain.to_ascii_lowercase();
            if host == "localhost"
                || host.ends_with(".localhost")
                || !host.contains('.')
                || host.ends_with('.')
                || !host_is_allowed(&host, &settings.allow_hosts)
            {
                return Err(Reason::UrlNotAllowed {
                    rule: UrlRule::Host,
                });
            }
            Ok(())
        }
        None => Err(Reason::UrlNotAllowed {
            rule: UrlRule::Host,
        }),
    }
}

fn check_ip_literal(
    ip: IpAddr,
    settings: &UrlSourceConfig,
    policy: AddressPolicy,
) -> Result<(), Reason> {
    if !policy.allows(ip) {
        return Err(Reason::UrlNotAllowed {
            rule: UrlRule::Address,
        });
    }
    // IP リテラルは `*` のときだけ許可する（FQDN の allow に IP は一致しない）。
    if !settings.allow_hosts.iter().any(|h| h == "*") {
        return Err(Reason::UrlNotAllowed {
            rule: UrlRule::Host,
        });
    }
    Ok(())
}

/// ureq の Resolver を包み、DNS 解決後の全アドレスを方針にかける。1 つでも不許可なら接続しない。
pub struct GuardedResolver<R> {
    inner: R,
    policy: AddressPolicy,
    rejected: Arc<AtomicBool>,
}

impl<R> std::fmt::Debug for GuardedResolver<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardedResolver").finish()
    }
}

impl<R: Resolver> Resolver for GuardedResolver<R> {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addrs = self.inner.resolve(uri, config, timeout)?;
        if addrs.iter().any(|addr| !self.policy.allows(addr.ip())) {
            self.rejected.store(true, Ordering::SeqCst);
            return Err(ureq::Error::HostNotFound);
        }
        Ok(addrs)
    }
}

fn agent(settings: &UrlSourceConfig, policy: AddressPolicy, rejected: Arc<AtomicBool>) -> Agent {
    let config = Config::builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_resolve(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(Duration::from_secs(settings.timeout_secs)))
        .http_status_as_error(false)
        .user_agent(format!("gaia-library/{}", env!("CARGO_PKG_VERSION")))
        .accept(ACCEPT)
        .accept_encoding("")
        .max_idle_connections(0)
        .build();
    Agent::with_parts(
        config,
        DefaultConnector::default(),
        GuardedResolver {
            inner: DefaultResolver::default(),
            policy,
            rejected,
        },
    )
}

fn fetch(uri: &str, settings: &UrlSourceConfig, policy: AddressPolicy) -> Result<Resolved, Reason> {
    let mut current = Url::parse(uri).map_err(|_| Reason::InvalidUri {
        system: "url",
        rule: "parse",
    })?;
    let rejected = Arc::new(AtomicBool::new(false));
    let agent = agent(settings, policy, rejected.clone());
    let mut hops = 0u32;
    loop {
        check_url(&current, settings, policy)?;
        current.set_fragment(None);
        rejected.store(false, Ordering::SeqCst);
        let mut response = match agent.get(current.as_str()).call() {
            Ok(response) => response,
            Err(error) => return Err(map_transport_error(error, settings, &rejected)),
        };
        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            hops += 1;
            if hops > settings.max_redirects {
                return Err(Reason::UrlNotAllowed {
                    rule: UrlRule::Redirects,
                });
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(Reason::UpstreamStatus { status })?;
            current = current.join(location).map_err(|_| Reason::UrlNotAllowed {
                rule: UrlRule::Host,
            })?;
            continue;
        }
        if status != 200 {
            return Err(Reason::UpstreamStatus { status });
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let media = accepted_media_type(content_type.as_deref())?;
        if let Some(length) = response
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            && length > settings.max_bytes
        {
            return Err(Reason::TooLarge);
        }
        let mut notes = Vec::new();
        let mut bytes = Vec::new();
        let read = response
            .body_mut()
            .with_config()
            .limit(settings.max_bytes + 1)
            .reader()
            .take(settings.max_bytes + 1)
            .read_to_end(&mut bytes);
        if let Err(error) = read {
            tracing::warn!(kind = ?error.kind(), "url resolver: reading body failed");
            return Err(if error.kind() == std::io::ErrorKind::TimedOut {
                Reason::TimedOut {
                    secs: settings.timeout_secs,
                }
            } else {
                Reason::ReadFailed
            });
        }
        if bytes.len() as u64 > settings.max_bytes {
            bytes.truncate(settings.max_bytes as usize);
            notes.push(Note::BodyTruncated {
                bytes: settings.max_bytes,
            });
        }
        let content = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => {
                notes.push(Note::NonUtf8Replaced);
                String::from_utf8_lossy(error.as_bytes()).into_owned()
            }
        };
        if media.is_html {
            notes.push(Note::HtmlAsIs);
        }
        return Ok(Resolved { content, notes });
    }
}

struct MediaType {
    is_html: bool,
}

/// media type が text 系で、charset が無いか utf-8 のときだけ受け入れる（変換は入れない）。
fn accepted_media_type(content_type: Option<&str>) -> Result<MediaType, Reason> {
    let raw = content_type.ok_or(Reason::UnsupportedContentType)?;
    let mut parts = raw.split(';');
    let media = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let accepted = media.starts_with("text/")
        || media == "application/json"
        || media == "application/xml"
        || media == "application/xhtml+xml"
        || (media.starts_with("application/")
            && (media.ends_with("+json") || media.ends_with("+xml")));
    if !accepted {
        return Err(Reason::UnsupportedContentType);
    }
    for param in parts {
        if let Some((key, value)) = param.split_once('=')
            && key.trim().eq_ignore_ascii_case("charset")
        {
            let charset = value.trim().trim_matches('"').to_ascii_lowercase();
            if charset != "utf-8" && charset != "utf8" {
                return Err(Reason::UnsupportedContentType);
            }
        }
    }
    Ok(MediaType {
        is_html: media == "text/html" || media == "application/xhtml+xml",
    })
}

fn map_transport_error(
    error: ureq::Error,
    settings: &UrlSourceConfig,
    rejected: &AtomicBool,
) -> Reason {
    if rejected.load(Ordering::SeqCst) {
        return Reason::UrlNotAllowed {
            rule: UrlRule::Address,
        };
    }
    match error {
        ureq::Error::Timeout(_) => Reason::TimedOut {
            secs: settings.timeout_secs,
        },
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => Reason::TimedOut {
            secs: settings.timeout_secs,
        },
        other => {
            tracing::warn!(error = %other, "url resolver: request failed");
            Reason::ConnectionFailed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::{SocketAddr, TcpListener},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
    };

    fn settings(hosts: &[&str]) -> UrlSourceConfig {
        UrlSourceConfig {
            allow_hosts: hosts.iter().map(|h| h.to_string()).collect(),
            timeout_secs: 5,
            max_bytes: 64,
            max_redirects: 3,
        }
    }

    fn check(url: &str, hosts: &[&str]) -> Result<(), Reason> {
        let parsed = Url::parse(url).map_err(|_| Reason::InvalidUri {
            system: "url",
            rule: "parse",
        })?;
        check_url(&parsed, &settings(hosts), AddressPolicy::PublicOnly)
    }

    #[test]
    fn check_url_table() {
        assert_eq!(check("https://example.com/a.md", &["example.com"]), Ok(()));
        assert_eq!(check("http://docs.example.com/x", &["example.com"]), Ok(()));
        for url in [
            "ftp://example.com/",
            "file:///etc/passwd",
            "javascript:alert(1)",
        ] {
            assert!(
                matches!(
                    check(url, &["*"]),
                    Err(Reason::UrlNotAllowed {
                        rule: UrlRule::Scheme
                    }) | Err(Reason::InvalidUri { .. })
                ),
                "{url}"
            );
        }
        assert_eq!(
            check("http://user:pw@example.com/", &["*"]),
            Err(Reason::UrlNotAllowed {
                rule: UrlRule::Credentials
            })
        );
        assert_eq!(
            check("http://user@example.com/", &["*"]),
            Err(Reason::UrlNotAllowed {
                rule: UrlRule::Credentials
            })
        );
        for url in [
            "http://localhost/",
            "http://foo.localhost/",
            "http://intranet/",
            "http://example.com./",
        ] {
            assert_eq!(
                check(url, &["*"]),
                Err(Reason::UrlNotAllowed {
                    rule: UrlRule::Host
                }),
                "{url}"
            );
        }
        for url in [
            "http://127.0.0.1/",
            "http://127.1/",
            "http://2130706433/",
            "http://0x7f.1/",
            "http://0177.0.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://169.254.169.254/latest/meta-data",
            "http://10.0.0.1/",
        ] {
            assert_eq!(
                check(url, &["*"]),
                Err(Reason::UrlNotAllowed {
                    rule: UrlRule::Address
                }),
                "{url}"
            );
        }
        // 公開 IP リテラルは `*` のときだけ
        assert_eq!(check("http://93.184.216.34/", &["*"]), Ok(()));
        assert_eq!(
            check("http://93.184.216.34/", &["example.com"]),
            Err(Reason::UrlNotAllowed {
                rule: UrlRule::Host
            })
        );
        assert_eq!(
            check("https://other.org/", &["example.com"]),
            Err(Reason::UrlNotAllowed {
                rule: UrlRule::Host
            })
        );
        assert_eq!(
            check("https://notexample.com/", &["example.com"]),
            Err(Reason::UrlNotAllowed {
                rule: UrlRule::Host
            })
        );
        assert!(matches!(
            check("not a url", &["*"]),
            Err(Reason::InvalidUri {
                system: "url",
                rule: "parse"
            })
        ));
    }

    #[derive(Debug)]
    struct FixedResolver(Vec<SocketAddr>);

    impl Resolver for FixedResolver {
        fn resolve(
            &self,
            _uri: &Uri,
            _config: &Config,
            _timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let mut out = self.empty();
            for addr in &self.0 {
                out.push(*addr);
            }
            Ok(out)
        }
    }

    #[test]
    fn guarded_resolver_rejects_when_any_address_is_private() {
        let uri: Uri = "http://example.com/".parse().unwrap();
        let timeout = NextTimeout {
            after: ureq::unversioned::transport::time::Duration::NotHappening,
            reason: ureq::Timeout::Global,
        };
        let public = FixedResolver(vec!["93.184.216.34:80".parse().unwrap()]);
        let rejected = Arc::new(AtomicBool::new(false));
        let guarded = GuardedResolver {
            inner: public,
            policy: AddressPolicy::PublicOnly,
            rejected: rejected.clone(),
        };
        assert!(guarded.resolve(&uri, &Config::default(), timeout).is_ok());
        assert!(!rejected.load(Ordering::SeqCst));
        let mixed = FixedResolver(vec![
            "93.184.216.34:80".parse().unwrap(),
            "10.0.0.5:80".parse().unwrap(),
        ]);
        let guarded = GuardedResolver {
            inner: mixed,
            policy: AddressPolicy::PublicOnly,
            rejected: rejected.clone(),
        };
        assert!(matches!(
            guarded.resolve(&uri, &Config::default(), timeout),
            Err(ureq::Error::HostNotFound)
        ));
        assert!(rejected.load(Ordering::SeqCst));
    }

    #[test]
    fn media_types() {
        assert!(accepted_media_type(Some("text/plain")).is_ok());
        assert!(accepted_media_type(Some("text/markdown; charset=utf-8")).is_ok());
        assert!(accepted_media_type(Some("application/json")).is_ok());
        assert!(accepted_media_type(Some("application/ld+json")).is_ok());
        assert!(accepted_media_type(Some("application/atom+xml")).is_ok());
        assert!(accepted_media_type(Some("text/html")).unwrap().is_html);
        assert!(!accepted_media_type(Some("text/plain")).unwrap().is_html);
        for ct in [
            None,
            Some("image/png"),
            Some("application/octet-stream"),
            Some("application/pdf"),
            Some("text/plain; charset=shift_jis"),
        ] {
            assert_eq!(
                accepted_media_type(ct).err(),
                Some(Reason::UnsupportedContentType),
                "{ct:?}"
            );
        }
    }

    /// 固定応答サーバー。リクエスト行とヘッダを記録し、パスごとの応答を返す。
    struct Server {
        addr: SocketAddr,
        requests: Arc<Mutex<Vec<String>>>,
        accepted: Arc<AtomicUsize>,
    }

    fn serve(routes: impl Fn(&str, SocketAddr) -> Vec<u8> + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (requests_c, accepted_c) = (requests.clone(), accepted.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                accepted_c.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                requests_c.lock().unwrap().push(head.clone());
                let path = head
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let response = routes(&path, addr);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        Server {
            addr,
            requests,
            accepted,
        }
    }

    fn http(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut out =
            format!("HTTP/1.1 {status}\r\n{headers}Connection: close\r\n\r\n").into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn resolve_with(server: &Server, path: &str, s: &UrlSourceConfig) -> Result<Resolved, Reason> {
        fetch(
            &format!("http://127.0.0.1:{}{}", server.addr.port(), path),
            s,
            AddressPolicy::AllowLoopback,
        )
    }

    fn loopback_settings() -> UrlSourceConfig {
        UrlSourceConfig {
            allow_hosts: vec!["*".into()],
            timeout_secs: 1,
            max_bytes: 64,
            max_redirects: 3,
        }
    }

    #[test]
    fn real_http_paths_over_loopback() {
        let server = serve(|path, addr| match path {
            "/ok" => http(
                "200 OK",
                "Content-Type: text/plain; charset=utf-8\r\nContent-Length: 5\r\n",
                b"hello",
            ),
            "/html" => http("200 OK", "Content-Type: text/html\r\n", b"<p>hi</p>"),
            "/png" => http("200 OK", "Content-Type: image/png\r\n", b"\x89PNG"),
            "/sjis" => http(
                "200 OK",
                "Content-Type: text/plain; charset=shift_jis\r\n",
                b"x",
            ),
            "/nocontenttype" => http("200 OK", "", b"x"),
            "/missing" => http("404 Not Found", "Content-Type: text/plain\r\n", b"nope"),
            "/toolarge" => http(
                "200 OK",
                "Content-Type: text/plain\r\nContent-Length: 65\r\n",
                &[b'a'; 65],
            ),
            "/unknownlength" => http("200 OK", "Content-Type: text/plain\r\n", &[b'b'; 100]),
            "/redirect" => http(
                "302 Found",
                &format!(
                    "Location: http://127.0.0.1:{}/ok\r\nSet-Cookie: a=b\r\n",
                    addr.port()
                ),
                b"",
            ),
            "/loop" => http(
                "302 Found",
                &format!("Location: http://127.0.0.1:{}/loop\r\n", addr.port()),
                b"",
            ),
            "/metadata" => http(
                "302 Found",
                "Location: http://169.254.169.254/latest\r\n",
                b"",
            ),
            "/redirect-nolocation" => http("302 Found", "", b""),
            "/badutf8" => http("200 OK", "Content-Type: text/plain\r\n", b"ok\xff\xfe"),
            "/slow" => {
                thread::sleep(Duration::from_millis(2500));
                http("200 OK", "Content-Type: text/plain\r\n", b"late")
            }
            _ => http("500 Internal Server Error", "", b""),
        });
        let s = loopback_settings();
        let ok = resolve_with(&server, "/ok", &s).unwrap();
        assert_eq!(ok.content, "hello");
        assert!(ok.notes.is_empty());
        let html = resolve_with(&server, "/html", &s).unwrap();
        assert_eq!(html.content, "<p>hi</p>");
        assert_eq!(html.notes, vec![Note::HtmlAsIs]);
        assert_eq!(
            resolve_with(&server, "/png", &s).unwrap_err(),
            Reason::UnsupportedContentType
        );
        assert_eq!(
            resolve_with(&server, "/sjis", &s).unwrap_err(),
            Reason::UnsupportedContentType
        );
        assert_eq!(
            resolve_with(&server, "/nocontenttype", &s).unwrap_err(),
            Reason::UnsupportedContentType
        );
        assert_eq!(
            resolve_with(&server, "/missing", &s).unwrap_err(),
            Reason::UpstreamStatus { status: 404 }
        );
        assert_eq!(
            resolve_with(&server, "/toolarge", &s).unwrap_err(),
            Reason::TooLarge
        );
        let truncated = resolve_with(&server, "/unknownlength", &s).unwrap();
        assert_eq!(truncated.content.len(), 64);
        assert_eq!(truncated.notes, vec![Note::BodyTruncated { bytes: 64 }]);
        let redirected = resolve_with(&server, "/redirect", &s).unwrap();
        assert_eq!(redirected.content, "hello");
        assert_eq!(
            resolve_with(&server, "/loop", &s).unwrap_err(),
            Reason::UrlNotAllowed {
                rule: UrlRule::Redirects
            }
        );
        assert_eq!(
            resolve_with(&server, "/metadata", &s).unwrap_err(),
            Reason::UrlNotAllowed {
                rule: UrlRule::Address
            }
        );
        assert_eq!(
            resolve_with(&server, "/redirect-nolocation", &s).unwrap_err(),
            Reason::UpstreamStatus { status: 302 }
        );
        let lossy = resolve_with(&server, "/badutf8", &s).unwrap();
        assert!(lossy.content.starts_with("ok"));
        assert_eq!(lossy.notes, vec![Note::NonUtf8Replaced]);
        assert_eq!(
            resolve_with(&server, "/slow", &s).unwrap_err(),
            Reason::TimedOut { secs: 1 }
        );
        // 送ったヘッダの検査: Accept-Encoding / Cookie / Authorization は無く、Cookie は次ホップに送らない
        let requests = server.requests.lock().unwrap();
        assert!(requests.len() >= 2);
        for head in requests.iter() {
            let lower = head.to_ascii_lowercase();
            assert!(!lower.contains("accept-encoding"), "{head}");
            assert!(!lower.contains("cookie:"), "{head}");
            assert!(!lower.contains("authorization"), "{head}");
            assert!(lower.contains("user-agent: gaia-library/"), "{head}");
            assert!(lower.contains("accept: text/markdown"), "{head}");
        }
    }

    #[test]
    fn production_policy_rejects_loopback_before_connecting() {
        let server = serve(|_, _| http("200 OK", "Content-Type: text/plain\r\n", b"never"));
        let mut s = loopback_settings();
        s.allow_hosts = vec!["*".into()];
        let result = fetch(
            &format!("http://127.0.0.1:{}/", server.addr.port()),
            &s,
            AddressPolicy::PublicOnly,
        );
        assert_eq!(
            result.unwrap_err(),
            Reason::UrlNotAllowed {
                rule: UrlRule::Address
            }
        );
        // ホスト名経由でも解決後に拒否され、接続されない
        let result = fetch(
            &format!("http://localhost.example.com:{}/", server.addr.port()),
            &s,
            AddressPolicy::PublicOnly,
        );
        assert!(matches!(
            result,
            Err(Reason::UrlNotAllowed {
                rule: UrlRule::Address
            }) | Err(Reason::ConnectionFailed)
        ));
        thread::sleep(Duration::from_millis(50));
        assert_eq!(server.accepted.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn resolver_availability_and_not_configured() {
        let resolver = UrlResolver::public_only();
        assert_eq!(resolver.system(), "url");
        assert_eq!(resolver.max_concurrency(), 2);
        let mut settings = SourcesConfig::default();
        assert_eq!(
            resolver.availability(&settings),
            Availability::Unconfigured { setting: SETTING }
        );
        settings.url.allow_hosts = vec!["example.com".into()];
        assert_eq!(resolver.availability(&settings), Availability::Ready);
    }
}
