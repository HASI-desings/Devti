//! The Devti wire protocol: framing plus message schemas.
//!
//! Every QUIC bi-directional stream starts with a [`messages::StreamOpen`] frame that
//! says what the stream is for; the rest of the stream carries messages of that kind.
//! See `docs/PROTOCOL.md` for the normative description.

pub mod frame;
pub mod messages;

pub use frame::{MAX_FRAME_HARD_LIMIT, decode, encode, read_message, write_message};
pub use messages::*;
