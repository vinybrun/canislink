//! ICE / TURN credential distribution for lab and production-shaped NAT traversal.
//!
//! Supports:
//! - static STUN/TURN URIs + username/credential via env
//! - coturn `use-auth-secret` ephemeral REST credentials (HMAC-SHA1)

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use hmac::{Hmac, Mac};
use protocol::IceConfig;
use sha1::Sha1;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

/// Runtime ICE policy loaded from env / CLI.
#[derive(Debug, Clone)]
pub struct IcePolicy {
    pub stun_urls: Vec<String>,
    pub turn_uris: Vec<String>,
    /// Static TURN username (used when `turn_secret` is empty).
    pub turn_username: String,
    /// Static TURN credential.
    pub turn_credential: String,
    /// Coturn shared secret for time-limited REST credentials.
    pub turn_secret: String,
    pub ttl_sec: u64,
    pub force_turn: bool,
}

impl Default for IcePolicy {
    fn default() -> Self {
        Self {
            stun_urls: vec!["stun:stun.l.google.com:19302".into()],
            turn_uris: vec![],
            turn_username: String::new(),
            turn_credential: String::new(),
            turn_secret: String::new(),
            ttl_sec: 3600,
            force_turn: false,
        }
    }
}

impl IcePolicy {
    /// Parse comma-separated URL list env values.
    pub fn from_parts(
        stun: &str,
        turn_uris: &str,
        username: &str,
        credential: &str,
        secret: &str,
        ttl_sec: u64,
        force_turn: bool,
    ) -> Self {
        let mut p = Self::default();
        let stun_list = split_csv(stun);
        if !stun_list.is_empty() {
            p.stun_urls = stun_list;
        }
        p.turn_uris = split_csv(turn_uris);
        p.turn_username = username.to_string();
        p.turn_credential = credential.to_string();
        p.turn_secret = secret.to_string();
        p.ttl_sec = ttl_sec.max(60);
        p.force_turn = force_turn;
        p
    }

    /// Build a per-request [`IceConfig`] (ephemeral TURN creds when secret is set).
    pub fn mint(&self, dog_label: &str) -> IceConfig {
        let (username, credential) = if !self.turn_secret.is_empty() && !self.turn_uris.is_empty()
        {
            mint_turn_rest(&self.turn_secret, dog_label, self.ttl_sec)
        } else {
            (self.turn_username.clone(), self.turn_credential.clone())
        };
        IceConfig {
            stun_urls: self.stun_urls.clone(),
            turn_uris: self.turn_uris.clone(),
            turn_username: username,
            turn_credential: credential,
            ttl_sec: self.ttl_sec,
        }
    }
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

/// Coturn REST API style: username = `{expiry}:{user}`, password = base64(hmac-sha1(secret, username)).
pub fn mint_turn_rest(secret: &str, user: &str, ttl_sec: u64) -> (String, String) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let expiry = now.saturating_add(ttl_sec);
    // sanitize user fragment for TURN username
    let user_safe: String = user
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let username = format!("{expiry}:{user_safe}");
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(username.as_bytes());
    let credential = B64.encode(mac.finalize().into_bytes());
    (username, credential)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_is_deterministic_for_same_second_bucket() {
        let (u1, c1) = mint_turn_rest("lab-secret", "dog-a", 3600);
        let (u2, c2) = mint_turn_rest("lab-secret", "dog-a", 3600);
        assert_eq!(u1, u2);
        assert_eq!(c1, c2);
        assert!(u1.contains(":dog-a") || u1.contains("dog_a") || u1.contains("dog-a"));
        assert!(!c1.is_empty());
    }

    #[test]
    fn policy_default_has_stun() {
        let p = IcePolicy::default();
        let ice = p.mint("x");
        assert!(!ice.stun_urls.is_empty());
        assert!(ice.turn_uris.is_empty());
    }

    #[test]
    fn policy_with_secret_mints_creds() {
        let p = IcePolicy::from_parts(
            "stun:stun.example:3478",
            "turn:turn.example:3478?transport=udp",
            "",
            "",
            "shared-secret",
            600,
            true,
        );
        let ice = p.mint("dog42");
        assert_eq!(ice.stun_urls, vec!["stun:stun.example:3478"]);
        assert_eq!(ice.turn_uris.len(), 1);
        assert!(ice.turn_username.contains("dog42") || ice.turn_username.contains("dog_42"));
        assert!(!ice.turn_credential.is_empty());
        assert_eq!(ice.ttl_sec, 600);
    }
}
