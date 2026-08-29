//! SSRF 判定の純関数。依存無し・表テスト。`std::net::Ipv4Addr::is_private` 等には頼らず範囲を列挙する。
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// 公開アドレスなら真。ループバック・プライベート・リンクローカル・メタデータ・予約・
/// マルチキャスト・文書用・埋め込み IPv4（mapped / NAT64 / 6to4）を拒否する。
pub fn ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_public(v4),
        IpAddr::V6(v6) => ipv6_is_public(v6),
    }
}

fn in_v4_block(ip: Ipv4Addr, network: [u8; 4], prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (u32::from(ip) & mask) == (u32::from_be_bytes(network) & mask)
}

fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    const BLOCKED: &[([u8; 4], u32)] = &[
        ([0, 0, 0, 0], 8),       // this network
        ([10, 0, 0, 0], 8),      // RFC 1918
        ([100, 64, 0, 0], 10),   // CGNAT
        ([127, 0, 0, 0], 8),     // loopback
        ([169, 254, 0, 0], 16),  // link-local（169.254.169.254 を含む）
        ([172, 16, 0, 0], 12),   // RFC 1918
        ([192, 0, 0, 0], 24),    // IETF 予約
        ([192, 0, 2, 0], 24),    // TEST-NET-1
        ([192, 88, 99, 0], 24),  // 6to4 中継
        ([192, 168, 0, 0], 16),  // RFC 1918
        ([198, 18, 0, 0], 15),   // ベンチマーク
        ([198, 51, 100, 0], 24), // TEST-NET-2
        ([203, 0, 113, 0], 24),  // TEST-NET-3
        ([224, 0, 0, 0], 4),     // マルチキャスト
        ([240, 0, 0, 0], 4),     // 予約・ブロードキャスト
    ];
    !BLOCKED
        .iter()
        .any(|(network, prefix)| in_v4_block(ip, *network, *prefix))
}

fn in_v6_block(ip: Ipv6Addr, network: Ipv6Addr, prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - prefix)
    };
    (u128::from(ip) & mask) == (u128::from(network) & mask)
}

/// 埋め込み IPv4 を取り出す（mapped `::ffff:0:0/96`、NAT64 `64:ff9b::/96`、6to4 `2002::/16`）。
fn embedded_v4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ip.segments();
    let octets = ip.octets();
    if in_v6_block(ip, Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0, 0), 96)
        || in_v6_block(ip, Ipv6Addr::new(0x64, 0xff9b, 0, 0, 0, 0, 0, 0), 96)
    {
        return Some(Ipv4Addr::new(
            octets[12], octets[13], octets[14], octets[15],
        ));
    }
    if segments[0] == 0x2002 {
        return Some(Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]));
    }
    None
}

fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() {
        return false;
    }
    if let Some(v4) = embedded_v4(ip) {
        return ipv4_is_public(v4);
    }
    const BLOCKED: &[(Ipv6Addr, u32)] = &[
        (Ipv6Addr::new(0x64, 0xff9b, 1, 0, 0, 0, 0, 0), 48), // NAT64 local-use
        (Ipv6Addr::new(0x100, 0, 0, 0, 0, 0, 0, 0), 64),     // discard
        (Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 32),    // Teredo
        (Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 0), 32), // 文書用
        (Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7),     // ULA
        (Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0), 10),    // link-local
        (Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0), 10),    // site-local
        (Ipv6Addr::new(0xff00, 0, 0, 0, 0, 0, 0, 0), 8),     // マルチキャスト
    ];
    !BLOCKED
        .iter()
        .any(|(network, prefix)| in_v6_block(ip, *network, *prefix))
}

/// `allow_hosts` の判定。`*` か、完全一致または `.<h>` 接尾辞一致。入力は ASCII 小文字化して比べる。
pub fn host_is_allowed(host: &str, allow_hosts: &[String]) -> bool {
    let host = host.to_ascii_lowercase();
    allow_hosts.iter().any(|allowed| {
        if allowed == "*" {
            return true;
        }
        let allowed = allowed.to_ascii_lowercase();
        host == allowed || host.ends_with(&format!(".{allowed}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn ipv4_boundaries() {
        let rejected = [
            "0.0.0.0",
            "0.255.255.255",
            "10.0.0.0",
            "10.255.255.255",
            "100.64.0.0",
            "100.127.255.255",
            "127.0.0.1",
            "127.255.255.255",
            "169.254.0.1",
            "169.254.169.254",
            "172.16.0.0",
            "172.31.255.255",
            "192.0.0.1",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.0.1",
            "192.168.255.255",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
        ];
        for addr in rejected {
            assert!(!ip_is_public(ip(addr)), "{addr} must be rejected");
        }
        let allowed = [
            "1.0.0.1",
            "9.255.255.255",
            "11.0.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "126.255.255.255",
            "128.0.0.1",
            "169.253.255.255",
            "169.255.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.0.1.1",
            "192.0.3.1",
            "192.88.98.1",
            "192.167.255.255",
            "192.169.0.0",
            "198.17.255.255",
            "198.20.0.0",
            "198.51.99.1",
            "203.0.112.1",
            "223.255.255.255",
            "93.184.216.34",
            "8.8.8.8",
        ];
        for addr in allowed {
            assert!(ip_is_public(ip(addr)), "{addr} must be allowed");
        }
    }

    #[test]
    fn ipv6_boundaries_and_embedded_ipv4() {
        let rejected = [
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "64:ff9b::a00:1",
            "64:ff9b:1::1",
            "2002:7f00:1::",
            "2002:a00:1::",
            "fc00::1",
            "fdff::1",
            "fe80::1",
            "febf::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "100::1",
        ];
        for addr in rejected {
            assert!(!ip_is_public(ip(addr)), "{addr} must be rejected");
        }
        let allowed = [
            "::ffff:93.184.216.34",
            "64:ff9b::5db8:d822",
            "2002:5db8:d822::",
            "2606:4700::1111",
            "2001:4860:4860::8888",
            "2001:db7::1",
            "2001:1::1",
            "fbff::1",
            "fe7f::1",
            "fe00::1",
        ];
        for addr in allowed {
            assert!(ip_is_public(ip(addr)), "{addr} must be allowed");
        }
    }

    #[test]
    fn host_allow_list_matches_exact_and_suffix_only() {
        let allow = vec!["example.com".to_string(), "Docs.Example.org".to_string()];
        assert!(host_is_allowed("example.com", &allow));
        assert!(host_is_allowed("EXAMPLE.COM", &allow));
        assert!(host_is_allowed("sub.example.com", &allow));
        assert!(host_is_allowed("a.b.example.com", &allow));
        assert!(host_is_allowed("docs.example.org", &allow));
        assert!(!host_is_allowed("notexample.com", &allow));
        assert!(!host_is_allowed("example.com.evil.net", &allow));
        assert!(!host_is_allowed("example.org", &allow));
        assert!(!host_is_allowed("example.com", &[]));
        assert!(host_is_allowed("anything.invalid", &["*".to_string()]));
    }
}
