//! `devti` — the command-line client for the Devti Hub's local API.
//!
//! It finds the API the same way any script can: the token and port files the Hub
//! writes into its data folder (or `DEVTI_API_TOKEN` / `DEVTI_API_PORT`), and talks
//! HTTP to 127.0.0.1. See `core/src/api.rs` for the endpoints.

use std::io::Read;
use std::path::PathBuf;

use serde_json::{Value, json};

pub const USAGE: &str = "\
devti — script your Devti ecosystem

USAGE
  devti status                      this computer
  devti devices                     paired devices and whether they're online
  devti send <device> <file>...     send files (device = name or id)
  devti ring <device> [--stop]      find my device
  devti clip                        print the latest clipboard text
  devti clip <text> | -             share text (or stdin) to every device's clipboard
  devti commands                    saved commands on this computer
  devti run <command> [--on <device>]
                                    run a saved command here or on another device
  devti watch                       print every event as a JSON line (Ctrl+C to stop)

The Devti Hub must be running. Settings → Local API shows the token and lets you turn
the API off. Environment overrides: DEVTI_API_PORT, DEVTI_API_TOKEN, DEVTI_DATA_DIR.";

/// Where the Hub keeps its token and port files.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("DEVTI_DATA_DIR") {
        return Some(PathBuf::from(d));
    }
    dirs::data_dir().map(|d| d.join("app.devti.hub"))
}

/// How to reach the API.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
}

impl Endpoint {
    pub fn discover() -> Result<Self, String> {
        let dir = data_dir();
        let read = |name: &str| dir.as_ref().and_then(|d| std::fs::read_to_string(d.join(name)).ok());
        let token = std::env::var("DEVTI_API_TOKEN").ok().or_else(|| read("api-token")).map(|t| t.trim().to_string());
        let port =
            std::env::var("DEVTI_API_PORT").ok().or_else(|| read("api-port")).and_then(|p| p.trim().parse().ok());
        match (token, port) {
            (Some(token), Some(port)) if !token.is_empty() => Ok(Self { port, token }),
            _ => {
                Err("Can't find the Devti Hub's local API. Is Devti running, with Settings → Local API turned on?"
                    .into())
            }
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(75)))
            .build()
            .into()
    }

    pub fn get(&self, path: &str) -> Result<Value, String> {
        let res = Self::agent()
            .get(&self.url(path))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| format!("Devti isn't answering: {e}"))?;
        Self::read(res)
    }

    pub fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let res = Self::agent()
            .post(&self.url(path))
            .header("Authorization", &format!("Bearer {}", self.token))
            .send_json(&body)
            .map_err(|e| format!("Devti isn't answering: {e}"))?;
        Self::read(res)
    }

    fn read(mut res: ureq::http::Response<ureq::Body>) -> Result<Value, String> {
        let ok = res.status().is_success();
        let v: Value = res.body_mut().read_json().unwrap_or(Value::Null);
        if ok { Ok(v) } else { Err(v.get("error").and_then(Value::as_str).unwrap_or("request failed").to_string()) }
    }
}

fn absolute(p: &str) -> String {
    std::fs::canonicalize(p).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| p.to_string())
}

/// Runs one command and returns what to print.
pub fn run(args: &[String], api: &Endpoint) -> Result<String, String> {
    let arg = |i: usize| args.get(i).map(String::as_str);
    match arg(0) {
        Some("status") => {
            let v = api.get("/v1/status")?;
            Ok(format!(
                "{} ({})\nid {}\nfingerprint {}{}",
                v["info"]["name"].as_str().unwrap_or("?"),
                v["info"]["kind"].as_str().unwrap_or("?"),
                v["info"]["id"].as_str().unwrap_or("?"),
                v["fingerprint"].as_str().unwrap_or("?"),
                if v["paused"].as_bool() == Some(true) { "\nPAUSED" } else { "" },
            ))
        }
        Some("devices") => {
            let v = api.get("/v1/devices")?;
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                return Ok("No paired devices yet.".into());
            }
            Ok(list
                .iter()
                .map(|p| {
                    let battery =
                        p["status"]["battery"]["percent"].as_u64().map(|b| format!("  {b}%")).unwrap_or_default();
                    format!(
                        "{}  {:<24} {:<8} {}{}",
                        if p["online"].as_bool() == Some(true) { "●" } else { "○" },
                        p["info"]["name"].as_str().unwrap_or("?"),
                        p["info"]["kind"].as_str().unwrap_or("?"),
                        p["info"]["id"].as_str().unwrap_or("?"),
                        battery
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("send") => {
            let device = arg(1).ok_or("usage: devti send <device> <file>...")?;
            let files: Vec<String> = args.iter().skip(2).map(|p| absolute(p)).collect();
            if files.is_empty() {
                return Err("usage: devti send <device> <file>...".into());
            }
            let v = api.post("/v1/send", json!({ "device": device, "paths": files }))?;
            let n = v["transfers"].as_array().map(Vec::len).unwrap_or(0);
            Ok(format!("Sending {n} file{} to {device}. Progress is in Devti → Files.", if n == 1 { "" } else { "s" }))
        }
        Some("ring") => {
            let device = arg(1).ok_or("usage: devti ring <device> [--stop]")?;
            let on = !args.iter().any(|a| a == "--stop");
            api.post("/v1/ring", json!({ "device": device, "on": on }))?;
            Ok(if on { format!("Ringing {device}.") } else { format!("Stopped ringing {device}.") })
        }
        Some("clip") => match arg(1) {
            None => {
                let v = api.get("/v1/clip")?;
                Ok(v["text"].as_str().unwrap_or("").to_string())
            }
            Some(t) => {
                let text = if t == "-" {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
                    s
                } else {
                    args[1..].join(" ")
                };
                let v = api.post("/v1/clip", json!({ "text": text }))?;
                Ok(if v["sent"].as_bool() == Some(true) {
                    "Shared to your devices' clipboards.".into()
                } else {
                    "Nothing sent (empty, too large, or no device online).".into()
                })
            }
        },
        Some("commands") => {
            let v = api.get("/v1/commands")?;
            let list = v.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                return Ok("No saved commands. Add some in Devti → Control.".into());
            }
            Ok(list
                .iter()
                .map(|c| format!("{:<20} {}", c["id"].as_str().unwrap_or("?"), c["name"].as_str().unwrap_or("?")))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("run") => {
            let command = arg(1).ok_or("usage: devti run <command> [--on <device>]")?;
            let device = args.iter().position(|a| a == "--on").and_then(|i| args.get(i + 1)).cloned();
            let v = api.post("/v1/run", json!({ "command": command, "device": device }))?;
            let out = v["output"].as_str().unwrap_or("").to_string();
            if v["ok"].as_bool() == Some(true) {
                Ok(out)
            } else {
                Err(if out.is_empty() { "The command failed.".into() } else { out })
            }
        }
        Some("help") | Some("--help") | Some("-h") | None => Ok(USAGE.into()),
        Some("--version") | Some("-V") => Ok(format!("devti {}", env!("CARGO_PKG_VERSION"))),
        Some(other) => Err(format!("unknown command \"{other}\"\n\n{USAGE}")),
    }
}

/// `devti watch`: prints events until the connection closes.
pub fn watch(api: &Endpoint, mut out: impl FnMut(&str) -> bool) -> Result<(), String> {
    let url = format!("ws://127.0.0.1:{}/v1/events?token={}", api.port, api.token);
    let (mut socket, _) = tungstenite::connect(url).map_err(|e| format!("Devti isn't answering: {e}"))?;
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(t)) => {
                if !out(t.as_str()) {
                    let _ = socket.close(None);
                    return Ok(());
                }
            }
            Ok(tungstenite::Message::Close(_)) | Err(_) => return Ok(()),
            Ok(_) => {}
        }
    }
}
