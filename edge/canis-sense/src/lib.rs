//! Sense pipeline: MCU frames → PresenceFilter.

use presence::{PresenceFilter, SenseSample};
use protocol::mcu::{FrameDecoder, MsgType, SensePayload};
use protocol::ForceBand;

#[derive(Debug, Clone)]
pub struct SenseSnapshot {
    pub present: bool,
    pub confidence: f32,
    pub force_n: f32,
    pub force_band: ForceBand,
    pub tof_mm: Option<u16>,
    pub flipped: bool,
}

#[derive(Debug)]
pub struct SensePipeline {
    decoder: FrameDecoder,
    filter: PresenceFilter,
    t_ms: u64,
}

impl Default for SensePipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl SensePipeline {
    pub fn new() -> Self {
        Self {
            decoder: FrameDecoder::new(),
            filter: PresenceFilter::default(),
            t_ms: 0,
        }
    }

    pub fn filter(&self) -> &PresenceFilter {
        &self.filter
    }

    /// Advance local clock and inject a direct sample (for emulators that skip UART).
    pub fn push_sample(
        &mut self,
        force_n: f32,
        tof_mm: Option<u16>,
        motion: bool,
        dt_ms: u64,
    ) -> SenseSnapshot {
        self.t_ms = self.t_ms.saturating_add(dt_ms);
        let flipped = self.filter.update(SenseSample {
            force_n,
            tof_mm,
            motion,
            t_ms: self.t_ms,
        });
        self.snapshot(flipped)
    }

    pub fn push_bytes(&mut self, data: &[u8], dt_ms: u64) -> Vec<SenseSnapshot> {
        self.t_ms = self.t_ms.saturating_add(dt_ms);
        let frames = self.decoder.push(data);
        let mut out = Vec::new();
        for fr in frames {
            if fr.msg != MsgType::Sense {
                continue;
            }
            let Some(p) = SensePayload::from_bytes(&fr.payload) else {
                continue;
            };
            let tof = if p.tof_mm == 0 { None } else { Some(p.tof_mm) };
            let flipped = self.filter.update(SenseSample {
                force_n: p.force_n(),
                tof_mm: tof,
                motion: p.motion(),
                t_ms: self.t_ms,
            });
            out.push(self.snapshot(flipped));
        }
        out
    }

    fn snapshot(&self, flipped: bool) -> SenseSnapshot {
        SenseSnapshot {
            present: self.filter.present(),
            confidence: self.filter.confidence(),
            force_n: self.filter.last_force_n(),
            force_band: self.filter.force_band(),
            tof_mm: self.filter.last_tof_mm(),
            flipped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::mcu::{encode, MsgType, SensePayload};

    #[test]
    fn uart_path_detects_presence() {
        let mut pipe = SensePipeline::new();
        let payload = SensePayload {
            force_deci_n: 1000, // 100 N
            tof_mm: 400,
            flags: 0,
            pad: [0; 3],
        };
        let frame = encode(MsgType::Sense, &payload.to_bytes());
        // feed over enter window
        for _ in 0..10 {
            let snaps = pipe.push_bytes(&frame, 100);
            assert!(!snaps.is_empty());
        }
        assert!(pipe.filter().present());
    }
}
