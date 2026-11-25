//! 客户端 IP（D38），只用于登录限流，不写入数据库和日志。

use std::net::IpAddr;

use axum::http::HeaderMap;
use ipnet::IpNet;

/// 直连地址 `peer` 不在 `trusted` 里时就用它，忽略 `X-Forwarded-For`。可信时从
/// `X-Forwarded-For` 最右侧向左跳过可信地址，取第一个不可信的；全部可信时取最左边的。
/// 请求头缺失或格式错误时退回直连地址。
pub fn client_ip(peer: IpAddr, headers: &HeaderMap, trusted: &[IpNet]) -> IpAddr {
    let peer = peer.to_canonical();
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
    if !is_trusted(&peer) {
        return peer;
    }
    let mut hops = Vec::new();
    for value in headers.get_all("x-forwarded-for") {
        let Ok(value) = value.to_str() else {
            return peer;
        };
        for item in value.split(',') {
            match item.trim().parse::<IpAddr>() {
                Ok(ip) => hops.push(ip.to_canonical()),
                Err(_) => return peer,
            }
        }
    }
    hops.iter()
        .rev()
        .find(|ip| !is_trusted(ip))
        .or(hops.first())
        .copied()
        .unwrap_or(peer)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn proxies() -> Vec<IpNet> {
        vec![
            "127.0.0.1/32".parse().unwrap(),
            "10.0.0.0/8".parse().unwrap(),
        ]
    }

    fn forwarded(values: &[&'static str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for v in values {
            headers.append("x-forwarded-for", HeaderValue::from_static(v));
        }
        headers
    }

    #[test]
    fn untrusted_peer_ignores_forwarded_header() {
        let headers = forwarded(&["198.51.100.7"]);
        assert_eq!(
            client_ip(ip("203.0.113.9"), &headers, &proxies()),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn trusted_peer_takes_rightmost_untrusted_hop() {
        // 客户端自己伪造的最左项被跳过：最右侧的不可信地址才是代理看到的客户端。
        let headers = forwarded(&["1.1.1.1, 198.51.100.7", "10.0.0.5"]);
        assert_eq!(
            client_ip(ip("127.0.0.1"), &headers, &proxies()),
            ip("198.51.100.7")
        );
    }

    #[test]
    fn all_trusted_hops_take_leftmost() {
        let headers = forwarded(&["10.0.0.9, 10.0.0.5"]);
        assert_eq!(
            client_ip(ip("127.0.0.1"), &headers, &proxies()),
            ip("10.0.0.9")
        );
    }

    #[test]
    fn missing_or_malformed_header_falls_back_to_peer() {
        let peer = ip("127.0.0.1");
        assert_eq!(client_ip(peer, &HeaderMap::new(), &proxies()), peer);
        let headers = forwarded(&["198.51.100.7, not-an-ip"]);
        assert_eq!(client_ip(peer, &headers, &proxies()), peer);
    }

    #[test]
    fn ipv4_mapped_peer_is_matched_as_ipv4() {
        let headers = forwarded(&["198.51.100.7"]);
        assert_eq!(
            client_ip(ip("::ffff:127.0.0.1"), &headers, &proxies()),
            ip("198.51.100.7")
        );
    }
}
