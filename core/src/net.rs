//! Small networking helpers.

use std::net::IpAddr;

/// LAN addresses of this machine, best first: private IPv4, then other IPv4, then
/// global/ULA IPv6. Used in the QR pairing link so phones can connect even when mDNS
/// is blocked.
pub fn local_addresses() -> Vec<IpAddr> {
    let mut addrs: Vec<IpAddr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .map(|i| i.ip())
        .filter(|ip| match ip {
            IpAddr::V4(v4) => !v4.is_link_local() && !v4.is_unspecified(),
            IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80 && !v6.is_unspecified(),
        })
        .collect();
    addrs.sort_by_key(rank);
    addrs.dedup();
    addrs.truncate(6);
    addrs
}

fn rank(ip: &IpAddr) -> (u8, IpAddr) {
    let r = match ip {
        IpAddr::V4(v4) if v4.is_private() => 0,
        IpAddr::V4(_) => 1,
        IpAddr::V6(_) => 2,
    };
    (r, *ip)
}

/// Current time in Unix milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_prefers_private_ipv4() {
        let mut v: Vec<IpAddr> =
            vec!["2001:db8::1".parse().unwrap(), "8.8.8.8".parse().unwrap(), "192.168.1.2".parse().unwrap()];
        v.sort_by_key(rank);
        assert_eq!(v[0].to_string(), "192.168.1.2");
        assert_eq!(v[2].to_string(), "2001:db8::1");
    }

    #[test]
    fn local_addresses_never_include_loopback() {
        assert!(local_addresses().iter().all(|ip| !ip.is_loopback()));
        assert!(now_ms() > 1_700_000_000_000);
    }
}
