//! Bluetooth accessories (Phase 5): the **device profile library** and everything about
//! profiles that doesn't need a radio — so it is shared by every platform and unit-tested.
//!
//! * [`Profile`] — a declarative, community-maintained description (YAML) of an accessory:
//!   how to recognise it, how to talk to it (L2CAP, RFCOMM or GATT, plus the vendor's
//!   framing), what to send for each control, and how to read state and batteries from
//!   what it sends back. The UI is generated from it.
//! * [`library`] — the built-in profiles and matching ([`match_profile`]).
//! * [`codec`] — hex templates, wildcard patterns and vendor framings (Apple AAP raw,
//!   Sony MDR, Samsung Galaxy Buds, Huawei FreeBuds, Nothing).
//! * [`draft_profile`] — turns an unknown device's GATT layout into a draft profile the
//!   user can submit ("Help support this device"). Nothing is ever sent automatically.

pub mod codec;
mod library;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use library::{builtin_profiles, match_profile};

/// How a profile recognises a device.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Matcher {
    /// Bluetooth SIG company identifiers from advertisements / device info.
    #[serde(default)]
    pub manufacturer_ids: Vec<u16>,
    /// Name globs: `*` matches anything, case-insensitive ("WF-1000XM*").
    #[serde(default)]
    pub names: Vec<String>,
    /// Service UUIDs (16-bit short form like "180f" or full UUIDs).
    #[serde(default)]
    pub services: Vec<String>,
}

/// The channel a profile's vendor protocol runs over.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transport {
    /// Classic L2CAP (seqpacket), e.g. Apple's AAP on PSM 0x1001.
    L2cap { psm: u16 },
    /// Classic RFCOMM, by the vendor's SDP service UUID.
    Rfcomm { uuid: String },
    /// BLE GATT: write to one characteristic, read notifications from another.
    Gatt { service: String, write: String, notify: String },
}

/// The vendor's packet format around each payload.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Framing {
    /// Payloads as they are (one L2CAP packet or GATT write each).
    #[default]
    Raw,
    /// Sony MDR: 0x3E … checksum 0x3C with byte escaping and sequence numbers.
    Sony,
    /// Samsung Galaxy Buds: 0xFD len id … CRC-16/XMODEM 0xDD.
    Galaxy,
    /// Huawei FreeBuds: 0x5A len 0x00 … CRC-16/XMODEM.
    Huawei,
    /// Nothing Ear: 0x55 0x60 0x01 cmd len … CRC-16/MODBUS.
    Nothing,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
    /// Hex payload template sent when chosen.
    pub send: String,
}

/// One generated control. `send` strings are hex templates (see [`codec::fill`]).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlKind {
    Toggle {
        on: String,
        off: String,
    },
    Choice {
        options: Vec<ChoiceOption>,
    },
    Slider {
        min: i32,
        max: i32,
        #[serde(default = "one")]
        step: i32,
        send: String,
    },
    /// `offset` is added to each band before sending (Sony sends 0–20 for −10…+10).
    Equalizer {
        bands: Vec<String>,
        min: i32,
        max: i32,
        #[serde(default)]
        offset: i32,
        send: String,
    },
    Action {
        send: String,
    },
}

fn one() -> i32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Control {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub hint: String,
    #[serde(flatten)]
    pub kind: ControlKind,
}

/// One battery component inside a payload, at a fixed byte.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatteryByte {
    pub name: String,
    pub byte: usize,
    /// Bit in the same byte that means "charging" (e.g. 0x80), if the vendor uses one.
    #[serde(default)]
    pub charging_bit: Option<u8>,
    /// Or a separate byte that is non-zero while charging.
    #[serde(default)]
    pub charging_byte: Option<usize>,
}

/// How to read a matched payload.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Parse {
    /// Sets a control's current value from one byte.
    Value { control: String, byte: usize, map: BTreeMap<String, String> },
    /// Battery levels at fixed bytes.
    Battery { components: Vec<BatteryByte> },
    /// Apple's variable battery list (component, level, status records).
    AapBattery,
    /// Apple's in-ear detection (primary, secondary).
    AapEar,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reading {
    /// Hex with `??` wildcards, matched against the start of each decoded payload.
    #[serde(rename = "match")]
    pub pattern: String,
    #[serde(flatten)]
    pub parse: Parse,
}

/// A device profile from the library.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// Tested end to end on real hardware by contributors. Unverified profiles are marked
    /// as such in the UI.
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(rename = "match", default)]
    pub matcher: Matcher,
    #[serde(default)]
    pub transport: Option<Transport>,
    #[serde(default)]
    pub framing: Framing,
    /// Payloads sent right after connecting (handshakes, "send me notifications").
    #[serde(default)]
    pub init: Vec<String>,
    #[serde(default)]
    pub controls: Vec<Control>,
    #[serde(default)]
    pub readings: Vec<Reading>,
}

impl Profile {
    pub fn from_yaml(text: &str) -> Result<Profile, String> {
        let p: Profile = serde_yaml_ng::from_str(text).map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }

    /// Checks every template and pattern parses, ids are unique, readings point at controls.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.name.is_empty() {
            return Err("a profile needs an id and a name".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.controls {
            if !ids.insert(c.id.as_str()) {
                return Err(format!("control id {} is used twice", c.id));
            }
            let templates: Vec<&str> = match &c.kind {
                ControlKind::Toggle { on, off } => vec![on, off],
                ControlKind::Choice { options } => options.iter().map(|o| o.send.as_str()).collect(),
                ControlKind::Slider { send, .. }
                | ControlKind::Equalizer { send, .. }
                | ControlKind::Action { send } => {
                    vec![send]
                }
            };
            for t in templates {
                codec::parse_template(t).map_err(|e| format!("control {}: {e}", c.id))?;
            }
        }
        for t in &self.init {
            codec::parse_template(t).map_err(|e| format!("init: {e}"))?;
        }
        for r in &self.readings {
            codec::parse_pattern(&r.pattern).map_err(|e| format!("reading: {e}"))?;
            if let Parse::Value { control, .. } = &r.parse
                && !ids.contains(control.as_str())
            {
                return Err(format!("reading sets unknown control {control}"));
            }
        }
        Ok(())
    }

    /// The payload to send for a control change. `value` is the option value, "on"/"off",
    /// a number for sliders, or comma-separated numbers for an equalizer.
    pub fn payload_for(&self, control: &str, value: &str) -> Result<Vec<u8>, String> {
        let c = self.controls.iter().find(|c| c.id == control).ok_or("no such control")?;
        match &c.kind {
            ControlKind::Toggle { on, off } => codec::fill(if value == "on" { on } else { off }, &[]),
            ControlKind::Choice { options } => {
                let o = options.iter().find(|o| o.value == value).ok_or("no such option")?;
                codec::fill(&o.send, &[])
            }
            ControlKind::Slider { min, max, send, .. } => {
                let v: i32 = value.parse().map_err(|_| "not a number")?;
                codec::fill(send, &[v.clamp(*min, *max)])
            }
            ControlKind::Equalizer { bands, min, max, offset, send } => {
                let vs: Result<Vec<i32>, _> = value.split(',').map(|s| s.trim().parse::<i32>()).collect();
                let vs = vs.map_err(|_| "not numbers")?;
                if vs.len() != bands.len() {
                    return Err(format!("expected {} bands", bands.len()));
                }
                let vs: Vec<i32> = vs.into_iter().map(|v| v.clamp(*min, *max) + offset).collect();
                codec::fill(send, &vs)
            }
            ControlKind::Action { send } => codec::fill(send, &[]),
        }
    }

    /// Reads one decoded payload: control values and battery components it carries.
    pub fn read(&self, payload: &[u8]) -> Observation {
        let mut obs = Observation::default();
        for r in &self.readings {
            let Ok(pattern) = codec::parse_pattern(&r.pattern) else { continue };
            if !codec::matches(&pattern, payload) {
                continue;
            }
            match &r.parse {
                Parse::Value { control, byte, map } => {
                    if let Some(b) = payload.get(*byte)
                        && let Some(v) = map.get(&b.to_string())
                    {
                        obs.values.insert(control.clone(), v.clone());
                    }
                }
                Parse::Battery { components } => {
                    for c in components {
                        if let Some(&b) = payload.get(c.byte) {
                            let charging = c.charging_bit.is_some_and(|bit| b & bit != 0)
                                || c.charging_byte.and_then(|i| payload.get(i)).is_some_and(|v| *v != 0);
                            let level = b & !c.charging_bit.unwrap_or(0);
                            if level <= 100 {
                                obs.battery.insert(c.name.clone(), BatteryLevel { percent: level, charging });
                            }
                        }
                    }
                }
                Parse::AapBattery => obs.battery.extend(codec::aap_battery(payload)),
                Parse::AapEar => {
                    if let (Some(&p), Some(&s)) = (payload.get(6), payload.get(7)) {
                        let label = |b: u8| match b {
                            0 => "in ear",
                            1 => "out of ear",
                            2 => "in case",
                            _ => "unknown",
                        };
                        obs.values.insert("ear_primary".into(), label(p).into());
                        obs.values.insert("ear_secondary".into(), label(s).into());
                    }
                }
            }
        }
        obs
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatteryLevel {
    pub percent: u8,
    pub charging: bool,
}

/// What one payload told us.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Observation {
    pub values: BTreeMap<String, String>,
    pub battery: BTreeMap<String, BatteryLevel>,
}

/// What the radio layer knows about a device, for matching.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Seen {
    pub name: String,
    #[serde(default)]
    pub manufacturer_ids: Vec<u16>,
    #[serde(default)]
    pub services: Vec<String>,
}

/// A GATT characteristic of an unknown device, for "Help support this device".
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GattChar {
    pub service: String,
    pub uuid: String,
    /// "read", "write", "write_without_response", "notify", "indicate".
    pub properties: Vec<String>,
}

/// A draft profile for an unknown device: its identity and GATT layout, ready to edit and
/// submit as a GitHub contribution. Only structure goes in — no values read from the
/// device, no addresses.
pub fn draft_profile(seen: &Seen, gatt: &[GattChar]) -> String {
    let slug: String = seen
        .name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let mut out = String::new();
    out.push_str("# Draft Devti device profile — fill in the controls and open a pull request:\n");
    out.push_str("# https://github.com/hasi-desings/devti/blob/main/docs/PLUGINS.md#device-profiles\n");
    out.push_str(&format!("id: {}\n", if slug.is_empty() { "unknown-device".into() } else { slug }));
    out.push_str(&format!("name: {:?}\nvendor: \"\"\nverified: false\n", seen.name));
    out.push_str("match:\n");
    if !seen.manufacturer_ids.is_empty() {
        let ids: Vec<String> = seen.manufacturer_ids.iter().map(|i| format!("0x{i:04X}")).collect();
        out.push_str(&format!("  manufacturer_ids: [{}]\n", ids.join(", ")));
    }
    out.push_str(&format!("  names: [{:?}]\n", seen.name));
    if !seen.services.is_empty() {
        let s: Vec<String> = seen.services.iter().map(|s| format!("{s:?}")).collect();
        out.push_str(&format!("  services: [{}]\n", s.join(", ")));
    }
    out.push_str("# GATT layout found on the device:\n");
    let mut services: Vec<&str> = gatt.iter().map(|g| g.service.as_str()).collect();
    services.dedup();
    for s in services {
        out.push_str(&format!("#   service {s}\n"));
        for g in gatt.iter().filter(|g| g.service == s) {
            out.push_str(&format!("#     {} [{}]\n", g.uuid, g.properties.join(", ")));
        }
    }
    let writable = gatt.iter().find(|g| g.properties.iter().any(|p| p.starts_with("write")));
    let notify = gatt.iter().find(|g| g.properties.iter().any(|p| p == "notify" || p == "indicate"));
    if let (Some(w), Some(n)) = (writable, notify) {
        out.push_str(&format!(
            "transport:\n  kind: gatt\n  service: {:?}\n  write: {:?}\n  notify: {:?}\n",
            w.service, w.uuid, n.uuid
        ));
    }
    out.push_str("controls: []\nreadings: []\n");
    out
}

#[cfg(test)]
mod tests;
