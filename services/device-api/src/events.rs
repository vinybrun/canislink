//! Device realtime events (invite push, session updates, keepalive).

use protocol::{DogId, Invite, SessionRecord};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DeviceEvent {
    Hello { dog_id: DogId },
    InviteRinging { invite: Invite, lure_led: String },
    InviteClosed { invite_id: String, reason: String },
    SessionUpdated { session: SessionRecord },
    SessionEnded { session_id: String, reason: String },
    Ping { ts_ms: u64 },
}

#[derive(Clone, Default)]
pub struct EventHub {
    inner: Arc<RwLock<HashMap<DogId, broadcast::Sender<DeviceEvent>>>>,
}

impl EventHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn subscribe(&self, dog: DogId) -> broadcast::Receiver<DeviceEvent> {
        let mut map = self.inner.write().await;
        map.entry(dog)
            .or_insert_with(|| broadcast::channel(128).0)
            .subscribe()
    }

    pub async fn publish(&self, dog: DogId, event: DeviceEvent) {
        let map = self.inner.read().await;
        if let Some(tx) = map.get(&dog) {
            let _ = tx.send(event);
        }
    }

    pub async fn publish_many(&self, dogs: &[DogId], event: DeviceEvent) {
        for d in dogs {
            self.publish(*d, event.clone()).await;
        }
    }
}
