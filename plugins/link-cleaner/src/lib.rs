//! Example Devti plugin: removes tracking parameters from links.
//!
//! * `command("clean")` cleans the link on this computer's clipboard.
//! * `on_event` cleans a link that arrives from another device's clipboard.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown`, then put
//! `target/wasm32-unknown-unknown/release/link_cleaner.wasm` next to `devti-plugin.json`
//! as `plugin.wasm` and add the folder in Devti → Plugins.

#[cfg(target_arch = "wasm32")]
mod plugin {
    use extism_pdk::*;
    use serde::Deserialize;

    #[host_fn]
    extern "ExtismHost" {
        fn devti_clipboard_get(unused: String) -> String;
        fn devti_clipboard_set(text: String) -> String;
        fn devti_notify(json: String) -> String;
        fn devti_log(line: String) -> String;
    }

    fn clean_clipboard(text: &str) -> FnResult<usize> {
        let (cleaned, removed) = crate::clean_text(text);
        if removed > 0 {
            unsafe { devti_clipboard_set(cleaned)? };
        }
        Ok(removed)
    }

    #[plugin_fn]
    pub fn command(id: String) -> FnResult<String> {
        if id != "clean" {
            return Ok(format!("Unknown command “{id}”."));
        }
        let text = unsafe { devti_clipboard_get(String::new())? };
        let removed = clean_clipboard(&text)?;
        Ok(match removed {
            0 => "Nothing to remove: the copied link has no tracking parameters.".into(),
            1 => "Removed 1 tracking parameter.".into(),
            n => format!("Removed {n} tracking parameters."),
        })
    }

    #[derive(Deserialize)]
    struct Event {
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        from_name: String,
    }

    #[plugin_fn]
    pub fn on_event(json: String) -> FnResult<String> {
        let ev: Event = serde_json::from_str(&json)?;
        if ev.kind == "clipboard" {
            let removed = clean_clipboard(&ev.text)?;
            if removed > 0 {
                unsafe { devti_log(format!("cleaned a link from {}", ev.from_name))? };
                let note = serde_json::json!({ "title": "Link cleaned", "body": format!("Removed {removed} tracking parameter(s) from the link {} sent.", ev.from_name) });
                unsafe { devti_notify(note.to_string())? };
            }
        }
        Ok(String::new())
    }
}

/// Query keys that only exist to track where a click came from.
const TRACKERS: &[&str] = &[
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "mc_cid", "mc_eid", "igshid", "yclid", "twclid", "si",
    "_hsenc", "_hsmi", "mkt_tok", "ref_src", "spm",
];

fn is_tracker(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    k.starts_with("utm_") || TRACKERS.contains(&k.as_str())
}

/// Cleans one http(s) link; returns it with the number of parameters removed.
pub fn clean_url(url: &str) -> (String, usize) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return (url.to_string(), 0);
    }
    let (rest, fragment) = match url.split_once('#') {
        Some((r, f)) => (r, Some(f)),
        None => (url, None),
    };
    let Some((base, query)) = rest.split_once('?') else { return (url.to_string(), 0) };
    let mut removed = 0;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| {
            if p.is_empty() {
                return false;
            }
            let key = p.split('=').next().unwrap_or("");
            let drop = is_tracker(key);
            removed += drop as usize;
            !drop
        })
        .collect();
    if removed == 0 {
        return (url.to_string(), 0);
    }
    let mut out = base.to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    if let Some(f) = fragment {
        out.push('#');
        out.push_str(f);
    }
    (out, removed)
}

/// Cleans every link in a piece of text.
pub fn clean_text(text: &str) -> (String, usize) {
    let mut total = 0;
    let out: Vec<String> = text
        .split(' ')
        .map(|word| {
            let (w, n) = clean_url(word);
            total += n;
            w
        })
        .collect();
    (out.join(" "), total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trackers_go_and_everything_else_stays() {
        assert_eq!(clean_url("https://x.org/a?id=7&utm_source=mail&fbclid=1"), ("https://x.org/a?id=7".into(), 2));
        assert_eq!(clean_url("https://x.org/a?utm_medium=x#top"), ("https://x.org/a#top".into(), 1));
        assert_eq!(clean_url("https://youtu.be/abc?si=xyz&t=30"), ("https://youtu.be/abc?t=30".into(), 1));
        assert_eq!(clean_url("https://x.org/a?id=7"), ("https://x.org/a?id=7".into(), 0));
        assert_eq!(clean_url("https://x.org/a"), ("https://x.org/a".into(), 0));
        assert_eq!(clean_url("ftp://x.org/a?utm_source=1"), ("ftp://x.org/a?utm_source=1".into(), 0));
        assert_eq!(clean_url("https://x.org/?GCLID=1"), ("https://x.org/".into(), 1));
    }

    #[test]
    fn text_with_several_links() {
        let (t, n) = clean_text("see https://a.org/?utm_campaign=x and https://b.org/p?q=1&gclid=2");
        assert_eq!(t, "see https://a.org/ and https://b.org/p?q=1");
        assert_eq!(n, 2);
    }
}
