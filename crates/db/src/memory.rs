//! In-memory stores (unit tests + lightweight e2e mocks).

use bond::BondGraph;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use parking_lot::Mutex;
use policy::DogPolicy;
use protocol::{
    DogId, ForceBand, Invite, InviteId, PresenceReport, PresenceView, SessionId, SessionRecord,
    SessionState, TerminalId, PRESENCE_TTL_MS,
};
use std::collections::HashMap;
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
        let present = e.present && age_ms <= PRESENCE_TTL_MS;
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
    by_id: Mutex<HashMap<InviteId, Invite>>,
    open_for_dog: Mutex<HashMap<DogId, InviteId>>,
    invite_times: Mutex<HashMap<DogId, Vec<DateTime<Utc>>>>,
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
        let id = *open.get(&dog)?;
        self.by_id.lock().get(&id).cloned()
    }

    pub fn incoming_for(&self, dog: DogId) -> Option<Invite> {
        self.for_dog(dog)
            .filter(|i| i.to_dog == dog && i.state == SessionState::Ringing)
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

    pub fn record_invite(&self, dog: DogId, at: DateTime<Utc>) {
        self.invite_times.lock().entry(dog).or_default().push(at);
    }

    pub fn invites_last_hour(&self, dog: DogId, now: DateTime<Utc>) -> u32 {
        let mut map = self.invite_times.lock();
        let Some(times) = map.get_mut(&dog) else {
            return 0;
        };
        times.retain(|t| (now - *t).num_seconds() < 3600);
        times.len() as u32
    }
}

#[derive(Debug, Clone)]
struct SessionExtra {
    media_a: bool,
    media_b: bool,
}

#[derive(Debug, Default)]
pub struct SessionStore {
    by_id: Mutex<HashMap<SessionId, SessionRecord>>,
    by_dog: Mutex<HashMap<DogId, SessionId>>,
    media: Mutex<HashMap<SessionId, SessionExtra>>,
}

impl SessionStore {
    pub fn insert(&self, session: SessionRecord) -> Result<(), &'static str> {
        let mut by_dog = self.by_dog.lock();
        if by_dog.contains_key(&session.dog_a) || by_dog.contains_key(&session.dog_b) {
            return Err("busy");
        }
        by_dog.insert(session.dog_a, session.id);
        by_dog.insert(session.dog_b, session.id);
        self.media.lock().insert(
            session.id,
            SessionExtra {
                media_a: false,
                media_b: false,
            },
        );
        self.by_id.lock().insert(session.id, session);
        Ok(())
    }

    pub fn get(&self, id: SessionId) -> Option<SessionRecord> {
        self.by_id.lock().get(&id).cloned()
    }

    pub fn for_dog(&self, dog: DogId) -> Option<SessionRecord> {
        let id = *self.by_dog.lock().get(&dog)?;
        self.get(id)
    }

    pub fn all(&self) -> Vec<SessionRecord> {
        self.by_id.lock().values().cloned().collect()
    }

    pub fn update<F: FnOnce(&mut SessionRecord)>(
        &self,
        id: SessionId,
        f: F,
    ) -> Option<SessionRecord> {
        let mut map = self.by_id.lock();
        let s = map.get_mut(&id)?;
        f(s);
        Some(s.clone())
    }

    pub fn end(&self, id: SessionId) -> Option<SessionRecord> {
        let s = self.by_id.lock().remove(&id)?;
        let mut by_dog = self.by_dog.lock();
        by_dog.remove(&s.dog_a);
        by_dog.remove(&s.dog_b);
        self.media.lock().remove(&id);
        Some(s)
    }

    pub fn set_media_ready(
        &self,
        id: SessionId,
        dog: DogId,
        ready: bool,
    ) -> Option<(bool, SessionRecord)> {
        let sess = self.get(id)?;
        let mut media = self.media.lock();
        let extra = media.get_mut(&id)?;
        if dog == sess.dog_a {
            extra.media_a = ready;
        } else if dog == sess.dog_b {
            extra.media_b = ready;
        } else {
            return None;
        }
        let both = extra.media_a && extra.media_b;
        drop(media);
        if both {
            self.update(id, |s| s.state = SessionState::Active);
        }
        Some((both, self.get(id)?))
    }
}

#[derive(Debug, Default)]
pub struct PolicyStore {
    by_dog: Mutex<HashMap<DogId, DogPolicy>>,
    terminal_dog: Mutex<HashMap<TerminalId, DogId>>,
}

impl PolicyStore {
    pub fn ensure(&self, dog: DogId) -> DogPolicy {
        let mut map = self.by_dog.lock();
        map.entry(dog)
            .or_insert_with(|| DogPolicy::default_for(dog))
            .clone()
    }

    pub fn get(&self, dog: DogId) -> DogPolicy {
        self.ensure(dog)
    }

    pub fn set(&self, policy: DogPolicy) {
        self.by_dog.lock().insert(policy.dog_id, policy);
    }

    pub fn bind_terminal(&self, terminal: TerminalId, dog: DogId) {
        self.terminal_dog.lock().insert(terminal, dog);
        self.ensure(dog);
    }

    pub fn set_estop(&self, dog: DogId, on: bool) {
        let mut p = self.ensure(dog);
        p.emergency_stop = on;
        self.set(p);
    }

    pub fn set_social_disabled(&self, dog: DogId, on: bool) {
        let mut p = self.ensure(dog);
        p.social_disabled = on;
        self.set(p);
    }
}

#[derive(Debug, Default)]
pub struct AppData {
    pub presence: PresenceStore,
    pub bonds: Mutex<BondGraph>,
    pub invites: InviteStore,
    pub sessions: SessionStore,
    pub policies: PolicyStore,
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
