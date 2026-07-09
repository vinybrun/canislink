//! Device realtime events (invite push, session updates).
//! Replaces poll-only lure path for phones / Android portal.

use protocol::{DogId, Invite, SessionRecord};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DeviceEvent {
    InviteRinging { invite: Invite, lure_led: String },
    InviteClosed { invite_id: String, reason: String },
    SessionUpdated { session: SessionRecord },
    SessionEnded { session_id: String, reason: String },
    Ping,
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
        let tx = map
            .entry(dog)
            .or_insert_with(|| broadcast::channel(64).0)
            .clone();
        tx.subscribe()
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
