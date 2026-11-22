//! Local API for scripts and the `devti` CLI (feature `api`).
//!
//! * Listens on **127.0.0.1 only** (default port 47811; the port actually used is written to
//!   `<data dir>/api-port`).
//! * Every request needs `Authorization: Bearer <token>`. The token is 32 random bytes,
//!   stored in `<data dir>/api-token` readable only by the user (0600 on Unix). The
//!   WebSocket also accepts `?token=` because browsers can't set headers on it.
//! * Requests whose `Host` isn't a loopback name are refused (blocks DNS rebinding), and
//!   there are no CORS headers, so web pages can't call it.
//!
//! | Method | Path | Body | Does |
//! |---|---|---|---|
//! | GET  | `/v1/status` | | this device |
//! | GET  | `/v1/devices` | | paired devices |
//! | POST | `/v1/send` | `{device, paths}` | send files |
//! | POST | `/v1/ring` | `{device, on?}` | find my device |
//! | GET  | `/v1/clip` | | latest clipboard entry |
//! | POST | `/v1/clip` | `{text}` | share text to every device's clipboard |
//! | GET  | `/v1/commands` | | this device's saved commands |
//! | POST | `/v1/run` | `{command, device?}` | run a saved command here or on a device |
//! | POST | `/v1/input` | `{device, events}` | remote input |
//! | GET  | `/v1/events` | | WebSocket of every event as JSON |

use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::node::{Node, NodeEvent};
use crate::protocol::InputEvent;

pub const DEFAULT_PORT: u16 = 47811;
pub const TOKEN_FILE: &str = "api-token";
pub const PORT_FILE: &str = "api-port";

/// Runs one of this device's saved commands: `(ok, output)`.
pub type LocalRunner = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = (bool, String)> + Send>> + Send + Sync>;

#[derive(Clone)]
struct Ctx {
    node: Node,
    token: Arc<String>,
    runner: Option<LocalRunner>,
}

/// A running API server.
pub struct ApiServer {
    pub addr: SocketAddr,
    pub token_path: PathBuf,
    task: tokio::task::JoinHandle<()>,
}

impl ApiServer {
    pub fn stop(self) {
        self.task.abort();
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Reads the token, creating it on first use.
pub fn load_or_create_token(dir: &Path) -> Result<String> {
    let path = dir.join(TOKEN_FILE);
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| Error::Crypto(e.to_string()))?;
    let token = data_encoding::HEXLOWER.encode(&bytes);
    std::fs::create_dir_all(dir)?;
    write_private(&path, token.as_bytes())?;
    Ok(token)
}

/// Replaces the token (e.g. "Reset API token" in settings).
pub fn reset_token(dir: &Path) -> Result<String> {
    let _ = std::fs::remove_file(dir.join(TOKEN_FILE));
    load_or_create_token(dir)
}

fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(data)?;
    }
    #[cfg(not(unix))]
    {
        // %APPDATA% is already private to the user on Windows.
        std::fs::write(path, data)?;
    }
    Ok(())
}

/// Starts the API on 127.0.0.1 (`port` 0 = any free port; the default port falls back to
/// a free one if it is taken).
pub async fn start(node: Node, dir: &Path, port: u16, runner: Option<LocalRunner>) -> Result<ApiServer> {
    let token = load_or_create_token(dir)?;
    let listener = match tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(l) => l,
        Err(_) if port != 0 => tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?,
        Err(e) => return Err(e.into()),
    };
    let addr = listener.local_addr()?;
    std::fs::write(dir.join(PORT_FILE), addr.port().to_string())?;
    let ctx = Ctx { node, token: Arc::new(token), runner };
    let app = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/devices", get(devices))
        .route("/v1/send", post(send))
        .route("/v1/ring", post(ring))
        .route("/v1/clip", get(clip_get).post(clip_set))
        .route("/v1/commands", get(commands))
        .route("/v1/run", post(run))
        .route("/v1/input", post(input))
        .route("/v1/events", get(events))
        .layer(middleware::from_fn_with_state(ctx.clone(), guard))
        .with_state(ctx);
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::warn!(error = %e, "local API stopped");
        }
    });
    Ok(ApiServer { addr, token_path: dir.join(TOKEN_FILE), task })
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// Loopback Host only, and a valid bearer token (or `?token=` on the WebSocket).
async fn guard(State(ctx): State<Ctx>, Query(q): Query<TokenQuery>, req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if !matches!(host_name, "127.0.0.1" | "localhost" | "[::1]") {
        return err(StatusCode::FORBIDDEN, "only local requests are allowed");
    }
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::to_string);
    let ws_token = if req.uri().path() == "/v1/events" { q.token } else { None };
    let given = bearer.or(ws_token).unwrap_or_default();
    if !constant_time_eq(given.as_bytes(), ctx.token.as_bytes()) {
        return err(StatusCode::UNAUTHORIZED, "missing or wrong API token");
    }
    next.run(req).await
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

fn fail(e: Error) -> Response {
    let code = match e {
        Error::InvalidInput(_) | Error::UnknownPeer(_) => StatusCode::BAD_REQUEST,
        Error::Paused => StatusCode::CONFLICT,
        _ => StatusCode::BAD_GATEWAY,
    };
    err(code, &e.to_string())
}

/// Accepts a device id or a (case-insensitive) device name. The error is the response to
/// return as-is (handlers return it straight away, so its size doesn't matter).
#[allow(clippy::result_large_err)]
fn resolve(node: &Node, device: &str) -> std::result::Result<DeviceId, Response> {
    if let Ok(id) = device.parse::<DeviceId>() {
        return Ok(id);
    }
    let matches: Vec<_> = node.peers().into_iter().filter(|p| p.info.name.eq_ignore_ascii_case(device)).collect();
    match matches.as_slice() {
        [one] => Ok(one.info.id),
        [] => Err(err(StatusCode::NOT_FOUND, &format!("no paired device called \"{device}\""))),
        _ => Err(err(StatusCode::BAD_REQUEST, "several devices have that name; use its id")),
    }
}

async fn status(State(ctx): State<Ctx>) -> Response {
    Json(ctx.node.local()).into_response()
}

async fn devices(State(ctx): State<Ctx>) -> Response {
    Json(ctx.node.peers()).into_response()
}

#[derive(Deserialize)]
struct SendBody {
    device: String,
    paths: Vec<String>,
}

async fn send(State(ctx): State<Ctx>, Json(b): Json<SendBody>) -> Response {
    let id = match resolve(&ctx.node, &b.device) {
        Ok(id) => id,
        Err(r) => return r,
    };
    let mut ids = Vec::new();
    for p in &b.paths {
        match ctx.node.send_file(&id, Path::new(p)) {
            Ok(t) => ids.push(t),
            Err(e) => return fail(e),
        }
    }
    Json(json!({ "transfers": ids })).into_response()
}

#[derive(Deserialize)]
struct RingBody {
    device: String,
    on: Option<bool>,
}

async fn ring(State(ctx): State<Ctx>, Json(b): Json<RingBody>) -> Response {
    let id = match resolve(&ctx.node, &b.device) {
        Ok(id) => id,
        Err(r) => return r,
    };
    match ctx.node.ring(&id, b.on.unwrap_or(true)) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => fail(e),
    }
}

async fn clip_get(State(ctx): State<Ctx>) -> Response {
    match ctx.node.clipboard_history() {
        Ok(h) => Json(h.into_iter().next().map(|e| json!(e)).unwrap_or(Value::Null)).into_response(),
        Err(e) => fail(e),
    }
}

#[derive(Deserialize)]
struct ClipBody {
    text: String,
}

async fn clip_set(State(ctx): State<Ctx>, Json(b): Json<ClipBody>) -> Response {
    match ctx.node.send_clipboard(&b.text) {
        Ok(sent) => Json(json!({ "sent": sent })).into_response(),
        Err(e) => fail(e),
    }
}

async fn commands(State(ctx): State<Ctx>) -> Response {
    Json(ctx.node.catalog().commands).into_response()
}

#[derive(Deserialize)]
struct RunBody {
    command: String,
    device: Option<String>,
}

/// Runs a saved command (matched by id or name) and waits up to 60 s for its output.
async fn run(State(ctx): State<Ctx>, Json(b): Json<RunBody>) -> Response {
    match b.device {
        None => {
            let Some(runner) = ctx.runner.clone() else {
                return err(StatusCode::NOT_IMPLEMENTED, "this app can't run commands");
            };
            let cmd = ctx
                .node
                .catalog()
                .commands
                .into_iter()
                .find(|c| c.id == b.command || c.name.eq_ignore_ascii_case(&b.command));
            let Some(cmd) = cmd else { return err(StatusCode::NOT_FOUND, "no saved command with that name") };
            let (ok, output) = runner(cmd.id).await;
            Json(json!({ "ok": ok, "output": output })).into_response()
        }
        Some(device) => {
            let id = match resolve(&ctx.node, &device) {
                Ok(id) => id,
                Err(r) => return r,
            };
            let mut ev = ctx.node.subscribe();
            if let Err(e) = ctx.node.run_command(&id, &b.command) {
                return fail(e);
            }
            let wait = async {
                loop {
                    match ev.recv().await {
                        Ok(NodeEvent::RunFinished { from, command, ok, output })
                            if from == id && command == b.command =>
                        {
                            return Some((ok, output));
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => return None,
                    }
                }
            };
            match tokio::time::timeout(Duration::from_secs(60), wait).await {
                Ok(Some((ok, output))) => Json(json!({ "ok": ok, "output": output })).into_response(),
                _ => err(StatusCode::GATEWAY_TIMEOUT, "no answer from the device within 60 seconds"),
            }
        }
    }
}

#[derive(Deserialize)]
struct InputBody {
    device: String,
    events: Vec<InputEvent>,
}

async fn input(State(ctx): State<Ctx>, Json(b): Json<InputBody>) -> Response {
    let id = match resolve(&ctx.node, &b.device) {
        Ok(id) => id,
        Err(r) => return r,
    };
    match ctx.node.send_input(&id, b.events) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => fail(e),
    }
}

async fn events(State(ctx): State<Ctx>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    // Browsers always send Origin; scripts and the CLI don't. Pages may not subscribe.
    if headers.contains_key(header::ORIGIN) {
        return err(StatusCode::FORBIDDEN, "browser pages can't use the Devti API");
    }
    let node = ctx.node.clone();
    ws.on_upgrade(move |socket| pump(node, socket))
}

async fn pump(node: Node, mut socket: WebSocket) {
    let mut rx = node.subscribe();
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(e) => {
                    let text = serde_json::to_string(&e).unwrap_or_default();
                    if socket.send(Message::Text(text.into())).await.is_err() { break; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let _ = socket.send(Message::Text(r#"{"type":"resync"}"#.into())).await;
                }
                Err(_) => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
        }
    }
}
