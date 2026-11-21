//! The pairing link encoded in the Hub's QR code.
//!
//! ```text
//! devti://pair?v=1&id=<device id>&fp=<b64url spki sha256>&s=<b32 secret>
//!             &p=<udp port>&a=<ip>,<ip>&n=<name>&k=<kind>
//! ```
//!
//! The link carries everything a phone needs, even when mDNS is blocked: where the Hub
//! is (`a`, `p`), who it is (`fp`, pinned during the TLS handshake) and the one-time
//! secret (`s`) that feeds the PAKE. It is only valid while the pairing window is open.

use std::net::{IpAddr, SocketAddr};

use data_encoding::BASE32_NOPAD;
use url::Url;

use crate::error::{Error, Result};
use crate::model::{DeviceId, DeviceKind, Fingerprint, sanitize_name};

/// Length of the QR pairing secret in bytes (128 bits).
pub const QR_SECRET_LEN: usize = 16;
/// Longest link we accept — QR payloads are small, so anything larger is hostile.
const MAX_URI_LEN: usize = 1024;
/// Most addresses a link may carry.
const MAX_ADDRS: usize = 8;

/// Decoded pairing link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingUri {
    pub hub_id: DeviceId,
    pub fingerprint: Fingerprint,
    pub secret: [u8; QR_SECRET_LEN],
    pub port: u16,
    pub addrs: Vec<IpAddr>,
    pub name: String,
    pub kind: DeviceKind,
}

impl PairingUri {
    /// Renders the link (what gets encoded into the QR code).
    pub fn to_uri(&self) -> String {
        let mut url = Url::parse("devti://pair").expect("static url is valid");
        let addrs = self.addrs.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(",");
        url.query_pairs_mut()
            .append_pair("v", "1")
            .append_pair("id", &self.hub_id.to_string())
            .append_pair("fp", &self.fingerprint.to_b64())
            .append_pair("s", &BASE32_NOPAD.encode(&self.secret))
            .append_pair("p", &self.port.to_string())
            .append_pair("a", &addrs)
            .append_pair("n", &self.name)
            .append_pair("k", self.kind.as_str());
        url.to_string()
    }

    /// Socket addresses to try, in the order given by the Hub.
    pub fn socket_addrs(&self) -> Vec<SocketAddr> {
        self.addrs.iter().map(|ip| SocketAddr::new(*ip, self.port)).collect()
    }

    /// Strictly parses a link. Unknown query parameters are ignored (forward compatible),
    /// everything else must be present and well-formed.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        if input.len() > MAX_URI_LEN {
            return Err(bad("link is too long"));
        }
        let url = Url::parse(input).map_err(|_| bad("not a link"))?;
        if url.scheme() != "devti" || url.host_str() != Some("pair") {
            return Err(bad("not a Devti pairing link"));
        }

        let mut v = None;
        let mut id = None;
        let mut fp = None;
        let mut secret = None;
        let mut port = None;
        let mut addrs = None;
        let mut name = None;
        let mut kind = DeviceKind::Desktop;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "v" => v = Some(value.into_owned()),
                "id" => id = Some(value.parse::<DeviceId>().map_err(|_| bad("bad device id"))?),
                "fp" => fp = Some(Fingerprint::from_b64(&value).map_err(|_| bad("bad fingerprint"))?),
                "s" => {
                    let bytes = BASE32_NOPAD.decode(value.as_bytes()).map_err(|_| bad("bad secret"))?;
                    let arr: [u8; QR_SECRET_LEN] = bytes.try_into().map_err(|_| bad("bad secret length"))?;
                    secret = Some(arr);
                }
                "p" => {
                    let p: u16 = value.parse().map_err(|_| bad("bad port"))?;
                    if p == 0 {
                        return Err(bad("bad port"));
                    }
                    port = Some(p);
                }
                "a" => {
                    let list = value
                        .split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.parse::<IpAddr>().map_err(|_| bad("bad address")))
                        .collect::<Result<Vec<_>>>()?;
                    if list.len() > MAX_ADDRS {
                        return Err(bad("too many addresses"));
                    }
                    addrs = Some(list);
                }
                "n" => name = Some(sanitize_name(&value)),
                "k" => kind = DeviceKind::parse(&value),
                _ => {}
            }
        }

        if v.as_deref() != Some("1") {
            return Err(bad("unsupported link version — update Devti on this device"));
        }
        let hub_id = id.ok_or_else(|| bad("missing device id"))?;
        let fingerprint = fp.ok_or_else(|| bad("missing fingerprint"))?;
        if fingerprint.device_id() != hub_id {
            return Err(bad("device id doesn't match fingerprint"));
        }
        let addrs = addrs.unwrap_or_default();
        if addrs.is_empty() {
            return Err(bad("missing address"));
        }
        Ok(Self {
            hub_id,
            fingerprint,
            secret: secret.ok_or_else(|| bad("missing secret"))?,
            port: port.ok_or_else(|| bad("missing port"))?,
            addrs,
            name: name.unwrap_or_else(|| sanitize_name("")),
            kind,
        })
    }
}

fn bad(msg: &str) -> Error {
    Error::InvalidPairingUri(msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PairingUri {
        let fp = Fingerprint([7; 32]);
        PairingUri {
            hub_id: fp.device_id(),
            fingerprint: fp,
            secret: [9; QR_SECRET_LEN],
            port: 47810,
            addrs: vec!["192.168.1.20".parse().unwrap(), "fe80::1".parse().unwrap()],
            name: "Hussnain's Laptop & Co".into(),
            kind: DeviceKind::Laptop,
        }
    }

    #[test]
    fn roundtrip() {
        let uri = sample();
        let text = uri.to_uri();
        assert!(text.starts_with("devti://pair?v=1&"));
        let back = PairingUri::parse(&text).unwrap();
        assert_eq!(back, uri);
        assert_eq!(back.socket_addrs()[0].to_string(), "192.168.1.20:47810");
    }

    #[test]
    fn unknown_params_are_ignored() {
        let text = format!("{}&future=yes", sample().to_uri());
        assert!(PairingUri::parse(&text).is_ok());
    }

    #[test]
    fn rejects_bad_links() {
        let good = sample().to_uri();
        let cases = [
            "https://example.com".to_string(),
            "devti://other?v=1".to_string(),
            "not a url".to_string(),
            good.replace("v=1", "v=2"),
            good.replace("p=47810", "p=0"),
            good.replace("p=47810", "p=99999"),
            good.replace("a=192.168.1.20", "a=999.1.1.1"),
            "x".repeat(2000),
        ];
        for c in &cases {
            assert!(PairingUri::parse(c).is_err(), "should reject {c}");
        }
    }

    #[test]
    fn rejects_missing_fields_and_mismatched_id() {
        let u = sample();
        let fp = u.fingerprint.to_b64();
        let base =
            format!("devti://pair?v=1&id={}&fp={fp}&s={}&p=1&a=10.0.0.1", u.hub_id, BASE32_NOPAD.encode(&u.secret));
        assert!(PairingUri::parse(&base).is_ok());
        assert!(PairingUri::parse(&base.replace(&format!("&fp={fp}"), "")).is_err());
        assert!(PairingUri::parse(&base.replace("&p=1", "")).is_err());
        assert!(PairingUri::parse(&base.replace("&a=10.0.0.1", "")).is_err());
        assert!(PairingUri::parse(&base.replace("&a=10.0.0.1", "&a=")).is_err());
        assert!(PairingUri::parse(&base.replace("v=1&", "")).is_err());
        let other_id = DeviceId::from_bytes([1; 10]);
        assert!(PairingUri::parse(&base.replace(&u.hub_id.to_string(), &other_id.to_string())).is_err());
        let short_secret = base.replace(&BASE32_NOPAD.encode(&u.secret), "AAAA");
        assert!(PairingUri::parse(&short_secret).is_err());
        let many = (0..9).map(|i| format!("10.0.0.{i}")).collect::<Vec<_>>().join(",");
        assert!(PairingUri::parse(&base.replace("a=10.0.0.1", &format!("a={many}"))).is_err());
        let no_secret = base.replace(&format!("&s={}", BASE32_NOPAD.encode(&u.secret)), "");
        assert!(PairingUri::parse(&no_secret).is_err());
        let no_id = base.replace(&format!("&id={}", u.hub_id), "");
        assert!(PairingUri::parse(&no_id).is_err());
    }

    #[test]
    fn hostile_names_are_sanitized() {
        let mut u = sample();
        u.name = "\u{202e}evil\u{0000}".into();
        let back = PairingUri::parse(&u.to_uri()).unwrap();
        assert_eq!(back.name, "evil");
    }
}
