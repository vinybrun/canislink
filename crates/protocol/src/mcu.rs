//! MCU ↔ SBC UART frame protocol (v1).
//!
//! Frame layout (little-endian):
//! ```text
//! [0]=0xCA magic0  [1]=0x15 magic1
//! [2]=msg_type     [3]=payload_len
//! [4..4+len]=payload
//! [last]=xor checksum of bytes [2..last)
//! ```

use serde::{Deserialize, Serialize};

pub const MAGIC0: u8 = 0xCA;
pub const MAGIC1: u8 = 0x15;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgType {
    /// Periodic sensor sample from MCU → SBC.
    Sense = 0x01,
    /// Button event MCU → SBC.
    Button = 0x02,
    /// LED command SBC → MCU.
    Led = 0x10,
    /// Heartbeat / ping either direction.
    Ping = 0x7E,
    Pong = 0x7F,
}

impl MsgType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Self::Sense),
            0x02 => Some(Self::Button),
            0x10 => Some(Self::Led),
            0x7E => Some(Self::Ping),
            0x7F => Some(Self::Pong),
            _ => None,
        }
    }
}

/// 8-byte sense payload.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SensePayload {
    /// Force in 0.1 N units (u16).
    pub force_deci_n: u16,
    /// ToF distance mm (0 = invalid).
    pub tof_mm: u16,
    /// Bit0 motion recent.
    pub flags: u8,
    pub pad: [u8; 3],
}

impl SensePayload {
    pub fn force_n(self) -> f32 {
        self.force_deci_n as f32 / 10.0
    }

    pub fn motion(self) -> bool {
        self.flags & 1 != 0
    }

    pub fn to_bytes(self) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..2].copy_from_slice(&self.force_deci_n.to_le_bytes());
        b[2..4].copy_from_slice(&self.tof_mm.to_le_bytes());
        b[4] = self.flags;
        b[5..8].copy_from_slice(&self.pad);
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 8 {
            return None;
        }
        Some(Self {
            force_deci_n: u16::from_le_bytes([b[0], b[1]]),
            tof_mm: u16::from_le_bytes([b[2], b[3]]),
            flags: b[4],
            pad: [b[5], b[6], b[7]],
        })
    }
}

/// Button payload: pad index + event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonPayload {
    /// 0=Call, 1=Play, 2=Again, 3=Done
    pub pad: u8,
    /// 1=press, 2=release, 3=hold
    pub event: u8,
    pub hold_ms: u16,
}

impl ButtonPayload {
    pub fn to_bytes(self) -> [u8; 4] {
        let mut b = [0u8; 4];
        b[0] = self.pad;
        b[1] = self.event;
        b[2..4].copy_from_slice(&self.hold_ms.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 4 {
            return None;
        }
        Some(Self {
            pad: b[0],
            event: b[1],
            hold_ms: u16::from_le_bytes([b[2], b[3]]),
        })
    }
}

pub fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a ^ b)
}

/// Encode a frame.
pub fn encode(msg: MsgType, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() <= 255);
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(msg as u8);
    out.push(payload.len() as u8);
    out.extend_from_slice(payload);
    let csum = checksum(&out[2..]);
    out.push(csum);
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub msg: MsgType,
    pub payload: Vec<u8>,
}

/// Streaming frame decoder.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(data);
        let mut frames = Vec::new();
        while let Some(f) = self.try_pop() {
            frames.push(f);
        }
        frames
    }

    fn try_pop(&mut self) -> Option<Frame> {
        // find magic
        while self.buf.len() >= 2 {
            if self.buf[0] == MAGIC0 && self.buf[1] == MAGIC1 {
                break;
            }
            self.buf.remove(0);
        }
        if self.buf.len() < 5 {
            return None;
        }
        let plen = self.buf[3] as usize;
        let total = 5 + plen;
        if self.buf.len() < total {
            return None;
        }
        let csum = self.buf[4 + plen];
        let expect = checksum(&self.buf[2..4 + plen]);
        if csum != expect {
            // resync
            self.buf.remove(0);
            return None;
        }
        let msg = MsgType::from_u8(self.buf[2])?;
        let payload = self.buf[4..4 + plen].to_vec();
        self.buf.drain(..total);
        Some(Frame { msg, payload })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sense() {
        let p = SensePayload {
            force_deci_n: 1234,
            tof_mm: 450,
            flags: 1,
            pad: [0; 3],
        };
        let frame = encode(MsgType::Sense, &p.to_bytes());
        let mut dec = FrameDecoder::new();
        let frames = dec.push(&frame);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].msg, MsgType::Sense);
        let p2 = SensePayload::from_bytes(&frames[0].payload).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn resync_after_garbage() {
        let p = SensePayload {
            force_deci_n: 100,
            tof_mm: 0,
            flags: 0,
            pad: [0; 3],
        };
        let mut stream = vec![0x00, 0xFF, 0xCA];
        stream.extend(encode(MsgType::Sense, &p.to_bytes()));
        let mut dec = FrameDecoder::new();
        let frames = dec.push(&stream);
        assert_eq!(frames.len(), 1);
    }
}
