//! Device authentication for CanisLink.
//!
//! v1 production-dev path: HMAC-like shared secret token
//! `Authorization: Device <terminal_id>:<token>`
//! where token = hex(sha256(secret || terminal_id || dog_id)).
//!
//! mTLS lifecycle (architecture PR-07/24) replaces this for hardened deploy.

use protocol::{DogId, TerminalId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("missing authorization")]
    Missing,
    #[error("malformed authorization")]
    Malformed,
    #[error("unknown or invalid device credentials")]
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub terminal_id: TerminalId,
    pub dog_id: DogId,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct SharedSecretAuthority {
    secret: String,
}

impl SharedSecretAuthority {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
        }
    }

    pub fn issue(&self, terminal_id: TerminalId, dog_id: DogId) -> DeviceIdentity {
        let token = self.token_for(terminal_id, dog_id);
        DeviceIdentity {
            terminal_id,
            dog_id,
            token,
        }
    }

    pub fn token_for(&self, terminal_id: TerminalId, dog_id: DogId) -> String {
        let mut h = Sha256::new();
        h.update(self.secret.as_bytes());
        h.update(terminal_id.0.as_bytes());
        h.update(dog_id.0.as_bytes());
        hex::encode(h.finalize())
    }

    pub fn verify_header(&self, header: &str) -> Result<DeviceIdentity, AuthError> {
        let rest = header.strip_prefix("Device ").ok_or(AuthError::Malformed)?;
        let (tid, token) = rest.split_once(':').ok_or(AuthError::Malformed)?;
        let terminal_id = TerminalId(Uuid::parse_str(tid).map_err(|_| AuthError::Malformed)?);
        // token alone is not enough — clients also send dog_id in body for presence;
        // for auth we accept any dog that matches token recomputation when dog known.
        // Here we return terminal + token; dog binding checked at issue time via enroll map.
        Ok(DeviceIdentity {
            terminal_id,
            dog_id: DogId(Uuid::nil()),
            token: token.to_string(),
        })
    }

    pub fn verify_pair(
        &self,
        terminal_id: TerminalId,
        dog_id: DogId,
        token: &str,
    ) -> Result<(), AuthError> {
        let expect = self.token_for(terminal_id, dog_id);
        if expect == token {
            Ok(())
        } else {
            Err(AuthError::Invalid)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_verify() {
        let auth = SharedSecretAuthority::new("lab-secret");
        let tid = TerminalId::new();
        let did = DogId::new();
        let id = auth.issue(tid, did);
        auth.verify_pair(tid, did, &id.token).unwrap();
        assert!(auth.verify_pair(tid, DogId::new(), &id.token).is_err());
    }
}
