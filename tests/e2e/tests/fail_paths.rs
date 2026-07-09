//! Anticipated failure scenarios from untested paths.

use chrono::Utc;
use db::AppData;
use device_auth::SharedSecretAuthority;
use policy::{invite_rate_ok, social_allowed, DogPolicy};
use protocol::DogId;
use session::{route_invite, InviteError};
use bond::BondGraph;

#[test]
fn offline_peer_not_routable() {
    let mut g = BondGraph::new();
    let a = DogId::new();
    let b = DogId::new();
    g.bootstrap_mutual(a, b, 0.9);
    // bonded but B not present
    assert_eq!(
        route_invite(a, None, &g, &[], true),
        Err(InviteError::NoEligiblePeer)
    );
    assert_eq!(
        route_invite(a, Some(b), &g, &[], true),
        Err(InviteError::PeerNotPresent)
    );
}

#[test]
fn caller_not_present_denied() {
    let mut g = BondGraph::new();
    let a = DogId::new();
    let b = DogId::new();
    g.bootstrap_mutual(a, b, 0.9);
    assert_eq!(
        route_invite(a, Some(b), &g, &[b], false),
        Err(InviteError::CallerNotPresent)
    );
}

#[test]
fn sleep_and_rate_limit_block_social() {
    let mut p = DogPolicy::default_for(DogId::new());
    p.sleep_start_min = 0;
    p.sleep_end_min = 24 * 60; // always sleep
    p.utc_offset_min = 0;
    assert!(social_allowed(&p, Utc::now()).is_err());
    assert!(invite_rate_ok(99, 12).is_err());
}

#[test]
fn memory_busy_second_invite() {
    use protocol::{InviteMode, SessionState};
    use session::new_invite;
    let data = AppData::new();
    let a = DogId::new();
    let b = DogId::new();
    let inv = new_invite(a, b, InviteMode::Portal);
    assert!(data.invites.insert(inv).is_ok());
    let inv2 = new_invite(a, DogId::new(), InviteMode::Portal);
    assert_eq!(data.invites.insert(inv2), Err("busy"));
    let _ = SessionState::Ringing;
    let _ = SharedSecretAuthority::new("x");
}
