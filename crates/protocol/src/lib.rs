//! Shared CanisLink protocol types.
//!
//! Normative design: `docs/architecture/canislink-system-architecture.md`

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod mcu;

/// Stable dog identity (bound to a terminal in v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DogId(pub Uuid);

impl DogId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DogId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DogId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Physical terminal identity (device cert / enrollment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TerminalId(pub Uuid);

impl TerminalId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TerminalId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TerminalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Dog-facing intent vocabulary (v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Call,
    Play,
    Again,
    Done,
}

/// Cloud session states (canonical).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    None,
    InvitePending,
    Ringing,
    Negotiating,
    Active,
    Ending,
    Closed,
    Failed,
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    Done,
    WalkAway,
    SegmentExpired,
    MaxDuration,
    PeerLeft,
    PolicyHalt,
    EmergencyStop,
    Timeout,
    MediaFailed,
    Revoked,
}

/// Coarse weight/size band from force sensor (telemetry only in v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceBand {
    Empty,
    Small,
    Medium,
    Large,
    Overload,
}

impl ForceBand {
    /// Map force newtons (approx) to band for lab/default mat calibration.
    pub fn from_newtons(f: f32) -> Self {
        if f < 15.0 {
            Self::Empty
        } else if f < 80.0 {
            Self::Small
        } else if f < 200.0 {
            Self::Medium
        } else if f < 450.0 {
            Self::Large
        } else {
            Self::Overload
        }
    }
}

/// Presence sample published by edge → cloud.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresenceReport {
    pub dog_id: DogId,
    pub terminal_id: TerminalId,
    pub present: bool,
    pub confidence: f32,
    pub force_band: ForceBand,
    pub force_n: f32,
    pub tof_mm: Option<u16>,
    pub ts: DateTime<Utc>,
    /// Sequence number from edge (monotonic per terminal).
    pub seq: u64,
}

/// Cloud view of live presence (after TTL).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PresenceView {
    pub dog_id: DogId,
    pub terminal_id: TerminalId,
    pub present: bool,
    pub confidence: f32,
    pub force_band: ForceBand,
    pub last_seen: DateTime<Utc>,
    pub seq: u64,
}

/// Cloud presence TTL (architecture: 10s).
pub const PRESENCE_TTL_MS: u64 = 10_000;
/// Edge publish interval while present (architecture: 2s).
pub const PRESENCE_PUBLISH_MS: u64 = 2_000;
/// Enter debounce (architecture: 800ms).
pub const PRESENCE_ENTER_MS: u64 = 800;
/// Exit debounce (architecture: 2500ms).
pub const PRESENCE_EXIT_MS: u64 = 2_500;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_roundtrip() {
        let v = serde_json::to_string(&Intent::Call).unwrap();
        assert_eq!(v, "\"call\"");
    }

    #[test]
    fn force_band_thresholds() {
        assert_eq!(ForceBand::from_newtons(0.0), ForceBand::Empty);
        assert_eq!(ForceBand::from_newtons(50.0), ForceBand::Small);
        assert_eq!(ForceBand::from_newtons(120.0), ForceBand::Medium);
    }
}
