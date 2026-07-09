//! Shared CanisLink protocol types (scaffold).
//!
//! Normative design: `docs/architecture/canislink-system-architecture.md`

use serde::{Deserialize, Serialize};

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

pub fn scaffold_ok() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_roundtrip() {
        let v = serde_json::to_string(&Intent::Call).unwrap();
        assert_eq!(v, "\"call\"");
        assert!(scaffold_ok());
    }
}
