//! Phase 3 remote control on the control channel: input, system actions, app launch,
//! saved commands, the shortcuts deck and catalog exchange.
//!
//! Devti Core never touches the operating system itself. It checks the permission the
//! local user granted the sender, writes the activity log, and hands the request to
//! the host app as an event (`RemoteInput`, `SystemRequested`, …), which performs it
//! with the platform's input and power APIs.
//!
//! Permissions:
//! * `control`  — input events, volume, brightness, asking for the catalog.
//! * `commands` — power actions, launching apps, saved commands, deck buttons.

use std::sync::Arc;

use parking_lot::RwLock;

use super::{Inner, NodeEvent};
use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::protocol::{Catalog, ControlMsg, InputEvent, SystemAction};

/// Most input events accepted in one message (a remote batches touchpad motion).
const MAX_INPUT_BATCH: usize = 64;

pub(crate) struct State {
    /// What this device offers to remotes (set by the host app).
    catalog: RwLock<Catalog>,
}

impl State {
    pub fn new() -> Self {
        Self { catalog: RwLock::new(Catalog::default()) }
    }
}

/// Dispatches a control message from `peer`.
pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let Some(perms) = inner.permissions_of(&peer) else { return };
    let name = inner.peer_name(&peer);
    match msg {
        ControlMsg::Input { events } => {
            if !perms.control {
                return;
            }
            let events: Vec<InputEvent> =
                events.into_iter().take(MAX_INPUT_BATCH).filter_map(InputEvent::sanitized).collect();
            if !events.is_empty() {
                inner.emit(NodeEvent::RemoteInput { from: peer, events });
            }
        }
        ControlMsg::System { action } => {
            let allowed = if action.is_power() { perms.commands } else { perms.control };
            if !allowed || action == SystemAction::Unknown {
                return;
            }
            if action.is_power() {
                inner.log(Some((&peer, &name)), "system_action", &format!("{action:?}").to_lowercase());
            }
            inner.emit(NodeEvent::SystemRequested { from: peer, from_name: name, action });
        }
        ControlMsg::Launch { app } => {
            if perms.commands && inner.control.catalog.read().apps.iter().any(|a| a.id == app) {
                inner.log(Some((&peer, &name)), "app_launched", &app);
                inner.emit(NodeEvent::LaunchRequested { from: peer, from_name: name, app });
            }
        }
        ControlMsg::Run { command } => {
            if !perms.commands {
                let _ = inner.send_to(
                    &peer,
                    ControlMsg::RunResult { command, ok: false, output: "Not allowed on this device.".into() },
                );
                return;
            }
            let known = inner.control.catalog.read().commands.iter().find(|c| c.id == command).map(|c| c.name.clone());
            match known {
                Some(label) => {
                    inner.log(Some((&peer, &name)), "command_run", &label);
                    inner.emit(NodeEvent::RunRequested { from: peer, from_name: name, command });
                }
                None => {
                    let _ = inner.send_to(
                        &peer,
                        ControlMsg::RunResult { command, ok: false, output: "That command doesn't exist here.".into() },
                    );
                }
            }
        }
        ControlMsg::RunResult { command, ok, output } => {
            let output: String = output.chars().take(4000).collect();
            inner.emit(NodeEvent::RunFinished { from: peer, command, ok, output });
        }
        ControlMsg::DeckPress { button } => {
            let exists = inner.control.catalog.read().deck.iter().flat_map(|p| &p.buttons).any(|b| b.id == button);
            if perms.commands && exists {
                inner.emit(NodeEvent::DeckPressed { from: peer, from_name: name, button });
            }
        }
        ControlMsg::CatalogRequest => {
            if !(perms.control || perms.commands) {
                return;
            }
            let mut catalog = inner.control.catalog.read().clone();
            if !perms.commands {
                // A control-only remote gets the touchpad, keys, volume and brightness.
                catalog.apps.clear();
                catalog.commands.clear();
                catalog.deck.clear();
                catalog.system.retain(|s| s == "set_volume" || s == "set_brightness");
            }
            let _ = inner.send_to(&peer, ControlMsg::Catalog { catalog });
        }
        ControlMsg::Catalog { catalog } => {
            inner.emit(NodeEvent::CatalogReceived { from: peer, catalog: catalog.sanitized() });
        }
        _ => {}
    }
}

impl super::Node {
    /// Sends input to `peer` (needs `control` there).
    pub fn send_input(&self, peer: &DeviceId, events: Vec<InputEvent>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        if events.len() > MAX_INPUT_BATCH {
            return Err(Error::InvalidInput("too many input events at once".into()));
        }
        self.inner.send_to(peer, ControlMsg::Input { events })
    }

    /// Asks `peer` to lock, sleep, change volume, …
    pub fn system_action(&self, peer: &DeviceId, action: SystemAction) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::System { action })
    }

    /// Opens an app from `peer`'s catalog.
    pub fn launch_app(&self, peer: &DeviceId, app: &str) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::Launch { app: app.to_string() })
    }

    /// Runs one of `peer`'s saved commands; the outcome arrives as `RunFinished`.
    pub fn run_command(&self, peer: &DeviceId, command: &str) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::Run { command: command.to_string() })
    }

    /// Host reports how a `RunRequested` went; it is sent back to the requester.
    pub fn report_run(&self, peer: &DeviceId, command: &str, ok: bool, output: &str) -> Result<()> {
        let output: String = output.chars().take(4000).collect();
        self.inner.send_to(peer, ControlMsg::RunResult { command: command.to_string(), ok, output })
    }

    /// Presses a deck button defined on `peer`.
    pub fn deck_press(&self, peer: &DeviceId, button: &str) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::DeckPress { button: button.to_string() })
    }

    /// Asks `peer` for its catalog; it arrives as `CatalogReceived`.
    pub fn request_catalog(&self, peer: &DeviceId) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::CatalogRequest)
    }

    /// Sets what this device offers to remotes (apps, commands, deck, system actions).
    pub fn set_catalog(&self, catalog: Catalog) {
        *self.inner.control.catalog.write() = catalog.sanitized();
    }

    pub fn catalog(&self) -> Catalog {
        self.inner.control.catalog.read().clone()
    }
}
