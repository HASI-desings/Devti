//! The device model: who a device is, what it can do, and what it is allowed to do.
//!
//! Every type here is serialisable twice over: as CBOR on the wire (protocol) and as
//! JSON towards the UI layers (Tauri, UniFFI). Enums use `#[serde(other)]` fallbacks so
//! a newer peer announcing a kind or capability we don't know yet never breaks parsing.

use std::fmt;
use std::str::FromStr;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, Result};

/// Length in bytes of a [`DeviceId`].
pub const DEVICE_ID_LEN: usize = 10;
/// Maximum length (in characters) of a user-visible device name.
pub const MAX_NAME_CHARS: usize = 48;

/// Stable identifier of a device, derived from its public key.
///
/// It is the first 10 bytes of SHA-256(SubjectPublicKeyInfo), rendered as 16 Base32
/// characters. Because it is derived from the key, a device can't claim another
/// device's ID without also holding its private key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId([u8; DEVICE_ID_LEN]);

impl DeviceId {
    /// Builds an ID from raw bytes.
    pub const fn from_bytes(bytes: [u8; DEVICE_ID_LEN]) -> Self {
        Self(bytes)
    }

    /// Raw bytes of the ID.
    pub fn as_bytes(&self) -> &[u8; DEVICE_ID_LEN] {
        &self.0
    }

    /// Short human form, e.g. `K7Q2-M9XA`, used in the UI next to names.
    pub fn short(&self) -> String {
        let s = self.to_string();
        format!("{}-{}", &s[0..4], &s[4..8])
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&BASE32_NOPAD.encode(&self.0))
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({self})")
    }
}

impl FromStr for DeviceId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let cleaned: String = s.chars().filter(|c| *c != '-').collect::<String>().to_ascii_uppercase();
        let bytes = BASE32_NOPAD
            .decode(cleaned.as_bytes())
            .map_err(|_| Error::InvalidInput(format!("'{s}' is not a device id")))?;
        let arr: [u8; DEVICE_ID_LEN] =
            bytes.try_into().map_err(|_| Error::InvalidInput(format!("'{s}' has the wrong length for a device id")))?;
        Ok(Self(arr))
    }
}

impl Serialize for DeviceId {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for DeviceId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// SHA-256 of a device's SubjectPublicKeyInfo. The value that gets pinned.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// Derived device ID (first 10 bytes).
    pub fn device_id(&self) -> DeviceId {
        let mut id = [0u8; DEVICE_ID_LEN];
        id.copy_from_slice(&self.0[..DEVICE_ID_LEN]);
        DeviceId(id)
    }

    /// Lower-case hex, grouped by 4 for human comparison.
    pub fn grouped_hex(&self) -> String {
        let hex = data_encoding::HEXLOWER.encode(&self.0);
        hex.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect::<Vec<_>>().join(" ")
    }

    /// URL-safe Base64 without padding (43 chars) — used in pairing links.
    pub fn to_b64(&self) -> String {
        data_encoding::BASE64URL_NOPAD.encode(&self.0)
    }

    /// Parses the [`Fingerprint::to_b64`] form.
    pub fn from_b64(s: &str) -> Result<Self> {
        let bytes = data_encoding::BASE64URL_NOPAD
            .decode(s.as_bytes())
            .map_err(|_| Error::InvalidInput("fingerprint is not base64url".into()))?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| Error::InvalidInput("fingerprint must be 32 bytes".into()))?;
        Ok(Self(arr))
    }
}

impl Serialize for Fingerprint {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_b64())
    }
}

impl<'de> Deserialize<'de> for Fingerprint {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Fingerprint::from_b64(&s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", &self.to_b64()[..12])
    }
}

/// The physical form of a device — drives icons, default roles and layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Desktop,
    Laptop,
    Phone,
    Tablet,
    Accessory,
    #[serde(other)]
    Unknown,
}

impl DeviceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeviceKind::Desktop => "desktop",
            DeviceKind::Laptop => "laptop",
            DeviceKind::Phone => "phone",
            DeviceKind::Tablet => "tablet",
            DeviceKind::Accessory => "accessory",
            DeviceKind::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "desktop" => DeviceKind::Desktop,
            "laptop" => DeviceKind::Laptop,
            "phone" => DeviceKind::Phone,
            "tablet" => DeviceKind::Tablet,
            "accessory" => DeviceKind::Accessory,
            _ => DeviceKind::Unknown,
        }
    }
}

/// Operating system family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Ipados,
    #[serde(other)]
    Other,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::Macos => "macos",
            Platform::Android => "android",
            Platform::Ios => "ios",
            Platform::Ipados => "ipados",
            Platform::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "windows" => Platform::Windows,
            "linux" => Platform::Linux,
            "macos" => Platform::Macos,
            "android" => Platform::Android,
            "ios" => Platform::Ios,
            "ipados" => Platform::Ipados,
            _ => Platform::Other,
        }
    }

    /// The platform this binary was compiled for (mobile apps override it).
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "android") {
            Platform::Android
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "ios") {
            Platform::Ios
        } else {
            Platform::Other
        }
    }
}

/// A feature a device can offer to the ecosystem. Announced in `Hello`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Battery,
    Clipboard,
    FileReceive,
    FileSend,
    Notifications,
    ScreenSource,
    ScreenSink,
    InputSource,
    InputSink,
    Sms,
    Calls,
    FindMe,
    BluetoothHub,
    Commands,
    #[serde(other)]
    Unknown,
}

/// Public description of a device, exchanged at pairing time and in `Hello`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: String,
    pub kind: DeviceKind,
    pub platform: Platform,
    /// Free-form OS version, e.g. "Windows 11 23H2" or "Android 15".
    #[serde(default)]
    pub os_version: String,
    /// Hardware model if known, e.g. "Pixel 8".
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

impl DeviceInfo {
    /// Normalises user/peer-controlled strings so nothing hostile reaches the UI or DB.
    pub fn sanitized(mut self) -> Self {
        self.name = sanitize_name(&self.name);
        self.os_version = truncate_clean(&self.os_version, 64);
        self.model = self.model.map(|m| truncate_clean(&m, 64)).filter(|m| !m.is_empty());
        self.app_version = truncate_clean(&self.app_version, 32);
        self.capabilities.retain(|c| *c != Capability::Unknown);
        self.capabilities.sort_by_key(|c| *c as u8);
        self.capabilities.dedup();
        self.capabilities.truncate(64);
        self
    }
}

/// Cleans a device name: trims, strips control characters, caps the length and
/// falls back to "Unnamed device" when nothing printable is left.
pub fn sanitize_name(name: &str) -> String {
    let cleaned = truncate_clean(name, MAX_NAME_CHARS);
    if cleaned.is_empty() { "Unnamed device".to_string() } else { cleaned }
}

fn truncate_clean(s: &str, max_chars: usize) -> String {
    let filtered: String = s
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_invisible_format_char(*c))
        .collect();
    let collapsed = filtered.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max_chars).collect::<String>().trim().to_string()
}

/// Zero-width and bidi-override characters can make one name look like another
/// ("spoofing"), so they are removed from anything a peer sends us.
fn is_invisible_format_char(c: char) -> bool {
    matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2069 | 0xFEFF)
}

/// Battery state of a device or accessory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatteryState {
    /// 0–100.
    pub percent: u8,
    pub charging: bool,
}

impl BatteryState {
    pub fn new(percent: u8, charging: bool) -> Self {
        Self { percent: percent.min(100), charging }
    }
}

/// Live state that changes while a device is connected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceStatus {
    #[serde(default)]
    pub battery: Option<BatteryState>,
}

impl DeviceStatus {
    pub fn sanitized(mut self) -> Self {
        self.battery = self.battery.map(|b| BatteryState::new(b.percent, b.charging));
        self
    }
}

/// Role of a paired device. Roles are presets; each permission can then be tuned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustRole {
    /// Everything allowed — the user's own devices.
    Trusted,
    /// Only low-risk features (files, clipboard, notifications) — shared or borrowed devices.
    Restricted,
}

impl TrustRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrustRole::Trusted => "trusted",
            TrustRole::Restricted => "restricted",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "restricted" { TrustRole::Restricted } else { TrustRole::Trusted }
    }
}

/// What a paired device may do **to this device**. Checked locally before acting
/// on any request — a peer can never grant itself permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    pub screen_view: bool,
    pub control: bool,
    pub files: bool,
    pub clipboard: bool,
    pub notifications: bool,
    pub commands: bool,
    pub bluetooth: bool,
}

impl Permissions {
    /// Default permission set for a role.
    pub fn for_role(role: TrustRole) -> Self {
        match role {
            TrustRole::Trusted => Self {
                screen_view: true,
                control: true,
                files: true,
                clipboard: true,
                notifications: true,
                commands: true,
                bluetooth: true,
            },
            TrustRole::Restricted => Self {
                screen_view: false,
                control: false,
                files: true,
                clipboard: true,
                notifications: true,
                commands: false,
                bluetooth: false,
            },
        }
    }
}

/// How a peer is currently reachable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// Direct QUIC over the local network.
    Lan,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_roundtrips_through_text() {
        let id = DeviceId::from_bytes([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let text = id.to_string();
        assert_eq!(text.len(), 16);
        assert_eq!(text.parse::<DeviceId>().unwrap(), id);
        // Grouped / lower-case input is accepted too.
        let grouped = format!("{}-{}", &text[..8], &text[8..]).to_lowercase();
        assert_eq!(grouped.parse::<DeviceId>().unwrap(), id);
        assert_eq!(id.short().len(), 9);
        assert!(format!("{id:?}").contains(&text));
    }

    #[test]
    fn device_id_rejects_garbage() {
        assert!("not an id".parse::<DeviceId>().is_err());
        assert!("AAAA".parse::<DeviceId>().is_err());
    }

    #[test]
    fn device_id_serde_is_a_string() {
        let id = DeviceId::from_bytes([9; 10]);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{id}\""));
        let back: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
        assert!(serde_json::from_str::<DeviceId>("\"zz\"").is_err());
    }

    #[test]
    fn fingerprint_forms() {
        let fp = Fingerprint([0xab; 32]);
        assert_eq!(fp.device_id().as_bytes(), &[0xab; 10]);
        assert_eq!(Fingerprint::from_b64(&fp.to_b64()).unwrap(), fp);
        assert!(fp.grouped_hex().starts_with("abab abab"));
        assert!(Fingerprint::from_b64("short").is_err());
        assert!(Fingerprint::from_b64("!!!").is_err());
        assert!(format!("{fp:?}").starts_with("Fingerprint("));
        let json = serde_json::to_string(&fp).unwrap();
        assert_eq!(serde_json::from_str::<Fingerprint>(&json).unwrap(), fp);
        assert!(serde_json::from_str::<Fingerprint>("\"x\"").is_err());
    }

    #[test]
    fn unknown_enum_values_fall_back() {
        let k: DeviceKind = serde_json::from_str("\"hologram\"").unwrap();
        assert_eq!(k, DeviceKind::Unknown);
        let p: Platform = serde_json::from_str("\"plan9\"").unwrap();
        assert_eq!(p, Platform::Other);
        let c: Capability = serde_json::from_str("\"teleport\"").unwrap();
        assert_eq!(c, Capability::Unknown);
    }

    #[test]
    fn kind_and_platform_string_forms_roundtrip() {
        for k in [
            DeviceKind::Desktop,
            DeviceKind::Laptop,
            DeviceKind::Phone,
            DeviceKind::Tablet,
            DeviceKind::Accessory,
            DeviceKind::Unknown,
        ] {
            assert_eq!(DeviceKind::parse(k.as_str()), k);
        }
        for p in [
            Platform::Windows,
            Platform::Linux,
            Platform::Macos,
            Platform::Android,
            Platform::Ios,
            Platform::Ipados,
            Platform::Other,
        ] {
            assert_eq!(Platform::parse(p.as_str()), p);
        }
        assert_ne!(Platform::current(), Platform::Ios);
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("  My\u{0007} Laptop \n "), "My Laptop");
        assert_eq!(sanitize_name("\u{0000}\u{0001}"), "Unnamed device");
        let long = "x".repeat(200);
        assert_eq!(sanitize_name(&long).chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn device_info_sanitizes_everything() {
        let info = DeviceInfo {
            id: DeviceId::from_bytes([1; 10]),
            name: "\tPhone\u{200b}".into(),
            kind: DeviceKind::Phone,
            platform: Platform::Android,
            os_version: "Android 15\n".into(),
            model: Some("   ".into()),
            app_version: "1.0.0".into(),
            capabilities: vec![Capability::Battery, Capability::Unknown, Capability::Battery],
        }
        .sanitized();
        assert_eq!(info.name, "Phone");
        assert_eq!(info.os_version, "Android 15");
        assert_eq!(info.model, None);
        assert_eq!(info.capabilities, vec![Capability::Battery]);
    }

    #[test]
    fn battery_is_clamped() {
        assert_eq!(BatteryState::new(140, true).percent, 100);
        let status = DeviceStatus { battery: Some(BatteryState { percent: 250, charging: false }) };
        assert_eq!(status.sanitized().battery.unwrap().percent, 100);
    }

    #[test]
    fn role_permissions() {
        let t = Permissions::for_role(TrustRole::Trusted);
        assert!(t.control && t.screen_view && t.commands);
        let r = Permissions::for_role(TrustRole::Restricted);
        assert!(!r.control && !r.commands && r.files);
        assert_eq!(TrustRole::parse(TrustRole::Restricted.as_str()), TrustRole::Restricted);
        assert_eq!(TrustRole::parse("anything"), TrustRole::Trusted);
    }
}
