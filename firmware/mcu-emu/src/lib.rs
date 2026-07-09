//! MCU emulator: virtual mat + buttons → UART frames.

use protocol::mcu::{encode, ButtonPayload, MsgType, SensePayload};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McuWorld {
    /// Force on mat (newtons).
    pub force_n: f32,
    /// Distance to subject mm (None = invalid reading).
    pub tof_mm: Option<u16>,
    pub motion: bool,
}

impl Default for McuWorld {
    fn default() -> Self {
        Self {
            force_n: 0.0,
            tof_mm: Some(2000),
            motion: false,
        }
    }
}

impl McuWorld {
    pub fn dog_on_mat(weight_n: f32) -> Self {
        Self {
            force_n: weight_n,
            tof_mm: Some(350),
            motion: true,
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }
}

#[derive(Debug, Default)]
pub struct McuEmu {
    pub world: McuWorld,
    /// Pending button events to emit once.
    pending_buttons: Vec<ButtonPayload>,
    tick: u64,
}

impl McuEmu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_world(&mut self, world: McuWorld) {
        self.world = world;
    }

    pub fn press_pad(&mut self, pad: u8) {
        self.pending_buttons.push(ButtonPayload {
            pad,
            event: 1,
            hold_ms: 0,
        });
    }

    /// Produce UART bytes for one 50 ms tick (20 Hz sense).
    pub fn tick_50ms(&mut self) -> Vec<u8> {
        self.tick += 1;
        let mut out = Vec::new();
        let force_deci_n = (self.world.force_n * 10.0).clamp(0.0, 65535.0) as u16;
        let tof_mm = self.world.tof_mm.unwrap_or(0);
        let flags = if self.world.motion { 1 } else { 0 };
        let sense = SensePayload {
            force_deci_n,
            tof_mm,
            flags,
            pad: [0; 3],
        };
        out.extend(encode(MsgType::Sense, &sense.to_bytes()));
        for b in self.pending_buttons.drain(..) {
            out.extend(encode(MsgType::Button, &b.to_bytes()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::mcu::FrameDecoder;

    #[test]
    fn emits_decodable_sense() {
        let mut emu = McuEmu::new();
        emu.set_world(McuWorld::dog_on_mat(150.0));
        let bytes = emu.tick_50ms();
        let mut dec = FrameDecoder::new();
        let frames = dec.push(&bytes);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].msg, MsgType::Sense);
    }
}
