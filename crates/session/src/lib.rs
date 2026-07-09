//! Pure invite routing + ring state helpers (no I/O).

use bond::BondGraph;
use chrono::{Duration, Utc};
use protocol::{DogId, Invite, InviteId, InviteMode, SessionState, RING_TIMEOUT_MS};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InviteError {
    #[error("caller not present")]
    CallerNotPresent,
    #[error("no eligible present peer")]
    NoEligiblePeer,
    #[error("peer not present")]
    PeerNotPresent,
    #[error("not mutually bonded")]
    NotBonded,
    #[error("caller already has open invite")]
    CallerBusy,
    #[error("peer already ringing or in session")]
    PeerBusy,
}

/// Select K=1 peer: highest mutual bond among present dogs.
pub fn route_invite(
    from: DogId,
    preferred: Option<DogId>,
    bonds: &BondGraph,
    present: &[DogId],
    caller_present: bool,
) -> Result<DogId, InviteError> {
    if !caller_present {
        return Err(InviteError::CallerNotPresent);
    }
    if let Some(to) = preferred {
        if to == from {
            return Err(InviteError::NoEligiblePeer);
        }
        if !bonds.mutual_eligible(from, to) {
            return Err(InviteError::NotBonded);
        }
        if !present.contains(&to) {
            return Err(InviteError::PeerNotPresent);
        }
        return Ok(to);
    }
    for (peer, _w) in bonds.candidates(from) {
        if present.contains(&peer) {
            return Ok(peer);
        }
    }
    Err(InviteError::NoEligiblePeer)
}

pub fn new_invite(from: DogId, to: DogId, mode: InviteMode) -> Invite {
    let now = Utc::now();
    Invite {
        id: InviteId::new(),
        from_dog: from,
        to_dog: to,
        mode,
        state: SessionState::Ringing,
        created_at: now,
        expires_at: now + Duration::milliseconds(RING_TIMEOUT_MS as i64),
    }
}

pub fn is_expired(invite: &Invite, now: chrono::DateTime<Utc>) -> bool {
    now >= invite.expires_at
}

#[cfg(test)]
mod tests {
    use super::*;
    use bond::BondGraph;

    #[test]
    fn routes_to_strongest_present() {
        let mut g = BondGraph::new();
        let a = DogId::new();
        let b = DogId::new();
        let c = DogId::new();
        g.bootstrap_mutual(a, b, 0.4);
        g.bootstrap_mutual(a, c, 0.9);
        let to = route_invite(a, None, &g, &[b, c], true).unwrap();
        assert_eq!(to, c);
        let to = route_invite(a, None, &g, &[b], true).unwrap();
        assert_eq!(to, b);
        assert_eq!(
            route_invite(a, None, &g, &[], true),
            Err(InviteError::NoEligiblePeer)
        );
    }

    #[test]
    fn preferred_must_be_present_and_bonded() {
        let mut g = BondGraph::new();
        let a = DogId::new();
        let b = DogId::new();
        g.bootstrap_mutual(a, b, 0.5);
        assert!(route_invite(a, Some(b), &g, &[b], true).is_ok());
        assert_eq!(
            route_invite(a, Some(b), &g, &[], true),
            Err(InviteError::PeerNotPresent)
        );
    }
}
