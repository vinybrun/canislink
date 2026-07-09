-- CanisLink durable store (SQLite for lab kit - Postgres later)

CREATE TABLE IF NOT EXISTS dogs (
    dog_id TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS terminals (
    terminal_id TEXT PRIMARY KEY NOT NULL,
    dog_id TEXT NOT NULL REFERENCES dogs(dog_id),
    token_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS bonds (
    dog_a TEXT NOT NULL,
    dog_b TEXT NOT NULL,
    weight REAL NOT NULL,
    PRIMARY KEY (dog_a, dog_b)
);

CREATE TABLE IF NOT EXISTS dog_policy (
    dog_id TEXT PRIMARY KEY NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    sleep_start_min INTEGER NOT NULL DEFAULT 1320,
    sleep_end_min INTEGER NOT NULL DEFAULT 420,
    utc_offset_min INTEGER NOT NULL DEFAULT 0,
    emergency_stop INTEGER NOT NULL DEFAULT 0,
    social_disabled INTEGER NOT NULL DEFAULT 0,
    max_invites_per_hour INTEGER NOT NULL DEFAULT 12,
    max_session_sec INTEGER NOT NULL DEFAULT 900,
    segment_sec INTEGER NOT NULL DEFAULT 300
);

CREATE TABLE IF NOT EXISTS presence (
    dog_id TEXT PRIMARY KEY NOT NULL,
    terminal_id TEXT NOT NULL,
    present INTEGER NOT NULL,
    confidence REAL NOT NULL,
    force_band TEXT NOT NULL,
    force_n REAL NOT NULL DEFAULT 0,
    tof_mm INTEGER,
    last_seen TEXT NOT NULL,
    seq INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS invites (
    invite_id TEXT PRIMARY KEY NOT NULL,
    from_dog TEXT NOT NULL,
    to_dog TEXT NOT NULL,
    mode TEXT NOT NULL,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS invite_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    dog_id TEXT NOT NULL,
    at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    invite_id TEXT NOT NULL,
    dog_a TEXT NOT NULL,
    dog_b TEXT NOT NULL,
    mode TEXT NOT NULL,
    state TEXT NOT NULL,
    started_at TEXT NOT NULL,
    max_end_at TEXT NOT NULL,
    segment_deadline_at TEXT NOT NULL,
    media_a INTEGER NOT NULL DEFAULT 0,
    media_b INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_invites_from ON invites(from_dog);
CREATE INDEX IF NOT EXISTS idx_invites_to ON invites(to_dog);
CREATE INDEX IF NOT EXISTS idx_sessions_dogs ON sessions(dog_a, dog_b);
CREATE INDEX IF NOT EXISTS idx_invite_events_dog ON invite_events(dog_id, at);
