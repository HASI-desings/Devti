//! # Devti Core
//!
//! The shared heart of Devti: device identity, the encrypted QUIC transport,
//! pairing, discovery and persistence. The desktop Hub links it directly; Android
//! and iOS reach it through the UniFFI bindings in `devti-ffi`.
//!
//! Start with [`node::Node`] — everything else is a building block it uses.

#![forbid(unsafe_code)]

pub mod discovery;
pub mod error;
pub mod identity;
pub mod model;
pub mod net;
pub mod node;
pub mod pairing;
pub mod protocol;
pub mod secret;
pub mod store;
pub mod tls;

pub mod accessory;
#[cfg(feature = "api")]
pub mod api;
pub use error::{Error, Result};
pub use node::{Node, NodeConfig, NodeEvent};

/// Version of this crate (shown in About screens).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
