//! SQLite durability for lab-shippable deployments.
//!
//! Control plane state is mirrored to SQLite so restarts keep dogs, bonds,
//! policies, and open sessions. Presence is also stored but still TTL-gated in memory.

use anyhow::Context;
use chrono::{DateTime, Utc};
use policy::DogPolicy;
use protocol::{
    DogId, ForceBand, Invite, InviteId, InviteMode, PresenceReport, SessionId, SessionRecord,
    SessionState, TerminalId,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

use crate::memory::AppData;

pub async fn open(database_url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)
        .context("parse database url")?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .context("connect sqlite")?;
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let sql = include_str!("../migrations/001_init.sql");
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        if stmt.is_empty() || stmt.starts_with("--") {
            continue;
        }
        sqlx::query(stmt)
            .execute(pool)
            .await
            .with_context(|| format!("migrate stmt failed: {stmt}"))?;
    }
    info!("sqlite schema ready");
    Ok(())
}

pub async fn load_into(pool: &SqlitePool, data: &AppData) -> anyhow::Result<()> {
    // dogs / terminals / policies
    let rows = sqlx::query("SELECT dog_id, timezone, sleep_start_min, sleep_end_min, utc_offset_min, emergency_stop, social_disabled, max_invites_per_hour, max_session_sec, segment_sec FROM dog_policy")
        .fetch_all(pool)
        .await?;
    for r in rows {
        let dog = DogId(Uuid::parse_str(r.get::<String, _>("dog_id").as_str())?);
        let p = DogPolicy {
            dog_id: dog,
            timezone: r.get("timezone"),
            sleep_start_min: r.get::<i64, _>("sleep_start_min") as u16,
            sleep_end_min: r.get::<i64, _>("sleep_end_min") as u16,
            utc_offset_min: r.get::<i64, _>("utc_offset_min") as i16,
            emergency_stop: r.get::<i64, _>("emergency_stop") != 0,
            social_disabled: r.get::<i64, _>("social_disabled") != 0,
            max_invites_per_hour: r.get::<i64, _>("max_invites_per_hour") as u32,
            max_session_sec: r.get::<i64, _>("max_session_sec") as u64,
            segment_sec: r.get::<i64, _>("segment_sec") as u64,
        };
        data.policies.set(p);
    }

    let terms = sqlx::query("SELECT terminal_id, dog_id FROM terminals")
        .fetch_all(pool)
        .await?;
    for r in terms {
        let tid = TerminalId(Uuid::parse_str(r.get::<String, _>("terminal_id").as_str())?);
        let did = DogId(Uuid::parse_str(r.get::<String, _>("dog_id").as_str())?);
        data.policies.bind_terminal(tid, did);
    }

    let bonds = sqlx::query("SELECT dog_a, dog_b, weight FROM bonds")
        .fetch_all(pool)
        .await?;
    {
        let mut g = data.bonds.lock();
        for r in bonds {
            let a = DogId(Uuid::parse_str(r.get::<String, _>("dog_a").as_str())?);
            let b = DogId(Uuid::parse_str(r.get::<String, _>("dog_b").as_str())?);
            let w: f64 = r.get("weight");
            g.set_weight(a, b, w as f32);
        }
    }

    // active invites
    let invites = sqlx::query(
        "SELECT invite_id, from_dog, to_dog, mode, state, created_at, expires_at FROM invites",
    )
    .fetch_all(pool)
    .await?;
    for r in invites {
        let inv = Invite {
            id: InviteId(Uuid::parse_str(r.get::<String, _>("invite_id").as_str())?),
            from_dog: DogId(Uuid::parse_str(r.get::<String, _>("from_dog").as_str())?),
            to_dog: DogId(Uuid::parse_str(r.get::<String, _>("to_dog").as_str())?),
            mode: parse_mode(&r.get::<String, _>("mode")),
            state: parse_state(&r.get::<String, _>("state")),
            created_at: parse_dt(&r.get::<String, _>("created_at"))?,
            expires_at: parse_dt(&r.get::<String, _>("expires_at"))?,
        };
        let _ = data.invites.insert(inv);
    }

    let sessions = sqlx::query(
        "SELECT session_id, invite_id, dog_a, dog_b, mode, state, started_at, max_end_at, segment_deadline_at, media_a, media_b FROM sessions",
    )
    .fetch_all(pool)
    .await?;
    for r in sessions {
        let mut sess = SessionRecord {
            id: SessionId(Uuid::parse_str(r.get::<String, _>("session_id").as_str())?),
            invite_id: InviteId(Uuid::parse_str(r.get::<String, _>("invite_id").as_str())?),
            dog_a: DogId(Uuid::parse_str(r.get::<String, _>("dog_a").as_str())?),
            dog_b: DogId(Uuid::parse_str(r.get::<String, _>("dog_b").as_str())?),
            mode: parse_mode(&r.get::<String, _>("mode")),
            state: parse_state(&r.get::<String, _>("state")),
            started_at: parse_dt(&r.get::<String, _>("started_at"))?,
            max_end_at: parse_dt(&r.get::<String, _>("max_end_at"))?,
            segment_deadline_at: parse_dt(&r.get::<String, _>("segment_deadline_at"))?,
        };
        let _ = data.sessions.insert(sess.clone());
        let ma = r.get::<i64, _>("media_a") != 0;
        let mb = r.get::<i64, _>("media_b") != 0;
        if ma {
            let _ = data.sessions.set_media_ready(sess.id, sess.dog_a, true);
        }
        if mb {
            let _ = data.sessions.set_media_ready(sess.id, sess.dog_b, true);
        }
        // refresh state after media flags
        if let Some(s) = data.sessions.get(sess.id) {
            sess = s;
        }
        let _ = sess;
    }

    info!("loaded durable state from sqlite");
    Ok(())
}

fn parse_mode(s: &str) -> InviteMode {
    match s {
        "play_active" => InviteMode::PlayActive,
        _ => InviteMode::Portal,
    }
}

fn parse_state(s: &str) -> SessionState {
    match s {
        "invite_pending" => SessionState::InvitePending,
        "ringing" => SessionState::Ringing,
        "negotiating" => SessionState::Negotiating,
        "active" => SessionState::Active,
        "ending" => SessionState::Ending,
        "closed" => SessionState::Closed,
        "failed" => SessionState::Failed,
        _ => SessionState::None,
    }
}

fn mode_str(m: InviteMode) -> &'static str {
    match m {
        InviteMode::Portal => "portal",
        InviteMode::PlayActive => "play_active",
    }
}

fn state_str(s: SessionState) -> &'static str {
    match s {
        SessionState::None => "none",
        SessionState::InvitePending => "invite_pending",
        SessionState::Ringing => "ringing",
        SessionState::Negotiating => "negotiating",
        SessionState::Active => "active",
        SessionState::Ending => "ending",
        SessionState::Closed => "closed",
        SessionState::Failed => "failed",
    }
}

fn parse_dt(s: &str) -> anyhow::Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    // sqlite datetime
    Ok(
        DateTime::parse_from_str(&format!("{s}+00:00"), "%Y-%m-%d %H:%M:%S%z")
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    )
}

pub async fn save_enroll(
    pool: &SqlitePool,
    terminal_id: TerminalId,
    dog_id: DogId,
    token_hash: &str,
) -> anyhow::Result<()> {
    sqlx::query("INSERT OR IGNORE INTO dogs(dog_id) VALUES (?1)")
        .bind(dog_id.to_string())
        .execute(pool)
        .await?;
    sqlx::query(
        "INSERT OR REPLACE INTO terminals(terminal_id, dog_id, token_hash) VALUES (?1, ?2, ?3)",
    )
    .bind(terminal_id.to_string())
    .bind(dog_id.to_string())
    .bind(token_hash)
    .execute(pool)
    .await?;
    let p = DogPolicy::default_for(dog_id);
    save_policy(pool, &p).await?;
    Ok(())
}

pub async fn save_policy(pool: &SqlitePool, p: &DogPolicy) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT OR REPLACE INTO dog_policy(
            dog_id, timezone, sleep_start_min, sleep_end_min, utc_offset_min,
            emergency_stop, social_disabled, max_invites_per_hour, max_session_sec, segment_sec
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
    )
    .bind(p.dog_id.to_string())
    .bind(&p.timezone)
    .bind(p.sleep_start_min as i64)
    .bind(p.sleep_end_min as i64)
    .bind(p.utc_offset_min as i64)
    .bind(if p.emergency_stop { 1 } else { 0 })
    .bind(if p.social_disabled { 1 } else { 0 })
    .bind(p.max_invites_per_hour as i64)
    .bind(p.max_session_sec as i64)
    .bind(p.segment_sec as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_bond(pool: &SqlitePool, a: DogId, b: DogId, w: f32) -> anyhow::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO bonds(dog_a, dog_b, weight) VALUES (?1,?2,?3)")
        .bind(a.to_string())
        .bind(b.to_string())
        .bind(w as f64)
        .execute(pool)
        .await?;
    sqlx::query("INSERT OR REPLACE INTO bonds(dog_a, dog_b, weight) VALUES (?1,?2,?3)")
        .bind(b.to_string())
        .bind(a.to_string())
        .bind(w as f64)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_presence(pool: &SqlitePool, r: &PresenceReport) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT OR REPLACE INTO presence(
            dog_id, terminal_id, present, confidence, force_band, force_n, tof_mm, last_seen, seq
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
    )
    .bind(r.dog_id.to_string())
    .bind(r.terminal_id.to_string())
    .bind(if r.present { 1 } else { 0 })
    .bind(r.confidence as f64)
    .bind(format!("{:?}", r.force_band).to_lowercase())
    .bind(r.force_n as f64)
    .bind(r.tof_mm.map(|v| v as i64))
    .bind(r.ts.to_rfc3339())
    .bind(r.seq as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_invite(pool: &SqlitePool, inv: &Invite) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT OR REPLACE INTO invites(
            invite_id, from_dog, to_dog, mode, state, created_at, expires_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7)"#,
    )
    .bind(inv.id.to_string())
    .bind(inv.from_dog.to_string())
    .bind(inv.to_dog.to_string())
    .bind(mode_str(inv.mode))
    .bind(state_str(inv.state))
    .bind(inv.created_at.to_rfc3339())
    .bind(inv.expires_at.to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_invite(pool: &SqlitePool, id: InviteId) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM invites WHERE invite_id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn record_invite_event(
    pool: &SqlitePool,
    dog: DogId,
    at: DateTime<Utc>,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO invite_events(dog_id, at) VALUES (?1, ?2)")
        .bind(dog.to_string())
        .bind(at.to_rfc3339())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn save_session(
    pool: &SqlitePool,
    s: &SessionRecord,
    ma: bool,
    mb: bool,
) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT OR REPLACE INTO sessions(
            session_id, invite_id, dog_a, dog_b, mode, state, started_at, max_end_at, segment_deadline_at, media_a, media_b
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"#,
    )
    .bind(s.id.to_string())
    .bind(s.invite_id.to_string())
    .bind(s.dog_a.to_string())
    .bind(s.dog_b.to_string())
    .bind(mode_str(s.mode))
    .bind(state_str(s.state))
    .bind(s.started_at.to_rfc3339())
    .bind(s.max_end_at.to_rfc3339())
    .bind(s.segment_deadline_at.to_rfc3339())
    .bind(if ma { 1 } else { 0 })
    .bind(if mb { 1 } else { 0 })
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_session(pool: &SqlitePool, id: SessionId) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM sessions WHERE session_id = ?1")
        .bind(id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

#[allow(dead_code)]
fn _fb(_: ForceBand) {}
