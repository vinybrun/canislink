//! Welfare / social policy rules (architecture: rules as code).
//!
//! Humans never appear in accept path. Steward may set flags consumed here.

use chrono::{DateTime, Timelike, Utc};
use protocol::DogId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Per-dog policy snapshot (also embedded in ConfigV1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DogPolicy {
    pub dog_id: DogId,
    /// IANA timezone name (v1: evaluated as fixed offset minutes for simplicity in alpha).
    pub timezone: String,
    /// Minutes from local midnight for sleep start (inclusive).
    pub sleep_start_min: u16,
    /// Minutes from local midnight for sleep end (exclusive). Wrap-aware.
    pub sleep_end_min: u16,
    /// Fixed offset minutes east of UTC for alpha (e.g. -480 for PST).
    pub utc_offset_min: i16,
    pub emergency_stop: bool,
    pub social_disabled: bool,
    /// Max invites per rolling hour.
    pub max_invites_per_hour: u32,
    pub max_session_sec: u64,
    pub segment_sec: u64,
}

impl DogPolicy {
    pub fn default_for(dog_id: DogId) -> Self {
        Self {
            dog_id,
            timezone: "UTC".into(),
            sleep_start_min: 22 * 60, // 22:00
            sleep_end_min: 7 * 60,    // 07:00
            utc_offset_min: 0,
            emergency_stop: false,
            social_disabled: false,
            max_invites_per_hour: 12,
            max_session_sec: 15 * 60,
            segment_sec: 5 * 60,
        }
    }

    pub fn local_minutes_now(&self, now: DateTime<Utc>) -> u16 {
        let offset = chrono::FixedOffset::east_opt(self.utc_offset_min as i32 * 60)
            .unwrap_or(chrono::FixedOffset::east_opt(0).unwrap());
        let local = now.with_timezone(&offset);
        (local.hour() as u16) * 60 + local.minute() as u16
    }

    pub fn in_sleep(&self, now: DateTime<Utc>) -> bool {
        let m = self.local_minutes_now(now);
        if self.sleep_start_min == self.sleep_end_min {
            return false;
        }
        if self.sleep_start_min < self.sleep_end_min {
            m >= self.sleep_start_min && m < self.sleep_end_min
        } else {
            // wraps midnight
            m >= self.sleep_start_min || m < self.sleep_end_min
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PolicyDeny {
    #[error("emergency_stop active")]
    EmergencyStop,
    #[error("social_disabled")]
    SocialDisabled,
    #[error("sleep window")]
    Sleep,
    #[error("invite rate limit")]
    RateLimit,
}

/// Can this dog initiate or accept social contact right now?
pub fn social_allowed(p: &DogPolicy, now: DateTime<Utc>) -> Result<(), PolicyDeny> {
    if p.emergency_stop {
        return Err(PolicyDeny::EmergencyStop);
    }
    if p.social_disabled {
        return Err(PolicyDeny::SocialDisabled);
    }
    if p.in_sleep(now) {
        return Err(PolicyDeny::Sleep);
    }
    Ok(())
}

pub fn invite_rate_ok(sent_in_window: u32, max_per_hour: u32) -> Result<(), PolicyDeny> {
    if sent_in_window >= max_per_hour {
        Err(PolicyDeny::RateLimit)
    } else {
        Ok(())
    }
}

pub fn session_past_max(started: DateTime<Utc>, max_sec: u64, now: DateTime<Utc>) -> bool {
    (now - started).num_seconds() as u64 >= max_sec
}

pub fn session_past_segment(deadline: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now >= deadline
}

/// Extend soft segment; never past max_end.
pub fn extend_segment(
    now: DateTime<Utc>,
    segment_sec: u64,
    max_end: DateTime<Utc>,
) -> DateTime<Utc> {
    let next = now + chrono::Duration::seconds(segment_sec as i64);
    if next > max_end {
        max_end
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::DogId;

    #[test]
    fn sleep_wraps_midnight() {
        let mut p = DogPolicy::default_for(DogId::new());
        p.sleep_start_min = 22 * 60;
        p.sleep_end_min = 7 * 60;
        p.utc_offset_min = 0;
        // 23:00 UTC
        let t = DateTime::parse_from_rfc3339("2026-07-09T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(p.in_sleep(t));
        let t2 = DateTime::parse_from_rfc3339("2026-07-09T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!p.in_sleep(t2));
    }

    #[test]
    fn estop_blocks() {
        let mut p = DogPolicy::default_for(DogId::new());
        p.emergency_stop = true;
        assert_eq!(
            social_allowed(&p, Utc::now()),
            Err(PolicyDeny::EmergencyStop)
        );
    }

    #[test]
    fn rate_limit() {
        assert!(invite_rate_ok(11, 12).is_ok());
        assert_eq!(invite_rate_ok(12, 12), Err(PolicyDeny::RateLimit));
    }
}
