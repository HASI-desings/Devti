//! Length-prefixed CBOR framing.
//!
//! ```text
//! +----------------------+---------------------------+
//! | length: u32 (BE)     | CBOR-encoded message      |
//! +----------------------+---------------------------+
//! ```
//!
//! The reader checks the length against a per-stream limit *before* allocating, so a
//! hostile peer can't make us reserve gigabytes with a 4-byte header.

use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};

/// Absolute ceiling for any frame, whatever the stream type.
pub const MAX_FRAME_HARD_LIMIT: usize = 16 * 1024 * 1024;
/// Maximum CBOR nesting depth accepted from the network.
const RECURSION_LIMIT: usize = 64;

/// Serialises `msg` into a complete frame (header + body).
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    let mut body = Vec::with_capacity(128);
    ciborium::into_writer(msg, &mut body).map_err(|e| Error::Protocol(format!("encode: {e}")))?;
    if body.len() > MAX_FRAME_HARD_LIMIT {
        return Err(Error::FrameTooLarge { size: body.len(), limit: MAX_FRAME_HARD_LIMIT });
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Parses a frame *body* (without the length header).
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    ciborium::de::from_reader_with_recursion_limit(body, RECURSION_LIMIT)
        .map_err(|e| Error::Protocol(format!("decode: {e}")))
}

/// Writes one message to a stream.
pub async fn write_message<W, T>(w: &mut W, msg: &T) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let frame = encode(msg)?;
    w.write_all(&frame).await?;
    Ok(())
}

/// Reads one message. Returns `Ok(None)` on a clean end-of-stream at a frame boundary.
pub async fn read_message<R, T>(r: &mut R, limit: usize) -> Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let limit = limit.min(MAX_FRAME_HARD_LIMIT);
    let mut header = [0u8; 4];
    match r.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(header) as usize;
    if len > limit {
        return Err(Error::FrameTooLarge { size: len, limit });
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    decode(&body).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        a: u32,
        b: String,
    }

    #[tokio::test]
    async fn roundtrip_over_a_pipe() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let msg = Sample { a: 7, b: "hi".into() };
        write_message(&mut a, &msg).await.unwrap();
        write_message(&mut a, &msg).await.unwrap();
        drop(a);
        let got: Sample = read_message(&mut b, 1024).await.unwrap().unwrap();
        assert_eq!(got, msg);
        let got: Sample = read_message(&mut b, 1024).await.unwrap().unwrap();
        assert_eq!(got, msg);
        let end: Option<Sample> = read_message(&mut b, 1024).await.unwrap();
        assert!(end.is_none());
    }

    #[tokio::test]
    async fn oversized_frames_are_rejected_before_allocation() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        let err = read_message::<_, Sample>(&mut b, 1024).await.unwrap_err();
        assert!(matches!(err, Error::FrameTooLarge { limit: 1024, .. }));
    }

    #[tokio::test]
    async fn truncated_body_is_an_error() {
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&10u32.to_be_bytes()).await.unwrap();
        a.write_all(&[0xa0, 0x01]).await.unwrap();
        drop(a);
        assert!(read_message::<_, Sample>(&mut b, 1024).await.is_err());
    }

    #[test]
    fn garbage_does_not_decode() {
        assert!(decode::<Sample>(&[0xff, 0x00, 0x13]).is_err());
        assert!(decode::<Sample>(&[]).is_err());
    }

    #[test]
    fn deeply_nested_input_is_rejected() {
        // 200 nested CBOR arrays of length 1.
        let mut body = vec![0x81u8; 200];
        body.push(0x00);
        assert!(decode::<ciborium::Value>(&body).is_err());
    }

    #[test]
    fn encode_has_correct_header() {
        let f = encode(&Sample { a: 1, b: String::new() }).unwrap();
        let len = u32::from_be_bytes([f[0], f[1], f[2], f[3]]) as usize;
        assert_eq!(len, f.len() - 4);
        let back: Sample = decode(&f[4..]).unwrap();
        assert_eq!(back.a, 1);
    }
}
