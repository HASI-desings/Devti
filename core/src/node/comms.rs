//! Phase 6 communication on the control channel: SMS, calls and handoff.
//!
//! The phone owns the data (Android's SMS store and telephony). A computer asks for it, and the
//! phone's host app answers through this API. Permissions, checked on the receiving side:
//! * reading texts and seeing calls: `notifications` (they're the phone's conversations);
//! * sending a text, answering or declining: `commands` as well;
//! * handoff (open this link here): `clipboard` (it's shared content).

use std::sync::Arc;

use super::{Inner, NodeEvent};
use crate::error::{Error, Result};
use crate::model::DeviceId;
use crate::protocol::{CallInfo, ControlMsg, HandoffItem, SmsMessage, SmsThread};

const MAX_THREADS: usize = 200;
const MAX_MESSAGES: usize = 500;
const MAX_TEXT: usize = 10_000;

fn cap(s: &str, n: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(n).collect()
}

fn clean_message(mut m: SmsMessage) -> SmsMessage {
    m.id = cap(&m.id, 64);
    m.thread = cap(&m.thread, 64);
    m.address = cap(&m.address, 64);
    m.name = cap(&m.name, 80);
    m.body = cap(&m.body, MAX_TEXT);
    m
}

fn clean_call(mut c: CallInfo) -> CallInfo {
    c.id = cap(&c.id, 64);
    c.state = cap(&c.state, 16);
    c.number = cap(&c.number, 64);
    c.name = cap(&c.name, 80);
    c
}

pub(crate) fn handle(inner: &Arc<Inner>, peer: DeviceId, msg: ControlMsg) {
    let Some(perms) = inner.permissions_of(&peer) else { return };
    let name = inner.peer_name(&peer);
    match msg {
        // ---- asked of this phone
        ControlMsg::SmsThreadsRequest => {
            if perms.notifications {
                inner.emit(NodeEvent::SmsThreadsRequested { from: peer });
            }
        }
        ControlMsg::SmsMessagesRequest { thread } => {
            if perms.notifications {
                inner.emit(NodeEvent::SmsMessagesRequested { from: peer, thread: cap(&thread, 64) });
            }
        }
        ControlMsg::SmsSend { to, text } => {
            if !(perms.notifications && perms.commands) {
                let _ = inner.send_to(
                    &peer,
                    ControlMsg::SmsResult { ok: false, error: Some("Not allowed on this phone.".into()) },
                );
                return;
            }
            let to: Vec<String> = to.iter().take(20).map(|t| cap(t, 64)).filter(|t| !t.is_empty()).collect();
            if to.is_empty() || text.trim().is_empty() || text.len() > MAX_TEXT {
                let _ =
                    inner.send_to(&peer, ControlMsg::SmsResult { ok: false, error: Some("Nothing to send.".into()) });
                return;
            }
            inner.log(Some((&peer, &name)), "sms_send", &to.join(", "));
            inner.emit(NodeEvent::SmsSendRequested { from: peer, to, text });
        }
        ControlMsg::CallAction { id, action } => {
            if perms.notifications
                && perms.commands
                && matches!(action.as_str(), "answer" | "decline" | "end" | "mute" | "unmute")
            {
                inner.log(Some((&peer, &name)), "call_action", &action);
                inner.emit(NodeEvent::CallActionRequested { from: peer, id: cap(&id, 64), action });
            }
        }
        // ---- told by a phone
        ControlMsg::SmsThreads { threads } => {
            if perms.notifications {
                let threads = threads
                    .into_iter()
                    .take(MAX_THREADS)
                    .map(|mut t| {
                        t.id = cap(&t.id, 64);
                        t.snippet = cap(&t.snippet, 200);
                        t.addresses = t.addresses.iter().take(20).map(|a| cap(a, 64)).collect();
                        t.names = t.names.iter().take(20).map(|a| cap(a, 80)).collect();
                        t
                    })
                    .collect();
                inner.emit(NodeEvent::SmsThreads { from: peer, threads });
            }
        }
        ControlMsg::SmsMessages { thread, messages } => {
            if perms.notifications {
                let messages = messages.into_iter().take(MAX_MESSAGES).map(clean_message).collect();
                inner.emit(NodeEvent::SmsMessages { from: peer, thread: cap(&thread, 64), messages });
            }
        }
        ControlMsg::Sms { message } => {
            if perms.notifications {
                inner.emit(NodeEvent::Sms { from: peer, from_name: name, message: clean_message(message) });
            }
        }
        ControlMsg::SmsResult { ok, error } => {
            inner.emit(NodeEvent::SmsResult { from: peer, ok, error: error.map(|e| cap(&e, 500)) });
        }
        ControlMsg::Call { call } => {
            if perms.notifications {
                inner.emit(NodeEvent::Call { from: peer, from_name: name, call: clean_call(call) });
            }
        }
        ControlMsg::Handoff { item } if perms.clipboard && item.is_safe() => {
            inner.log(Some((&peer, &name)), "handoff", &item.kind);
            inner.emit(NodeEvent::Handoff { from: peer, from_name: name, item });
        }
        _ => {}
    }
}

impl super::Node {
    // ---- computer → phone
    pub fn request_sms_threads(&self, phone: &DeviceId) -> Result<()> {
        self.inner.send_to(phone, ControlMsg::SmsThreadsRequest)
    }

    pub fn request_sms_messages(&self, phone: &DeviceId, thread: &str) -> Result<()> {
        self.inner.send_to(phone, ControlMsg::SmsMessagesRequest { thread: thread.to_string() })
    }

    /// Asks the phone to send a text; the outcome arrives as `SmsResult`.
    pub fn send_sms(&self, phone: &DeviceId, to: Vec<String>, text: &str) -> Result<()> {
        if text.trim().is_empty() || text.len() > MAX_TEXT {
            return Err(Error::InvalidInput("the message is empty or too long".into()));
        }
        self.inner.send_to(phone, ControlMsg::SmsSend { to, text: text.to_string() })
    }

    pub fn call_action(&self, phone: &DeviceId, id: &str, action: &str) -> Result<()> {
        self.inner.send_to(phone, ControlMsg::CallAction { id: id.to_string(), action: action.to_string() })
    }

    // ---- phone → computers
    pub fn answer_sms_threads(&self, to: &DeviceId, threads: Vec<SmsThread>) -> Result<()> {
        self.inner.send_to(to, ControlMsg::SmsThreads { threads })
    }

    pub fn answer_sms_messages(&self, to: &DeviceId, thread: &str, messages: Vec<SmsMessage>) -> Result<()> {
        self.inner.send_to(to, ControlMsg::SmsMessages { thread: thread.to_string(), messages })
    }

    pub fn report_sms_result(&self, to: &DeviceId, ok: bool, error: Option<String>) -> Result<()> {
        self.inner.send_to(to, ControlMsg::SmsResult { ok, error })
    }

    /// A text arrived or was sent on this phone: tell every connected computer.
    pub fn publish_sms(&self, message: SmsMessage) {
        self.inner.broadcast(ControlMsg::Sms { message });
    }

    /// The call state changed on this phone.
    pub fn publish_call(&self, call: CallInfo) {
        self.inner.broadcast(ControlMsg::Call { call });
    }

    // ---- any direction
    /// Continue a link, media or text on another device.
    pub fn handoff(&self, to: &DeviceId, item: HandoffItem) -> Result<()> {
        if !item.is_safe() {
            return Err(Error::InvalidInput("only web links and text can be handed off".into()));
        }
        self.inner.send_to(to, ControlMsg::Handoff { item })
    }
}
