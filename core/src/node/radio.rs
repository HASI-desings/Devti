//! A computer switching a phone's radios, and asking for its microphone or sound (1.1).
//!
//! * `Radio { radio, on }` needs `commands` on the phone. The phone switches it itself
//!   where Android lets it (with Shizuku, or as device owner for Wi‑Fi), or opens the
//!   right Settings page for the person holding it, and answers `RadioResult`. Its
//!   radios' current state travels in its status (`DeviceStatus::radios`).
//! * `AudioRequest { kind, on }` needs `bluetooth` (sound and accessories) on the phone,
//!   which still asks its user, as Android requires for the microphone and for sound.

use std::sync::Arc;

use super::{Inner, Node, NodeEvent};
use crate::error::{Error, Result};
use crate::model::{DeviceId, RADIOS};
use crate::protocol::ControlMsg;

pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let Some(perms) = inner.permissions_of(&peer) else { return };
    let name = inner.peer_name(&peer);
    match msg {
        ControlMsg::Radio { radio, on } => {
            if !perms.commands || !RADIOS.contains(&radio.as_str()) {
                return;
            }
            inner.log(Some((&peer, &name)), "radio_requested", &format!("{radio} {}", if on { "on" } else { "off" }));
            inner.emit(NodeEvent::RadioRequested { from: peer, from_name: name, radio, on });
        }
        ControlMsg::RadioResult { radio, ok, message } => {
            let message: String = message.chars().take(300).collect();
            inner.emit(NodeEvent::RadioResult { from: peer, radio, ok, message });
        }
        ControlMsg::AudioRequest { kind, on } => {
            if !perms.bluetooth || !(kind == "mic" || kind == "relay") {
                return;
            }
            inner.log(Some((&peer, &name)), if on { "audio_requested" } else { "audio_request_stopped" }, &kind);
            inner.emit(NodeEvent::AudioRequested { from: peer, from_name: name, kind, on });
        }
        ControlMsg::MediaActivity { music, voice, app } => {
            let app: String = app.chars().take(64).collect();
            inner.emit(NodeEvent::MediaActivity { from: peer, music, voice, app });
        }
        ControlMsg::MediaCommand { action } => {
            if !perms.control || !(action == "pause" || action == "play") {
                return;
            }
            inner.emit(NodeEvent::MediaCommandRequested { from: peer, action });
        }
        _ => {}
    }
}

impl Node {
    /// Tells every connected device what this one is playing (music, voice message).
    pub fn report_media_activity(&self, music: bool, voice: bool, app: &str) {
        self.inner.broadcast(ControlMsg::MediaActivity { music, voice, app: app.into() });
    }

    /// Asks a device to pause or resume its music ("pause" or "play").
    pub fn media_command(&self, peer: &DeviceId, action: &str) -> Result<()> {
        if action != "pause" && action != "play" {
            return Err(Error::InvalidInput("“pause” or “play”".into()));
        }
        self.inner.send_to(peer, ControlMsg::MediaCommand { action: action.into() })
    }

    /// Asks a phone to switch one of its radios ("wifi", "data", "bluetooth", "hotspot",
    /// "airplane"). The answer arrives as `RadioResult`.
    pub fn set_radio(&self, peer: &DeviceId, radio: &str, on: bool) -> Result<()> {
        if !RADIOS.contains(&radio) {
            return Err(Error::InvalidInput(format!("unknown radio “{radio}”")));
        }
        self.inner.send_to(peer, ControlMsg::Radio { radio: radio.into(), on })
    }

    /// Phone side: how switching a radio went.
    pub fn report_radio(&self, peer: &DeviceId, radio: &str, ok: bool, message: &str) -> Result<()> {
        self.inner.send_to(peer, ControlMsg::RadioResult { radio: radio.into(), ok, message: message.into() })
    }

    /// Asks a phone to start (or stop) sending its microphone ("mic") or sound ("relay").
    pub fn request_audio(&self, peer: &DeviceId, kind: &str, on: bool) -> Result<()> {
        if kind != "mic" && kind != "relay" {
            return Err(Error::InvalidInput("audio is “mic” or “relay”".into()));
        }
        self.inner.send_to(peer, ControlMsg::AudioRequest { kind: kind.into(), on })
    }
}
