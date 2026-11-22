//! Phase 6 automation: "if this, then that" rules and BLE proximity, as pure logic.
//!
//! The Hub feeds [`Signal`]s in (a device connected, a battery level, the phone walked
//! away, a minute ticked…) and gets back the [`Action`]s to perform. Edge-triggered:
//! "battery below 15 %" fires once when the level crosses down, not on every report;
//! every rule also has a cooldown. Nothing here touches the OS, so it is fully tested.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// What starts a rule.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    /// A paired device came online (`device` empty = any).
    DeviceConnected {
        #[serde(default)]
        device: String,
    },
    DeviceDisconnected {
        #[serde(default)]
        device: String,
    },
    /// A device's battery dropped to or below `percent` while not charging.
    BatteryBelow {
        #[serde(default)]
        device: String,
        percent: u8,
    },
    /// A Bluetooth accessory connected (`name` is a case-insensitive substring; empty = any).
    AccessoryConnected {
        #[serde(default)]
        name: String,
    },
    /// The phone moved away from / came near this computer (BLE signal strength).
    ProximityAway {
        #[serde(default)]
        device: String,
    },
    ProximityNear {
        #[serde(default)]
        device: String,
    },
    /// Every day (or the listed weekdays, 1 = Monday … 7 = Sunday) at hh:mm local time.
    Time {
        hour: u8,
        minute: u8,
        #[serde(default)]
        days: Vec<u8>,
    },
    /// An app started on this computer (process name, case-insensitive).
    AppOpened { app: String },
    /// This computer joined a network (name substring; empty = any change).
    NetworkChanged {
        #[serde(default)]
        network: String,
    },
}

/// What a rule does.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Notify {
        title: String,
        #[serde(default)]
        body: String,
    },
    LockComputer,
    SetDnd {
        on: bool,
    },
    RunCommand {
        command: String,
    },
    LaunchApp {
        app: String,
    },
    /// Make this audio output (name substring) the default.
    SwitchAudioOutput {
        output: String,
    },
    /// Set an accessory control (e.g. noise control → anc).
    AccessoryControl {
        accessory: String,
        control: String,
        value: String,
    },
    RingDevice {
        device: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub actions: Vec<Action>,
    /// Minimum seconds between two runs.
    #[serde(default = "default_cooldown")]
    pub cooldown_s: u32,
}

fn default_cooldown() -> u32 {
    60
}

/// Something that happened, as the Hub sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    DeviceConnected {
        device: String,
        name: String,
    },
    DeviceDisconnected {
        device: String,
        name: String,
    },
    Battery {
        device: String,
        percent: u8,
        charging: bool,
    },
    AccessoryConnected {
        name: String,
    },
    Proximity {
        device: String,
        near: bool,
    },
    /// Local time, once a minute. `weekday` 1 = Monday … 7 = Sunday.
    Minute {
        hour: u8,
        minute: u8,
        weekday: u8,
    },
    AppStarted {
        app: String,
    },
    NetworkChanged {
        network: String,
    },
}

/// Runs rules against signals.
#[derive(Debug, Default)]
pub struct Engine {
    pub rules: Vec<Rule>,
    last_run: HashMap<String, i64>,
    last_battery: HashMap<String, u8>,
}

fn device_matches(want: &str, device: &str, name: &str) -> bool {
    want.is_empty() || want == device || want.eq_ignore_ascii_case(name)
}

impl Engine {
    pub fn new(rules: Vec<Rule>) -> Self {
        Self { rules, ..Default::default() }
    }

    /// The rules this signal fires (respecting cooldowns), with their actions.
    pub fn signal(&mut self, s: &Signal, now_ms: i64, names: &dyn Fn(&str) -> String) -> Vec<(String, Vec<Action>)> {
        // Battery triggers are edge-triggered: remember the previous level.
        let crossed = |prev: Option<u8>, now: u8, at: u8| now <= at && prev.is_none_or(|p| p > at);
        let prev_battery =
            if let Signal::Battery { device, .. } = s { self.last_battery.get(device).copied() } else { None };
        let mut fired = Vec::new();
        for r in &self.rules {
            if !r.enabled {
                continue;
            }
            let hit = match (&r.trigger, s) {
                (Trigger::DeviceConnected { device }, Signal::DeviceConnected { device: d, name }) => {
                    device_matches(device, d, name)
                }
                (Trigger::DeviceDisconnected { device }, Signal::DeviceDisconnected { device: d, name }) => {
                    device_matches(device, d, name)
                }
                (Trigger::BatteryBelow { device, percent }, Signal::Battery { device: d, percent: p, charging }) => {
                    !charging && device_matches(device, d, &names(d)) && crossed(prev_battery, *p, *percent)
                }
                (Trigger::AccessoryConnected { name }, Signal::AccessoryConnected { name: n }) => {
                    name.is_empty() || n.to_lowercase().contains(&name.to_lowercase())
                }
                (Trigger::ProximityAway { device }, Signal::Proximity { device: d, near: false }) => {
                    device_matches(device, d, &names(d))
                }
                (Trigger::ProximityNear { device }, Signal::Proximity { device: d, near: true }) => {
                    device_matches(device, d, &names(d))
                }
                (Trigger::Time { hour, minute, days }, Signal::Minute { hour: h, minute: m, weekday }) => {
                    hour == h && minute == m && (days.is_empty() || days.contains(weekday))
                }
                (Trigger::AppOpened { app }, Signal::AppStarted { app: a }) => app.eq_ignore_ascii_case(a),
                (Trigger::NetworkChanged { network }, Signal::NetworkChanged { network: n }) => {
                    network.is_empty() || n.to_lowercase().contains(&network.to_lowercase())
                }
                _ => false,
            };
            if !hit {
                continue;
            }
            if let Some(last) = self.last_run.get(&r.id)
                && now_ms - last < r.cooldown_s as i64 * 1000
            {
                continue;
            }
            fired.push((r.id.clone(), r.actions.clone()));
        }
        for (id, _) in &fired {
            self.last_run.insert(id.clone(), now_ms);
        }
        if let Signal::Battery { device, percent, .. } = s {
            self.last_battery.insert(device.clone(), *percent);
        }
        fired
    }
}

/// The rules a new Hub starts with (the brief's examples), all switched off until the
/// user turns them on — nothing should lock a computer by surprise.
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "earbuds-audio".into(),
            name: "Earbuds connect → play this computer's sound through them".into(),
            enabled: false,
            trigger: Trigger::AccessoryConnected { name: String::new() },
            actions: vec![Action::SwitchAudioOutput { output: String::new() }],
            cooldown_s: 10,
        },
        Rule {
            id: "phone-low-battery".into(),
            name: "Phone battery below 15 % → alert on this computer".into(),
            enabled: true,
            trigger: Trigger::BatteryBelow { device: String::new(), percent: 15 },
            actions: vec![Action::Notify { title: "Battery low".into(), body: "Plug it in soon.".into() }],
            cooldown_s: 600,
        },
        Rule {
            id: "walk-away-lock".into(),
            name: "Phone walks away → lock this computer".into(),
            enabled: false,
            trigger: Trigger::ProximityAway { device: String::new() },
            actions: vec![Action::LockComputer],
            cooldown_s: 120,
        },
    ]
}

/// BLE proximity beacon payload for a device in a 15-minute epoch:
/// `HMAC-SHA256(key = the device's certificate fingerprint, "devti-proximity" ‖ epoch)[..8]`.
///
/// Only devices that paired with it (and so know its fingerprint) can recognise the
/// beacon; to anyone else it is 8 random-looking bytes that change every 15 minutes, so
/// the phone can't be tracked by it.
pub fn proximity_token(fingerprint: &[u8; 32], epoch: u64) -> [u8; 8] {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, fingerprint);
    let mut msg = b"devti-proximity".to_vec();
    msg.extend_from_slice(&epoch.to_be_bytes());
    let tag = ring::hmac::sign(&key, &msg);
    let mut out = [0u8; 8];
    out.copy_from_slice(&tag.as_ref()[..8]);
    out
}

/// The 15-minute epoch for a Unix time in milliseconds.
pub fn proximity_epoch(now_ms: i64) -> u64 {
    (now_ms.max(0) as u64) / (15 * 60 * 1000)
}

/// BLE service UUID the Devti phone app advertises its proximity token under.
pub const PROXIMITY_SERVICE_UUID: &str = "6a1f0001-7d6e-4b8a-9e36-d3e7f1a0c0de";

/// Near/away from BLE signal strength, with smoothing and hysteresis so a phone at the
/// edge doesn't flap: near above −65 dBm, away below −80 dBm or after 30 s unseen.
#[derive(Debug, Default)]
pub struct Proximity {
    avg: Option<f64>,
    last_seen: Option<i64>,
    near: Option<bool>,
}

impl Proximity {
    const NEAR_DBM: f64 = -65.0;
    const AWAY_DBM: f64 = -80.0;
    const LOST_MS: i64 = 30_000;

    /// Feeds one RSSI reading; returns the new state when it changes.
    pub fn reading(&mut self, rssi: i16, now_ms: i64) -> Option<bool> {
        self.last_seen = Some(now_ms);
        let r = rssi as f64;
        let avg = match self.avg {
            None => r,
            Some(a) => a * 0.75 + r * 0.25,
        };
        self.avg = Some(avg);
        let next = if avg >= Self::NEAR_DBM {
            Some(true)
        } else if avg <= Self::AWAY_DBM {
            Some(false)
        } else {
            self.near // in between: keep the current state
        };
        self.update(next)
    }

    /// Call periodically: a phone that stops advertising is away.
    pub fn tick(&mut self, now_ms: i64) -> Option<bool> {
        if self.last_seen.is_some_and(|t| now_ms - t > Self::LOST_MS) {
            self.avg = None;
            return self.update(Some(false));
        }
        None
    }

    fn update(&mut self, next: Option<bool>) -> Option<bool> {
        if next.is_some() && next != self.near {
            self.near = next;
            return next;
        }
        None
    }

    pub fn is_near(&self) -> Option<bool> {
        self.near
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_names(_: &str) -> String {
        String::new()
    }

    fn rule(id: &str, trigger: Trigger) -> Rule {
        Rule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            trigger,
            actions: vec![Action::LockComputer],
            cooldown_s: 60,
        }
    }

    #[test]
    fn battery_fires_once_on_the_way_down() {
        let mut e = Engine::new(vec![rule("low", Trigger::BatteryBelow { device: "P".into(), percent: 15 })]);
        let b = |p: u8, charging: bool| Signal::Battery { device: "P".into(), percent: p, charging };
        assert!(e.signal(&b(40, false), 0, &no_names).is_empty());
        assert_eq!(e.signal(&b(15, false), 1_000, &no_names).len(), 1, "crossed 15");
        assert!(e.signal(&b(14, false), 200_000, &no_names).is_empty(), "already below: no repeat");
        assert!(e.signal(&b(30, true), 300_000, &no_names).is_empty(), "charging back up");
        assert_eq!(e.signal(&b(12, false), 400_000, &no_names).len(), 1, "crossed again later");
        assert!(e.signal(&b(10, true), 500_000, &no_names).is_empty(), "charging never fires");
    }

    #[test]
    fn cooldown_and_disabled_rules() {
        let mut e = Engine::new(vec![rule("c", Trigger::DeviceConnected { device: String::new() })]);
        let s = Signal::DeviceConnected { device: "X".into(), name: "Pixel".into() };
        assert_eq!(e.signal(&s, 0, &no_names).len(), 1);
        assert!(e.signal(&s, 30_000, &no_names).is_empty(), "inside the 60 s cooldown");
        assert_eq!(e.signal(&s, 61_000, &no_names).len(), 1);
        e.rules[0].enabled = false;
        assert!(e.signal(&s, 999_000, &no_names).is_empty());
    }

    #[test]
    fn devices_match_by_id_or_name_and_triggers_by_kind() {
        let mut e = Engine::new(vec![
            rule("by-name", Trigger::DeviceDisconnected { device: "pixel 8".into() }),
            rule("time", Trigger::Time { hour: 9, minute: 0, days: vec![1, 2, 3, 4, 5] }),
            rule("acc", Trigger::AccessoryConnected { name: "airpods".into() }),
            rule("app", Trigger::AppOpened { app: "zoom".into() }),
            rule("away", Trigger::ProximityAway { device: String::new() }),
        ]);
        let fired = |e: &mut Engine, s: Signal, t: i64| {
            e.signal(&s, t, &no_names).into_iter().map(|(id, _)| id).collect::<Vec<_>>()
        };
        assert_eq!(
            fired(&mut e, Signal::DeviceDisconnected { device: "ID".into(), name: "Pixel 8".into() }, 0),
            ["by-name"]
        );
        assert_eq!(fired(&mut e, Signal::Minute { hour: 9, minute: 0, weekday: 3 }, 0), ["time"]);
        assert!(fired(&mut e, Signal::Minute { hour: 9, minute: 0, weekday: 6 }, 100_000).is_empty(), "Saturday");
        assert_eq!(fired(&mut e, Signal::AccessoryConnected { name: "Kate's AirPods Pro".into() }, 0), ["acc"]);
        assert_eq!(fired(&mut e, Signal::AppStarted { app: "Zoom".into() }, 0), ["app"]);
        assert_eq!(fired(&mut e, Signal::Proximity { device: "P".into(), near: false }, 0), ["away"]);
        assert!(fired(&mut e, Signal::Proximity { device: "P".into(), near: true }, 500_000).is_empty());
    }

    #[test]
    fn rules_round_trip_as_json() {
        let rules = default_rules();
        let json = serde_json::to_string(&rules).unwrap();
        assert_eq!(serde_json::from_str::<Vec<Rule>>(&json).unwrap(), rules);
        assert!(json.contains(r#""kind":"battery_below""#));
        assert!(!rules.iter().find(|r| r.id == "walk-away-lock").unwrap().enabled, "locking is opt-in");
    }

    #[test]
    fn proximity_tokens_rotate_and_differ_per_device() {
        let a = [7u8; 32];
        let b = [8u8; 32];
        assert_eq!(proximity_token(&a, 10), proximity_token(&a, 10));
        assert_ne!(proximity_token(&a, 10), proximity_token(&a, 11), "rotates every epoch");
        assert_ne!(proximity_token(&a, 10), proximity_token(&b, 10), "differs per device");
        assert_eq!(proximity_epoch(15 * 60 * 1000 * 3 + 5), 3);
    }

    #[test]
    fn proximity_smooths_and_has_hysteresis() {
        let mut p = Proximity::default();
        assert_eq!(p.reading(-55, 0), Some(true));
        assert_eq!(p.reading(-70, 1_000), None, "in the gap: stays near");
        for i in 0..6 {
            p.reading(-90, 2_000 + i * 1_000);
        }
        assert_eq!(p.is_near(), Some(false), "averaged down past -80");
        assert_eq!(p.reading(-72, 9_000), None, "in the gap: stays away");
        let mut q = Proximity::default();
        q.reading(-50, 0);
        assert_eq!(q.tick(10_000), None);
        assert_eq!(q.tick(31_000), Some(false), "unseen for 30 s");
    }
}
