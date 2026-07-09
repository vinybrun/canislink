//! Pure invite routing + session allocation helpers.

use bond::BondGraph;
use chrono::{Duration, Utc};
use protocol::{
    DogId, Invite, InviteId, InviteMode, SessionId, SessionRecord, SessionState, RING_TIMEOUT_MS,
    SESSION_MAX_MS, SESSION_SEGMENT_MS,
};
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcceptError {
    #[error("invite not found")]
    NotFound,
    #[error("not the callee")]
    NotCallee,
    #[error("invite not ringing")]
    NotRinging,
    #[error("acceptor not present")]
    NotPresent,
    #[error("invite expired")]
    Expired,
}

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

/// Accept converts ringing invite → Active session (media stub ready immediately in v1).
pub fn accept_invite(
    invite: &Invite,
    acceptor: DogId,
    acceptor_present: bool,
    now: chrono::DateTime<Utc>,
) -> Result<SessionRecord, AcceptError> {
    if invite.to_dog != acceptor {
        return Err(AcceptError::NotCallee);
    }
    if invite.state != SessionState::Ringing {
        return Err(AcceptError::NotRinging);
    }
    if is_expired(invite, now) {
        return Err(AcceptError::Expired);
    }
    if !acceptor_present {
        return Err(AcceptError::NotPresent);
    }
    Ok(SessionRecord {
        id: SessionId::new(),
        invite_id: invite.id,
        dog_a: invite.from_dog,
        dog_b: invite.to_dog,
        mode: invite.mode,
        state: SessionState::Active,
        started_at: now,
        max_end_at: now + Duration::milliseconds(SESSION_MAX_MS as i64),
        segment_deadline_at: now + Duration::milliseconds(SESSION_SEGMENT_MS as i64),
    })
}

pub fn webrtc_role(session: &SessionRecord, dog: DogId) -> &'static str {
    if dog == session.dog_a {
        "offerer"
    } else {
        "answerer"
    }
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
    }

    #[test]
    fn accept_requires_callee_present() {
        let a = DogId::new();
        let b = DogId::new();
        let inv = new_invite(a, b, InviteMode::Portal);
        assert_eq!(
            accept_invite(&inv, b, false, Utc::now()),
            Err(AcceptError::NotPresent)
        );
        let s = accept_invite(&inv, b, true, Utc::now()).unwrap();
        assert_eq!(s.state, SessionState::Active);
        assert_eq!(webrtc_role(&s, a), "offerer");
        assert_eq!(webrtc_role(&s, b), "answerer");
    }
}
