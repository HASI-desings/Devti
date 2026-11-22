use super::codec::{Framer, fill, matches, parse_pattern};
use super::*;

fn hex(s: &str) -> Vec<u8> {
    s.split_whitespace().map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
}

fn profile(id: &str) -> &'static Profile {
    builtin_profiles().iter().find(|p| p.id == id).unwrap()
}

#[test]
fn every_builtin_profile_is_valid() {
    let ids: Vec<&str> = builtin_profiles().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        ["apple-airpods", "sony-headphones", "samsung-galaxy-buds", "huawei-freebuds", "nothing-ear", "jbl", "generic"]
    );
    for p in builtin_profiles() {
        p.validate().unwrap();
    }
}

#[test]
fn matching_prefers_the_strongest_evidence() {
    let seen = |name: &str, ids: &[u16]| Seen { name: name.into(), manufacturer_ids: ids.to_vec(), services: vec![] };
    assert_eq!(match_profile(&seen("Kate's AirPods Pro", &[0x004C]), &[]).id, "apple-airpods");
    assert_eq!(match_profile(&seen("WH-1000XM5", &[]), &[]).id, "sony-headphones");
    assert_eq!(match_profile(&seen("Galaxy Buds2 Pro", &[]), &[]).id, "samsung-galaxy-buds");
    assert_eq!(match_profile(&seen("JBL Flip 6", &[]), &[]).id, "jbl");
    assert_eq!(match_profile(&seen("Logitech MX Master", &[0x046D]), &[]).id, "generic");
    let by_service = Seen {
        name: "?".into(),
        manufacturer_ids: vec![],
        services: vec!["2E73A4AD-332D-41FC-90E2-16BEF06523F2".into()],
    };
    assert_eq!(match_profile(&by_service, &[]).id, "samsung-galaxy-buds");
    // A user profile wins over a built-in one with the same evidence.
    let mut mine = profile("apple-airpods").clone();
    mine.id = "my-airpods".into();
    mine.matcher.manufacturer_ids.push(0xFFFF);
    let extra = [mine];
    assert_eq!(match_profile(&seen("AirPods", &[0x004C, 0xFFFF]), &extra).id, "my-airpods");
}

#[test]
fn globs() {
    use super::library::glob_for_tests as g;
    assert!(g("WF-1000XM*", "wf-1000xm4"));
    assert!(g("*Buds*", "Galaxy Buds Live"));
    assert!(!g("AirPods*", "My AirPods"));
    assert!(g("JBL", "jbl"));
    assert!(!g("JBL", "JBL Flip"));
}

#[test]
fn airpods_controls_and_readings() {
    let p = profile("apple-airpods");
    assert_eq!(p.payload_for("anc", "transparency").unwrap(), hex("04 00 04 00 09 00 0D 03 00 00 00"));
    assert_eq!(p.payload_for("conversation", "off").unwrap(), hex("04 00 04 00 09 00 28 02 00 00 00"));
    assert!(p.payload_for("anc", "loud").is_err());

    // Right 100 % discharging, left 80 % charging, case disconnected (ignored).
    let obs = p.read(&hex("04 00 04 00 04 00 03 02 01 64 02 01 04 01 50 01 01 08 01 0F 04 01"));
    assert_eq!(obs.battery["right"], BatteryLevel { percent: 100, charging: false });
    assert_eq!(obs.battery["left"], BatteryLevel { percent: 80, charging: true });
    assert!(!obs.battery.contains_key("case"));

    let obs = p.read(&hex("04 00 04 00 09 00 0D 04 00 00 00"));
    assert_eq!(obs.values["anc"], "adaptive");
    let obs = p.read(&hex("04 00 04 00 06 00 00 02"));
    assert_eq!(obs.values["ear_primary"], "in ear");
    assert_eq!(obs.values["ear_secondary"], "in case");
    // Unrelated or truncated packets change nothing.
    assert_eq!(p.read(&hex("04 00 04 00")), Observation::default());
    assert_eq!(p.read(&hex("04 00 04 00 04 00 05 02")).battery.len(), 0);
}

#[test]
fn sony_equalizer_slider_and_battery() {
    let p = profile("sony-headphones");
    assert_eq!(p.payload_for("eq", "0,1,-1,10,-10,3").unwrap(), hex("58 01 A0 06 0A 0B 09 14 00 0D"));
    assert_eq!(p.payload_for("eq", "99,0,0,0,0,0").unwrap()[4], 20, "clamped to +10");
    assert!(p.payload_for("eq", "1,2").is_err());
    assert_eq!(p.payload_for("ambient_level", "50").unwrap(), hex("68 02 11 02 00 01 00 14"));
    let obs = p.read(&hex("11 01 46 01 50 00"));
    assert_eq!(obs.battery["left"], BatteryLevel { percent: 70, charging: true });
    assert_eq!(obs.battery["right"], BatteryLevel { percent: 80, charging: false });
}

#[test]
fn huawei_and_galaxy_readings() {
    let h = profile("huawei-freebuds").read(&hex("01 08 01 01 55 02 03 50 4B 1E 03 03 00 00 01"));
    assert_eq!((h.battery["left"].percent, h.battery["right"].percent, h.battery["case"].percent), (80, 75, 30));
    let g = profile("samsung-galaxy-buds").read(&hex("60 0B 5A 5F 01 00 00 3C"));
    assert_eq!((g.battery["left"].percent, g.battery["right"].percent, g.battery["case"].percent), (90, 95, 60));
}

#[test]
fn templates_and_patterns() {
    assert_eq!(fill("68 {v}", &[-1]).unwrap(), vec![0x68, 0xFF]);
    assert!(fill("68 {v}", &[]).is_err());
    assert!(fill("zz", &[]).is_err());
    assert!(fill("", &[]).is_err());
    let pat = parse_pattern("04 ?? 0D").unwrap();
    assert!(matches(&pat, &[4, 9, 0x0D, 1]));
    assert!(!matches(&pat, &[4, 9]));
    assert!(parse_pattern("04 g").is_err());
}

fn round_trip(framing: Framing, payloads: &[Vec<u8>]) {
    let mut tx = Framer::new(framing);
    let mut rx = Framer::new(framing);
    let mut stream = Vec::new();
    for p in payloads {
        stream.extend(tx.encode(p));
    }
    // Delivered in awkward chunks, with noise in front.
    let mut noisy = vec![0x00, 0x13, 0x37];
    noisy.extend(&stream);
    let mut got = Vec::new();
    for chunk in noisy.chunks(5) {
        got.extend(rx.decode(chunk));
    }
    assert_eq!(&got, payloads, "{framing:?}");
}

#[test]
fn vendor_framings_round_trip() {
    let payloads = vec![hex("68 02 11 02 02 01 00 14"), hex("3C 3D 3E 00 FF"), hex("10 01")];
    round_trip(Framing::Sony, &payloads);
    round_trip(Framing::Galaxy, &[hex("78 01"), hex("86 05 3E"), hex("60 0B 5A 5F 01 00 00 3C")]);
    round_trip(Framing::Huawei, &[hex("2B 04 01 01 02"), hex("01 08 01 00 02 00 03 00")]);
    round_trip(Framing::Nothing, &[hex("0F F0 01 05 00"), hex("07 C0")]);
}

#[test]
fn framings_reject_corruption() {
    for framing in [Framing::Sony, Framing::Galaxy, Framing::Huawei, Framing::Nothing] {
        let mut tx = Framer::new(framing);
        let mut frame = tx.encode(&hex("78 01 02 03"));
        let mid = frame.len() / 2;
        frame[mid] ^= 0x01;
        let mut rx = Framer::new(framing);
        assert!(rx.decode(&frame).is_empty(), "{framing:?} accepted a corrupted frame");
        // And it recovers for the next good frame.
        let good = Framer::new(framing).encode(&hex("10 01"));
        assert_eq!(rx.decode(&good), vec![hex("10 01")], "{framing:?}");
    }
}

#[test]
fn sony_escapes_reserved_bytes() {
    let frame = Framer::new(Framing::Sony).encode(&[0x3C, 0x3E]);
    let inner = &frame[1..frame.len() - 1];
    assert!(!inner.contains(&0x3C) && !inner.contains(&0x3E), "{frame:02X?}");
}

#[test]
fn validation_catches_mistakes() {
    let bad = |y: &str| Profile::from_yaml(y).unwrap_err();
    assert!(
        bad("id: x\nname: X\nvendor: v\ncontrols:\n- {id: a, label: A, type: action, send: 'zz'}\n")
            .contains("bad byte")
    );
    assert!(
        bad("id: x\nname: X\nvendor: v\ncontrols:\n- {id: a, label: A, type: action, send: '01'}\n- {id: a, label: B, type: action, send: '02'}\n")
            .contains("twice")
    );
    assert!(
        bad("id: x\nname: X\nvendor: v\nreadings:\n- {match: '01', kind: value, control: nope, byte: 1, map: {}}\n")
            .contains("unknown control")
    );
}

#[test]
fn draft_profile_is_a_valid_starting_point() {
    let seen = Seen { name: "Acme Buds 3".into(), manufacturer_ids: vec![0x1234], services: vec!["180f".into()] };
    let gatt = vec![
        GattChar { service: "180f".into(), uuid: "2a19".into(), properties: vec!["read".into(), "notify".into()] },
        GattChar { service: "fe2c".into(), uuid: "fe2c1234".into(), properties: vec!["write".into()] },
    ];
    let yaml = draft_profile(&seen, &gatt);
    assert!(yaml.contains("id: acme-buds-3"));
    assert!(yaml.contains("0x1234"));
    let p = Profile::from_yaml(&yaml).unwrap();
    assert_eq!(
        p.transport,
        Some(Transport::Gatt { service: "fe2c".into(), write: "fe2c1234".into(), notify: "2a19".into() })
    );
    assert!(!p.verified);
}
