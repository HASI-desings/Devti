//! The built-in profiles (YAML files in `core/profiles/`, compiled in) and matching.

use std::sync::OnceLock;

use super::{Profile, Seen};

const BUILTIN: &[(&str, &str)] = &[
    ("apple-airpods.yaml", include_str!("../../profiles/apple-airpods.yaml")),
    ("sony-headphones.yaml", include_str!("../../profiles/sony-headphones.yaml")),
    ("samsung-galaxy-buds.yaml", include_str!("../../profiles/samsung-galaxy-buds.yaml")),
    ("huawei-freebuds.yaml", include_str!("../../profiles/huawei-freebuds.yaml")),
    ("nothing-ear.yaml", include_str!("../../profiles/nothing-ear.yaml")),
    ("jbl-speakers.yaml", include_str!("../../profiles/jbl-speakers.yaml")),
    ("generic.yaml", include_str!("../../profiles/generic.yaml")),
];

/// Every built-in profile (parsed once; a broken built-in is a build-time bug caught by tests).
pub fn builtin_profiles() -> &'static [Profile] {
    static P: OnceLock<Vec<Profile>> = OnceLock::new();
    P.get_or_init(|| {
        BUILTIN
            .iter()
            .filter_map(|(name, text)| match Profile::from_yaml(text) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::error!(profile = name, error = %e, "built-in profile is invalid");
                    None
                }
            })
            .collect()
    })
}

fn glob(pattern: &str, text: &str) -> bool {
    let (p, t) = (pattern.to_lowercase(), text.to_lowercase());
    let parts: Vec<&str> = p.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match t[pos..].find(part) {
            Some(found) if i > 0 || found == 0 => pos += found + part.len(),
            _ => return false,
        }
    }
    p.ends_with('*') || pos == t.len()
}

fn same_uuid(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().to_lowercase();
        if s.len() == 4 { format!("0000{s}-0000-1000-8000-00805f9b34fb") } else { s }
    };
    norm(a) == norm(b)
}

/// Best profile for a device among `profiles` (extra ones from the user first, then the
/// built-ins). Falls back to the generic profile.
pub fn match_profile<'a>(seen: &Seen, extra: &'a [Profile]) -> &'a Profile
where
    'static: 'a,
{
    let mut best: Option<(&Profile, u32)> = None;
    for p in extra.iter().chain(builtin_profiles().iter()) {
        let m = &p.matcher;
        let mut score = 0;
        if m.manufacturer_ids.iter().any(|id| seen.manufacturer_ids.contains(id)) {
            score += 4;
        }
        if m.names.iter().any(|n| glob(n, &seen.name)) {
            score += 3;
        }
        if m.services.iter().any(|s| seen.services.iter().any(|x| same_uuid(s, x))) {
            score += 2;
        }
        if score > 0 && best.is_none_or(|(_, b)| score > b) {
            best = Some((p, score));
        }
    }
    best.map(|(p, _)| p).unwrap_or_else(|| {
        builtin_profiles().iter().find(|p| p.id == "generic").expect("the generic profile is built in")
    })
}

#[cfg(test)]
pub(super) fn glob_for_tests(p: &str, t: &str) -> bool {
    glob(p, t)
}
