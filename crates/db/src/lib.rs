//! In-memory stores for early production path.
//! Redis-backed presence lands later; this is correct w.r.t. TTL semantics.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use protocol::{DogId, ForceBand, PresenceReport, PresenceView, TerminalId, PRESENCE_TTL_MS};
use std::sync::Arc;

#[derive(Debug, Clone)]
struct PresenceEntry {
    terminal_id: TerminalId,
    present: bool,
    confidence: f32,
    force_band: ForceBand,
    last_seen: DateTime<Utc>,
    seq: u64,
}

#[derive(Debug, Default, Clone)]
pub struct PresenceStore {
    inner: Arc<DashMap<DogId, PresenceEntry>>,
}

impl PresenceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, report: PresenceReport) {
        use dashmap::mapref::entry::Entry;
        match self.inner.entry(report.dog_id) {
            Entry::Occupied(mut o) => {
                let e = o.get_mut();
                // Ignore stale seq
                if report.seq < e.seq {
                    return;
                }
                e.terminal_id = report.terminal_id;
                e.present = report.present;
                e.confidence = report.confidence;
                e.force_band = report.force_band;
                e.last_seen = report.ts;
                e.seq = report.seq;
            }
            Entry::Vacant(v) => {
                v.insert(PresenceEntry {
                    terminal_id: report.terminal_id,
                    present: report.present,
                    confidence: report.confidence,
                    force_band: report.force_band,
                    last_seen: report.ts,
                    seq: report.seq,
                });
            }
        }
    }

    pub fn get(&self, dog_id: DogId, now: DateTime<Utc>) -> Option<PresenceView> {
        let e = self.inner.get(&dog_id)?;
        let age_ms = (now - e.last_seen).num_milliseconds().max(0) as u64;
        let alive = age_ms <= PRESENCE_TTL_MS;
        let present = e.present && alive;
        Some(PresenceView {
            dog_id,
            terminal_id: e.terminal_id,
            present,
            confidence: if present { e.confidence } else { 0.0 },
            force_band: if present {
                e.force_band
            } else {
                ForceBand::Empty
            },
            last_seen: e.last_seen,
            seq: e.seq,
        })
    }

    pub fn list_present(&self, now: DateTime<Utc>) -> Vec<PresenceView> {
        self.inner
            .iter()
            .filter_map(|r| self.get(*r.key(), now))
            .filter(|v| v.present)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use protocol::ForceBand;

    #[test]
    fn ttl_expires_presence() {
        let store = PresenceStore::new();
        let dog = DogId::new();
        let t0 = Utc::now();
        store.upsert(PresenceReport {
            dog_id: dog,
            terminal_id: TerminalId::new(),
            present: true,
            confidence: 0.9,
            force_band: ForceBand::Medium,
            force_n: 100.0,
            tof_mm: Some(400),
            ts: t0,
            seq: 1,
        });
        assert!(store.get(dog, t0).unwrap().present);
        let later = t0 + Duration::milliseconds(PRESENCE_TTL_MS as i64 + 1);
        assert!(!store.get(dog, later).unwrap().present);
    }
}
