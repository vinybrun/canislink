//! In-memory stores for early production path.

use bond::BondGraph;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use protocol::{
    DogId, ForceBand, Invite, InviteId, PresenceReport, PresenceView, TerminalId, PRESENCE_TTL_MS,
};
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

    pub fn present_dog_ids(&self, now: DateTime<Utc>) -> Vec<DogId> {
        self.list_present(now)
            .into_iter()
            .map(|v| v.dog_id)
            .collect()
    }

    pub fn is_present(&self, dog_id: DogId, now: DateTime<Utc>) -> bool {
        self.get(dog_id, now).map(|v| v.present).unwrap_or(false)
    }
}

#[derive(Debug, Default)]
pub struct InviteStore {
    by_id: Mutex<std::collections::HashMap<InviteId, Invite>>,
    /// dog -> open invite they initiated or are receiving
    open_for_dog: Mutex<std::collections::HashMap<DogId, InviteId>>,
}

impl InviteStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, invite: Invite) -> Result<(), &'static str> {
        let mut open = self.open_for_dog.lock();
        if open.contains_key(&invite.from_dog) || open.contains_key(&invite.to_dog) {
            return Err("busy");
        }
        open.insert(invite.from_dog, invite.id);
        open.insert(invite.to_dog, invite.id);
        self.by_id.lock().insert(invite.id, invite);
        Ok(())
    }

    pub fn get(&self, id: InviteId) -> Option<Invite> {
        self.by_id.lock().get(&id).cloned()
    }

    pub fn for_dog(&self, dog: DogId) -> Option<Invite> {
        let open = self.open_for_dog.lock();
        let id = open.get(&dog)?;
        self.by_id.lock().get(id).cloned()
    }

    pub fn incoming_for(&self, dog: DogId) -> Option<Invite> {
        self.for_dog(dog)
            .filter(|i| i.to_dog == dog && i.state == protocol::SessionState::Ringing)
    }

    pub fn close(&self, id: InviteId) -> Option<Invite> {
        let inv = self.by_id.lock().remove(&id)?;
        let mut open = self.open_for_dog.lock();
        open.remove(&inv.from_dog);
        open.remove(&inv.to_dog);
        Some(inv)
    }

    pub fn expire_due(&self, now: DateTime<Utc>) -> Vec<Invite> {
        let due: Vec<InviteId> = self
            .by_id
            .lock()
            .iter()
            .filter(|(_, i)| now >= i.expires_at)
            .map(|(id, _)| *id)
            .collect();
        due.into_iter().filter_map(|id| self.close(id)).collect()
    }
}

#[derive(Debug, Default)]
pub struct AppData {
    pub presence: PresenceStore,
    pub bonds: Mutex<BondGraph>,
    pub invites: InviteStore,
}

impl AppData {
    pub fn new() -> Self {
        Self::default()
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
