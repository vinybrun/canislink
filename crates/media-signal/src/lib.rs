//! WebRTC signaling envelopes (session-scoped rooms).

use protocol::{DogId, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalMsg {
    Join {
        session_id: SessionId,
        dog_id: DogId,
        role: String, // offerer | answerer
    },
    Ready {
        session_id: SessionId,
        dog_id: DogId,
    },
    Offer {
        session_id: SessionId,
        from: DogId,
        sdp: String,
    },
    Answer {
        session_id: SessionId,
        from: DogId,
        sdp: String,
    },
    Ice {
        session_id: SessionId,
        from: DogId,
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
    Hangup {
        session_id: SessionId,
        from: DogId,
    },
    Error {
        message: String,
    },
}
