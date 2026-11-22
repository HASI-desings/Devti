//! Byte-level pieces of the profile library: hex templates, wildcard patterns, vendor
//! framings and Apple's battery records.

use std::collections::BTreeMap;

use crc::{CRC_16_MODBUS, CRC_16_XMODEM, Crc};

use super::{BatteryLevel, Framing};

const XMODEM: Crc<u16> = Crc::<u16>::new(&CRC_16_XMODEM);
const MODBUS: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);
/// Accessory messages are small; a bigger length means a corrupted header.
const MAX_FRAME: usize = 256;

/// One template token: a fixed byte or a placeholder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    Byte(u8),
    /// `{v}` — the (first) value as one byte (`u8`, or `i8` two's complement).
    Value,
    /// `{bands}` — every value, one byte each.
    All,
}

/// Parses "04 00 {v} 00" into tokens.
pub fn parse_template(t: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    for part in t.split_whitespace() {
        match part {
            "{v}" | "{v:u8}" | "{v:i8}" => out.push(Tok::Value),
            "{bands}" => out.push(Tok::All),
            hex => out.push(Tok::Byte(u8::from_str_radix(hex, 16).map_err(|_| format!("bad byte {hex:?}"))?)),
        }
    }
    if out.is_empty() {
        return Err("empty payload".into());
    }
    Ok(out)
}

/// Fills a template with values (each written as one byte; negatives as i8).
pub fn fill(template: &str, values: &[i32]) -> Result<Vec<u8>, String> {
    let toks = parse_template(template)?;
    let mut out = Vec::with_capacity(toks.len() + values.len());
    for t in toks {
        match t {
            Tok::Byte(b) => out.push(b),
            Tok::Value => out.push(*values.first().ok_or("missing value")? as i8 as u8),
            Tok::All => out.extend(values.iter().map(|v| *v as i8 as u8)),
        }
    }
    Ok(out)
}

/// Parses "04 00 ?? 0D" — `None` is a wildcard.
pub fn parse_pattern(p: &str) -> Result<Vec<Option<u8>>, String> {
    p.split_whitespace()
        .map(|s| {
            if s == "??" {
                Ok(None)
            } else {
                u8::from_str_radix(s, 16).map(Some).map_err(|_| format!("bad byte {s:?}"))
            }
        })
        .collect()
}

/// Does `payload` start with `pattern`?
pub fn matches(pattern: &[Option<u8>], payload: &[u8]) -> bool {
    payload.len() >= pattern.len() && pattern.iter().zip(payload).all(|(p, b)| p.is_none_or(|p| p == *b))
}

/// Apple AAP battery notification: `04 00 04 00 04 00 <n> { component 01 level status 01 } × n`.
/// Components: 01 single, 02 right, 04 left, 08 case. Status: 01 charging, 02 discharging,
/// 04 disconnected (skipped).
pub fn aap_battery(p: &[u8]) -> BTreeMap<String, BatteryLevel> {
    let mut out = BTreeMap::new();
    let Some(&n) = p.get(6) else { return out };
    for i in 0..n as usize {
        let base = 7 + i * 5;
        let (Some(&comp), Some(&level), Some(&status)) = (p.get(base), p.get(base + 2), p.get(base + 3)) else {
            break;
        };
        if status == 0x04 || level > 100 {
            continue;
        }
        let name = match comp {
            0x01 => "battery",
            0x02 => "right",
            0x04 => "left",
            0x08 => "case",
            _ => continue,
        };
        out.insert(name.to_string(), BatteryLevel { percent: level, charging: status == 0x01 });
    }
    out
}

/// Wraps payloads in a vendor framing and unwraps a byte stream back into payloads.
pub struct Framer {
    framing: Framing,
    seq: u8,
    buf: Vec<u8>,
}

impl Framer {
    pub fn new(framing: Framing) -> Self {
        Self { framing, seq: 0, buf: Vec::new() }
    }

    pub fn encode(&mut self, payload: &[u8]) -> Vec<u8> {
        match self.framing {
            Framing::Raw => payload.to_vec(),
            Framing::Sony => {
                // 0x0C = data; body = type, seq, len (u32 BE), payload; checksum = sum of body.
                let mut body = vec![0x0C, self.seq];
                self.seq ^= 1;
                body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
                body.extend_from_slice(payload);
                let sum = body.iter().fold(0u8, |a, b| a.wrapping_add(*b));
                body.push(sum);
                let mut out = vec![0x3E];
                for b in body {
                    if matches!(b, 0x3C..=0x3E) {
                        out.extend_from_slice(&[0x3D, b & 0xEF]);
                    } else {
                        out.push(b);
                    }
                }
                out.push(0x3C);
                out
            }
            Framing::Galaxy => {
                // payload = msg id + data; len counts id + data + crc.
                let len = (payload.len() + 2) as u16;
                let crc = XMODEM.checksum(payload);
                let mut out = vec![0xFD];
                out.extend_from_slice(&len.to_le_bytes());
                out.extend_from_slice(payload);
                out.extend_from_slice(&crc.to_le_bytes());
                out.push(0xDD);
                out
            }
            Framing::Huawei => {
                // 5A, len (u16 BE) = payload + 1, 00, payload, CRC-16/XMODEM (BE) over all of it.
                let mut out = vec![0x5A];
                out.extend_from_slice(&((payload.len() + 1) as u16).to_be_bytes());
                out.push(0x00);
                out.extend_from_slice(payload);
                let crc = XMODEM.checksum(&out);
                out.extend_from_slice(&crc.to_be_bytes());
                out
            }
            Framing::Nothing => {
                // 55 60 01, command (first two payload bytes, LE), len (u16 LE), 00, data, CRC-16/MODBUS (LE).
                let (cmd, data) = payload.split_at(payload.len().min(2));
                let mut out = vec![0x55, 0x60, 0x01];
                out.extend_from_slice(cmd);
                out.extend_from_slice(&(data.len() as u16).to_le_bytes());
                out.push(self.seq);
                self.seq = self.seq.wrapping_add(1);
                out.extend_from_slice(data);
                let crc = MODBUS.checksum(&out);
                out.extend_from_slice(&crc.to_le_bytes());
                out
            }
        }
    }

    /// Forgets partial input (call after a quiet period so a corrupted header can't stall).
    pub fn reset(&mut self) {
        self.buf.clear();
    }

    /// Feeds received bytes; returns complete payloads (bad checksums are dropped).
    pub fn decode(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        if self.framing == Framing::Raw {
            return vec![bytes.to_vec()];
        }
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > 64 * 1024 {
            self.buf.clear(); // garbage; resynchronise
        }
        let mut out = Vec::new();
        loop {
            let got = match self.framing {
                Framing::Sony => self.take_sony(),
                Framing::Galaxy => self.take_galaxy(),
                Framing::Huawei => self.take_huawei(),
                Framing::Nothing => self.take_nothing(),
                Framing::Raw => None,
            };
            match got {
                Some(Some(p)) => out.push(p),
                Some(None) => continue, // dropped a bad frame, keep scanning
                None => break,          // need more bytes
            }
        }
        out
    }

    fn resync(&mut self, start: u8) {
        match self.buf.iter().skip(1).position(|b| *b == start) {
            Some(i) => {
                self.buf.drain(..=i);
            }
            None => self.buf.clear(),
        }
    }

    fn take_sony(&mut self) -> Option<Option<Vec<u8>>> {
        let start = self.buf.iter().position(|b| *b == 0x3E)?;
        self.buf.drain(..start);
        let end = self.buf.iter().position(|b| *b == 0x3C)?;
        let raw: Vec<u8> = self.buf.drain(..=end).collect();
        let mut body = Vec::new();
        let mut esc = false;
        for &b in &raw[1..raw.len() - 1] {
            if esc {
                body.push(b | 0x10);
                esc = false;
            } else if b == 0x3D {
                esc = true;
            } else {
                body.push(b);
            }
        }
        if body.len() < 7 {
            return Some(None);
        }
        let (content, sum) = body.split_at(body.len() - 1);
        if content.iter().fold(0u8, |a, b| a.wrapping_add(*b)) != sum[0] {
            return Some(None);
        }
        let len = u32::from_be_bytes([content[2], content[3], content[4], content[5]]) as usize;
        if content.len() != 6 + len {
            return Some(None);
        }
        // Acks (type 0x01) carry no payload for profiles.
        if content[0] == 0x01 {
            return Some(None);
        }
        Some(Some(content[6..].to_vec()))
    }

    fn take_galaxy(&mut self) -> Option<Option<Vec<u8>>> {
        let start = self.buf.iter().position(|b| *b == 0xFD || *b == 0xFE)?;
        self.buf.drain(..start);
        if self.buf.len() < 3 {
            return None;
        }
        let len = (u16::from_le_bytes([self.buf[1], self.buf[2]]) & 0x03FF) as usize;
        let total = 1 + 2 + len + 1;
        if len < 3 {
            self.resync(0xFD);
            return Some(None);
        }
        if self.buf.len() < total {
            return None;
        }
        let frame = &self.buf[..total];
        let payload = &frame[3..3 + len - 2];
        let crc = u16::from_le_bytes([frame[3 + len - 2], frame[3 + len - 1]]);
        if frame[total - 1] != 0xDD || XMODEM.checksum(payload) != crc {
            self.resync(frame[0]);
            return Some(None);
        }
        let payload = payload.to_vec();
        self.buf.drain(..total);
        Some(Some(payload))
    }

    fn take_huawei(&mut self) -> Option<Option<Vec<u8>>> {
        let start = self.buf.iter().position(|b| *b == 0x5A)?;
        self.buf.drain(..start);
        if self.buf.len() < 4 {
            return None;
        }
        let len = u16::from_be_bytes([self.buf[1], self.buf[2]]) as usize;
        let total = 3 + len + 2;
        if len == 0 || len > MAX_FRAME {
            self.resync(0x5A);
            return Some(None);
        }
        if self.buf.len() < total {
            return None;
        }
        let frame = &self.buf[..total];
        let crc = u16::from_be_bytes([frame[total - 2], frame[total - 1]]);
        if XMODEM.checksum(&frame[..total - 2]) != crc {
            self.resync(0x5A);
            return Some(None);
        }
        let payload = frame[4..total - 2].to_vec();
        self.buf.drain(..total);
        Some(Some(payload))
    }

    fn take_nothing(&mut self) -> Option<Option<Vec<u8>>> {
        let start = self.buf.windows(2).position(|w| w == [0x55, 0x60])?;
        self.buf.drain(..start);
        if self.buf.len() < 8 {
            return None;
        }
        let len = u16::from_le_bytes([self.buf[5], self.buf[6]]) as usize;
        let total = 8 + len + 2;
        if len > MAX_FRAME {
            self.resync(0x55);
            return Some(None);
        }
        if self.buf.len() < total {
            return None;
        }
        let frame = &self.buf[..total];
        let crc = u16::from_le_bytes([frame[total - 2], frame[total - 1]]);
        if MODBUS.checksum(&frame[..total - 2]) != crc {
            self.resync(0x55);
            return Some(None);
        }
        let mut payload = frame[3..5].to_vec();
        payload.extend_from_slice(&frame[8..total - 2]);
        self.buf.drain(..total);
        Some(Some(payload))
    }
}
