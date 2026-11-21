-- Jade Public Game Schema (SQLite / PostgreSQL compatible baseline)
-- Owns accounts, sessions, invitations, authoritative matches, match events, and personal studies.
-- Historical corpus observations, entity resolution, and training runs live in private play-jade/cinnabar schemas.

CREATE TABLE IF NOT EXISTS accounts (
    account_id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    is_guest INTEGER NOT NULL DEFAULT 1,
    locale TEXT NOT NULL DEFAULT 'en-US',
    piece_face_set TEXT NOT NULL DEFAULT 'simplified-prototype-v1',
    board_material TEXT NOT NULL DEFAULT 'pale-jade',
    notation_format TEXT NOT NULL DEFAULT 'chinese',
    reduced_motion INTEGER NOT NULL DEFAULT 0,
    sound_enabled INTEGER NOT NULL DEFAULT 1,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS auth_sessions (
    session_id_hash TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    transport_kind TEXT NOT NULL, -- 'web_cookie' | 'native_token'
    csrf_token TEXT NOT NULL,
    refresh_token_hash TEXT,
    device_label TEXT,
    revoked_at_ms INTEGER,
    expires_at_ms INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS matches (
    match_id TEXT PRIMARY KEY,
    invitation_code TEXT NOT NULL UNIQUE,
    rules_profile TEXT NOT NULL DEFAULT 'program-2023.11-v1',
    is_friendly INTEGER NOT NULL DEFAULT 1,
    initial_fen TEXT NOT NULL,
    current_fen TEXT NOT NULL,
    current_seq INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'waiting', -- 'waiting' | 'playing' | 'finished' | 'interrupted'
    red_account_id TEXT REFERENCES accounts(account_id),
    black_account_id TEXT REFERENCES accounts(account_id),
    red_seat_token_hash TEXT,
    black_seat_token_hash TEXT,
    clock_initial_ms INTEGER, -- NULL for untimed
    clock_increment_ms INTEGER NOT NULL DEFAULT 0,
    red_remaining_ms INTEGER,
    black_remaining_ms INTEGER,
    turn_started_utc_ms INTEGER,
    outcome TEXT, -- NULL | 'red_win' | 'black_win' | 'draw' | 'interrupted'
    reason_code TEXT,
    visibility TEXT NOT NULL DEFAULT 'private', -- 'private' | 'unlisted' | 'public'
    red_visibility_choice TEXT NOT NULL DEFAULT 'private',
    black_visibility_choice TEXT NOT NULL DEFAULT 'private',
    red_consent_training INTEGER NOT NULL DEFAULT 0,
    black_consent_training INTEGER NOT NULL DEFAULT 0,
    red_consent_research INTEGER NOT NULL DEFAULT 0,
    black_consent_research INTEGER NOT NULL DEFAULT 0,
    red_consent_dataset INTEGER NOT NULL DEFAULT 0,
    black_consent_dataset INTEGER NOT NULL DEFAULT 0,
    rematch_of_match_id TEXT,
    next_rematch_id TEXT,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS match_events (
    match_id TEXT NOT NULL REFERENCES matches(match_id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    command_id TEXT NOT NULL,
    actor_side TEXT, -- 'red' | 'black' | 'system'
    event_type TEXT NOT NULL, -- 'seat_joined' | 'move' | 'draw_offered' | 'draw_accepted' | 'resigned' | 'timeout' | 'interrupted'
    payload_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY (match_id, seq),
    UNIQUE (match_id, command_id)
);

CREATE TABLE IF NOT EXISTS studies (
    study_id TEXT PRIMARY KEY,
    owner_account_id TEXT NOT NULL REFERENCES accounts(account_id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    source_match_id TEXT REFERENCES matches(match_id),
    source_archive_game_id TEXT,
    rules_profile TEXT NOT NULL DEFAULT 'program-2023.11-v1',
    root_fen TEXT NOT NULL,
    nodes_json TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    visibility TEXT NOT NULL DEFAULT 'private', -- 'private' | 'unlisted' | 'public'
    consent_research INTEGER NOT NULL DEFAULT 0,
    consent_dataset INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_matches_invitation ON matches(invitation_code);
CREATE INDEX IF NOT EXISTS idx_studies_owner ON studies(owner_account_id);
