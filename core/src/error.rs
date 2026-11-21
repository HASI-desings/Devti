//! Error types shared by every Devti Core module.

use thiserror::Error;

/// Result alias used throughout the core.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong inside Devti Core.
///
/// Variants are coarse on purpose: the UI maps them to human messages, while the
/// detailed cause travels in the string payload for diagnostics.
#[derive(Debug, Error)]
pub enum Error {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("secret store error: {0}")]
    SecretStore(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("tls error: {0}")]
    Tls(String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("frame of {size} bytes exceeds the {limit} byte limit")]
    FrameTooLarge { size: usize, limit: usize },

    #[error("connection error: {0}")]
    Connection(String),

    #[error("invalid pairing link: {0}")]
    InvalidPairingUri(String),

    #[error("pairing failed: {0}")]
    Pairing(String),

    #[error("device {0} is not paired with this device")]
    UnknownPeer(String),

    #[error("the ecosystem is paused")]
    Paused,

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("timed out: {0}")]
    Timeout(String),

    #[error("discovery error: {0}")]
    Discovery(String),

    #[error("node is shutting down")]
    Shutdown,
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Storage(e.to_string())
    }
}

impl From<rustls::Error> for Error {
    fn from(e: rustls::Error) -> Self {
        Error::Tls(e.to_string())
    }
}

impl From<quinn::ConnectionError> for Error {
    fn from(e: quinn::ConnectionError) -> Self {
        Error::Connection(e.to_string())
    }
}

impl From<quinn::ConnectError> for Error {
    fn from(e: quinn::ConnectError) -> Self {
        Error::Connection(e.to_string())
    }
}

impl From<quinn::WriteError> for Error {
    fn from(e: quinn::WriteError) -> Self {
        Error::Connection(e.to_string())
    }
}

impl From<quinn::ReadExactError> for Error {
    fn from(e: quinn::ReadExactError) -> Self {
        Error::Connection(e.to_string())
    }
}

impl From<quinn::ClosedStream> for Error {
    fn from(e: quinn::ClosedStream) -> Self {
        Error::Connection(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Storage(format!("json: {e}"))
    }
}
