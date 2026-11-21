use archive_client::{MatchPrivacyConsent, SampleArchiveProvider, Visibility};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{RwLock, broadcast, mpsc, oneshot};
use tower_http::cors::{AllowOrigin, CorsLayer};
use uuid::Uuid;
use xiangqi_core::{
    ADOPTED_RULES_PROFILE, Adjudication, GameOutcome, GameState, Move, Side, format_chinese_move,
};

const INITIAL_SCHEMA_SQL: &str = include_str!("../../../db/game/0001_initial_game_schema.sql");

pub enum ActorMsg {
    Command(MailboxEnvelope),
    StartPlaying { started_at: Instant },
}

#[derive(Clone)]
pub struct MatchActorHandle {
    pub tx: mpsc::Sender<ActorMsg>,
    pub bcast: broadcast::Sender<MatchSnapshotResponse>,
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub archive: Arc<SampleArchiveProvider>,
    pub matches: Arc<RwLock<HashMap<String, MatchActorHandle>>>,
    pub rematch_lock: Arc<tokio::sync::Mutex<()>>,
    pub allowed_origins: Arc<Vec<String>>,
    _db_owner_lock: Option<Arc<std::fs::File>>,
}

pub struct MailboxEnvelope {
    pub actor_account_id: String,
    pub command: MatchCommandRequest,
    pub received_at: Instant,
    pub received_utc_ms: i64,
    pub reply: oneshot::Sender<Result<MatchSnapshotResponse, ApiError>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MatchCommandRequest {
    Move {
        protocol: u8,
        match_id: String,
        command_id: String,
        expected_seq: u64,
        #[serde(rename = "move")]
        mv: CoordMovePayload,
    },
    Resign {
        protocol: u8,
        match_id: String,
        command_id: String,
        expected_seq: u64,
    },
    OfferDraw {
        protocol: u8,
        match_id: String,
        command_id: String,
        expected_seq: u64,
    },
    AcceptDraw {
        protocol: u8,
        match_id: String,
        command_id: String,
        expected_seq: u64,
    },
    RequestTakeback {
        protocol: u8,
        match_id: String,
        command_id: String,
        expected_seq: u64,
    },
    AcceptTakeback {
        protocol: u8,
        match_id: String,
        command_id: String,
        expected_seq: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CoordMovePayload {
    pub from: [u8; 2],
    pub to: [u8; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestSessionResponse {
    pub account_id: String,
    pub display_name: String,
    pub csrf_token: String,
    /// Returned for native clients using OS credential stores; web clients also receive `HttpOnly` cookie.
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInvitationRequest {
    pub preferred_side: Option<Side>,
    pub preferred_side_mode: Option<String>, // "red" | "black" | "random"
    pub clock_initial_ms: Option<i64>,       // None = untimed; Some(600_000) = 10 min default
    pub clock_increment_ms: Option<i64>,
    pub is_friendly: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationCreatedResponse {
    pub match_id: String,
    pub invitation_code: String,
    /// Shareable HTTPS path containing NO seat credentials (Section 3).
    pub share_path: String,
    pub rules_profile: String,
    pub clock_initial_ms: Option<i64>,
    pub clock_increment_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinInvitationResponse {
    pub match_id: String,
    pub assigned_side: Option<Side>, // None = spectator when both seats are already occupied
    pub snapshot: MatchSnapshotResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommittedEventRecord {
    pub seq: u64,
    pub command_id: String,
    pub actor_side: Option<String>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchSnapshotResponse {
    pub match_id: String,
    pub invitation_code: String,
    pub rules_profile: String,
    pub status: String,
    pub seq: u64,
    pub fen: String,
    pub side_to_move: Side,
    pub moves_uci: Vec<String>,
    pub moves_chinese: Vec<String>,
    pub red_seated: bool,
    pub black_seated: bool,
    pub clock_initial_ms: Option<i64>,
    pub clock_increment_ms: i64,
    pub red_remaining_ms: Option<i64>,
    pub black_remaining_ms: Option<i64>,
    pub is_friendly: bool,
    pub draw_offered_by: Option<Side>,
    pub takeback_requested_by: Option<Side>,
    pub next_rematch_invitation_code: Option<String>,
    pub outcome: Option<String>,
    pub reason_code: Option<String>,
    pub visibility: Visibility,
    pub missing_events: Vec<CommittedEventRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyUpdateRequest {
    pub visibility: Visibility,
    pub allow_internal_training: bool,
    pub allow_published_research: bool,
    pub allow_dataset_release: bool,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = Json(serde_json::json!({
            "error": {
                "code": self.code,
                "message": self.message,
            }
        }));
        (self.status, body).into_response()
    }
}

fn now_utc_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    hex::encode(h.finalize())
}

fn extract_sqlite_file_path(sqlite_url: &str) -> Option<std::path::PathBuf> {
    let rest = sqlite_url
        .strip_prefix("sqlite://")
        .or_else(|| sqlite_url.strip_prefix("sqlite:"))
        .unwrap_or(sqlite_url);
    let (path_part, query_part) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };
    if path_part.is_empty()
        || path_part == ":memory:"
        || query_part.split('&').any(|kv| kv == "mode=memory")
    {
        return None;
    }
    Some(std::path::PathBuf::from(path_part))
}

fn acquire_exclusive_sqlite_lock(
    sqlite_url: &str,
) -> Result<Option<Arc<std::fs::File>>, sqlx::Error> {
    let Some(db_path) = extract_sqlite_file_path(sqlite_url) else {
        return Ok(None);
    };
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
    }
    let mut lock_os = db_path.into_os_string();
    lock_os.push(".owner.lock");
    let lock_path = std::path::PathBuf::from(lock_os);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(sqlx::Error::Io)?;
    file.try_lock().map_err(|_| {
        sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            format!(
                "SQLite database lock {} is already held by a running match-server",
                lock_path.display()
            ),
        ))
    })?;
    Ok(Some(Arc::new(file)))
}

pub async fn build_app_state(sqlite_url: &str) -> Result<AppState, sqlx::Error> {
    // Establish exclusive ownership BEFORE opening or interrupting any stored matches!
    let owner_lock = acquire_exclusive_sqlite_lock(sqlite_url)?;
    let db = SqlitePoolOptions::new()
        .max_connections(8)
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("PRAGMA busy_timeout = 5000;")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(sqlite_url)
        .await?;
    sqlx::raw_sql(INITIAL_SCHEMA_SQL).execute(&db).await?;
    mark_interrupted_timed_matches_on_startup(&db).await?;

    Ok(AppState {
        db,
        archive: Arc::new(SampleArchiveProvider::new_embedded()),
        matches: Arc::new(RwLock::new(HashMap::new())),
        rematch_lock: Arc::new(tokio::sync::Mutex::new(())),
        allowed_origins: Arc::new(vec![
            "http://localhost:3000".to_string(),
            "http://127.0.0.1:3000".to_string(),
            "http://localhost:3180".to_string(),
            "http://127.0.0.1:3180".to_string(),
            "http://localhost:4180".to_string(),
            "http://127.0.0.1:4180".to_string(),
        ]),
        _db_owner_lock: owner_lock,
    })
}

pub fn build_router(state: AppState) -> Router {
    let origins: Vec<HeaderValue> = state
        .allowed_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            HeaderName::from_static("x-csrf-token"),
        ]);

    Router::new()
        .route("/healthz", get(healthz))
        .route(
            "/.well-known/apple-app-site-association",
            get(well_known_apple_universal_links),
        )
        .route(
            "/.well-known/assetlinks.json",
            get(well_known_android_asset_links),
        )
        .route("/api/v1/auth/guest", post(create_guest_session))
        .route("/api/v1/invitations", post(create_invitation))
        .route("/api/v1/invitations/{code}", get(inspect_invitation))
        .route("/api/v1/invitations/{code}/join", post(join_invitation))
        .route("/api/v1/matches/{match_id}", get(get_match_snapshot))
        .route(
            "/api/v1/matches/{match_id}/export.pgn",
            get(export_match_pgn),
        )
        .route(
            "/api/v1/matches/{match_id}/commands",
            post(post_match_command),
        )
        .route(
            "/api/v1/matches/{match_id}/rematch",
            post(create_or_get_rematch),
        )
        .route(
            "/api/v1/matches/{match_id}/privacy",
            post(update_match_privacy),
        )
        .route("/api/v1/matches/{match_id}/ws", get(ws_match_stream))
        .route("/api/v1/archive/players", get(archive_search_players))
        .route("/api/v1/archive/games/{game_id}", get(archive_get_game))
        .route("/api/v1/archive/positions", get(archive_query_position))
        .layer(cors)
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "rules_profile": ADOPTED_RULES_PROFILE,
    }))
}

/// Startup clock-continuity guard (Handoff Section 8, p. 21):
/// Monotonic clock origins do not survive an unexpected process restart; preserve all moves
/// and mark previously active timed matches `interrupted` without awarding a loss!
pub async fn mark_interrupted_timed_matches_on_startup(
    db: &SqlitePool,
) -> Result<u64, sqlx::Error> {
    let now = now_utc_ms();
    let rows = sqlx::query(
        "SELECT match_id, current_seq FROM matches WHERE status = 'playing' AND clock_initial_ms IS NOT NULL",
    )
    .fetch_all(db)
    .await?;

    let mut count = 0u64;
    for r in rows {
        let mid: String = r.get("match_id");
        let seq = r.get::<i64, _>("current_seq") + 1;
        let mut tx = db.begin().await?;
        sqlx::query(
            "INSERT INTO match_events (match_id, seq, command_id, actor_side, event_type, payload_json, created_at_ms)
             VALUES (?, ?, ?, 'system', 'interrupted', '{\"reason\":\"server_interruption\"}', ?)",
        )
        .bind(&mid)
        .bind(seq)
        .bind(format!("startup_interrupt_seq_{seq}"))
        .bind(now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE matches SET current_seq = ?, status = 'interrupted', outcome = 'interrupted',
             reason_code = 'server_interruption', turn_started_utc_ms = NULL, updated_at_ms = ?
             WHERE match_id = ?",
        )
        .bind(seq)
        .bind(now)
        .bind(&mid)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        count += 1;
    }
    Ok(count)
}

async fn well_known_apple_universal_links() -> impl IntoResponse {
    let app_id = std::env::var("JADE_APPLE_APP_ID")
        .unwrap_or_else(|_| "PROVISIONAL.play.jade.ios.dev".to_string());
    let mac_id = std::env::var("JADE_MACOS_APP_ID")
        .unwrap_or_else(|_| "PROVISIONAL.play.jade.macos.dev".to_string());
    Json(serde_json::json!({
        "applinks": {
            "details": [
                {
                    "appIDs": [app_id, mac_id],
                    "components": [
                        { "/": "/invite/*", "comment": "Friendly match invitations" },
                        { "/": "/g/*", "comment": "Shared game replays" },
                        { "/": "/archive*", "comment": "Archive and position explorer" },
                        { "/": "/players/*", "comment": "Historical player profiles" }
                    ]
                }
            ]
        }
    }))
}

async fn well_known_android_asset_links() -> impl IntoResponse {
    let pkg =
        std::env::var("JADE_ANDROID_PACKAGE").unwrap_or_else(|_| "play.jade.app.dev".to_string());
    let cert = std::env::var("JADE_ANDROID_SHA256_CERT")
        .or_else(|_| std::env::var("JADE_ANDROID_CERT_SHA256"))
        .unwrap_or_else(|_| {
            "00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00:00"
                .to_string()
        });
    Json(serde_json::json!([
        {
            "relation": ["delegate_permission/common.handle_all_urls"],
            "target": {
                "namespace": "android_app",
                "package_name": pkg,
                "sha256_cert_fingerprints": [cert]
            }
        }
    ]))
}

#[derive(Deserialize)]
struct GuestInitBody {
    display_name: Option<String>,
    transport: Option<String>, // "web_cookie" | "native_token"
}

async fn create_guest_session(
    State(state): State<AppState>,
    Json(body): Json<GuestInitBody>,
) -> Result<impl IntoResponse, ApiError> {
    let account_id = format!("acct_{}", Uuid::new_v4().simple());
    let display_name = body
        .display_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Guest Player".to_string());
    let transport = body.transport.unwrap_or_else(|| "web_cookie".to_string());

    let raw_token = format!(
        "jtok_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let refresh_token = format!(
        "jref_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let csrf_token = format!("jcsrf_{}", Uuid::new_v4().simple());
    let ts = now_utc_ms();

    sqlx::query(
        "INSERT INTO accounts (account_id, display_name, is_guest, created_at_ms) VALUES (?, ?, 1, ?)",
    )
    .bind(&account_id)
    .bind(&display_name)
    .bind(ts)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    sqlx::query(
        "INSERT INTO auth_sessions (session_id_hash, account_id, transport_kind, csrf_token, refresh_token_hash, expires_at_ms, created_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(sha256_hex(&raw_token))
    .bind(&account_id)
    .bind(&transport)
    .bind(&csrf_token)
    .bind(sha256_hex(&refresh_token))
    .bind(ts + 7 * 24 * 3600 * 1000)
    .bind(ts)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let cookie_val = format!("jade_session={raw_token}; HttpOnly; Secure; SameSite=Lax; Path=/");
    let mut headers = HeaderMap::new();
    headers.insert(axum::http::header::SET_COOKIE, cookie_val.parse().unwrap());

    Ok((
        headers,
        Json(GuestSessionResponse {
            account_id,
            display_name,
            csrf_token,
            access_token: raw_token,
            refresh_token,
        }),
    ))
}

async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
    require_csrf_on_cookie: bool,
) -> Result<String, ApiError> {
    // 1. Native / explicit Bearer token (`Authorization: Bearer <token>`)
    if let Some(auth_hdr) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        && let Some(tok) = auth_hdr.strip_prefix("Bearer ")
    {
        return lookup_session_account(state, tok.trim(), None).await;
    }

    // 2. Web `HttpOnly` cookie (`jade_session=...`) + mandatory `X-CSRF-Token` on HTTP mutations!
    if let Some(cookie_hdr) = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
    {
        for part in cookie_hdr.split(';') {
            if let Some(tok) = part.trim().strip_prefix("jade_session=") {
                let csrf = if require_csrf_on_cookie {
                    let provided = headers
                        .get("x-csrf-token")
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| ApiError {
                            status: StatusCode::FORBIDDEN,
                            code: "csrf_missing",
                            message: "Cookie-authenticated mutation requires X-CSRF-Token header"
                                .to_string(),
                        })?;
                    Some(provided)
                } else {
                    None
                };
                return lookup_session_account(state, tok, csrf).await;
            }
        }
    }

    Err(ApiError {
        status: StatusCode::UNAUTHORIZED,
        code: "unauthenticated",
        message: "Missing session credential".to_string(),
    })
}

async fn lookup_session_account(
    state: &AppState,
    raw_token: &str,
    expected_csrf: Option<&str>,
) -> Result<String, ApiError> {
    let row = sqlx::query(
        "SELECT account_id, csrf_token, revoked_at_ms, expires_at_ms FROM auth_sessions WHERE session_id_hash = ?",
    )
    .bind(sha256_hex(raw_token))
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?
    .ok_or_else(|| ApiError {
        status: StatusCode::UNAUTHORIZED,
        code: "invalid_session",
        message: "Session not found".to_string(),
    })?;

    let revoked: Option<i64> = row.get("revoked_at_ms");
    let expires: i64 = row.get("expires_at_ms");
    if revoked.is_some() || expires < now_utc_ms() {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            code: "session_expired_or_revoked",
            message: "Session expired or revoked".to_string(),
        });
    }
    if let Some(csrf_check) = expected_csrf {
        let actual_csrf: String = row.get("csrf_token");
        if actual_csrf != csrf_check {
            return Err(ApiError {
                status: StatusCode::FORBIDDEN,
                code: "csrf_mismatch",
                message: "CSRF token mismatch".to_string(),
            });
        }
    }
    Ok(row.get("account_id"))
}

async fn create_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateInvitationRequest>,
) -> Result<Json<InvitationCreatedResponse>, ApiError> {
    let creator_id = authenticate_request(&state, &headers, true).await?;
    let match_id = format!("m_{}", Uuid::new_v4().simple());
    let invitation_code = Uuid::new_v4().simple().to_string()[..10].to_string();

    // Friendly default is 10 minutes + 5 seconds increment; explicit null initial_ms means untimed!
    let clock_initial_ms = req.clock_initial_ms;
    let clock_increment_ms = req
        .clock_increment_ms
        .unwrap_or(if clock_initial_ms.is_some() { 5_000 } else { 0 });
    let side = match req.preferred_side_mode.as_deref() {
        Some("random") => {
            if Uuid::new_v4().as_bytes()[0] & 1 == 0 {
                Side::Red
            } else {
                Side::Black
            }
        }
        Some("black") => Side::Black,
        Some("red") => Side::Red,
        _ => req.preferred_side.unwrap_or(Side::Red),
    };
    let (red_acct, blk_acct) = match side {
        Side::Red => (Some(creator_id.clone()), None),
        Side::Black => (None, Some(creator_id.clone())),
    };

    let ts = now_utc_ms();
    let start_fen = xiangqi_core::STARTING_FEN;
    sqlx::query(
        "INSERT INTO matches (
            match_id, invitation_code, rules_profile, is_friendly, initial_fen, current_fen,
            current_seq, status, red_account_id, black_account_id, clock_initial_ms, clock_increment_ms,
            red_remaining_ms, black_remaining_ms, visibility, created_at_ms, updated_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, 0, 'waiting', ?, ?, ?, ?, ?, ?, 'private', ?, ?)",
    )
    .bind(&match_id)
    .bind(&invitation_code)
    .bind(ADOPTED_RULES_PROFILE)
    .bind(if req.is_friendly.unwrap_or(true) { 1 } else { 0 })
    .bind(start_fen)
    .bind(start_fen)
    .bind(&red_acct)
    .bind(&blk_acct)
    .bind(clock_initial_ms)
    .bind(clock_increment_ms)
    .bind(clock_initial_ms)
    .bind(clock_initial_ms)
    .bind(ts)
    .bind(ts)
    .execute(&state.db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    ensure_match_actor(&state, &match_id).await?;

    Ok(Json(InvitationCreatedResponse {
        match_id,
        share_path: format!("/invite/{invitation_code}"),
        invitation_code,
        rules_profile: ADOPTED_RULES_PROFILE.to_string(),
        clock_initial_ms,
        clock_increment_ms,
    }))
}

async fn inspect_invitation(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let row = sqlx::query(
        "SELECT match_id, rules_profile, status, clock_initial_ms, clock_increment_ms, red_account_id, black_account_id FROM matches WHERE invitation_code = ?",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?
    .ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        code: "invitation_not_found",
        message: "Invitation not found".to_string(),
    })?;

    let red: Option<String> = row.get("red_account_id");
    let blk: Option<String> = row.get("black_account_id");
    Ok(Json(serde_json::json!({
        "invitation_code": code,
        "match_id": row.get::<String, _>("match_id"),
        "rules_profile": row.get::<String, _>("rules_profile"),
        "status": row.get::<String, _>("status"),
        "clock_initial_ms": row.get::<Option<i64>, _>("clock_initial_ms"),
        "clock_increment_ms": row.get::<i64, _>("clock_increment_ms"),
        "red_occupied": red.is_some(),
        "black_occupied": blk.is_some(),
    })))
}

async fn join_invitation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<JoinInvitationResponse>, ApiError> {
    let joiner_id = authenticate_request(&state, &headers, true).await?;
    let row = sqlx::query(
        "SELECT match_id, red_account_id, black_account_id, status, rematch_of_match_id FROM matches WHERE invitation_code = ?",
    )
    .bind(&code)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?
    .ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        code: "invitation_not_found",
        message: "Invitation not found".to_string(),
    })?;

    let match_id: String = row.get("match_id");
    let red_id: Option<String> = row.get("red_account_id");
    let blk_id: Option<String> = row.get("black_account_id");
    let rematch_of_id: Option<String> = row.get("rematch_of_match_id");

    // If this invitation is a linked rematch, reserve the open seat strictly for the original match's participants!
    let eligible_to_claim_seat = if let Some(ref orig_mid) = rematch_of_id {
        if let Some(orig_row) =
            sqlx::query("SELECT red_account_id, black_account_id FROM matches WHERE match_id = ?")
                .bind(orig_mid)
                .fetch_optional(&state.db)
                .await
                .map_err(|e| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    code: "db_error",
                    message: e.to_string(),
                })?
        {
            let orig_red: Option<String> = orig_row.get("red_account_id");
            let orig_blk: Option<String> = orig_row.get("black_account_id");
            orig_red.as_deref() == Some(joiner_id.as_str())
                || orig_blk.as_deref() == Some(joiner_id.as_str())
        } else {
            false
        }
    } else {
        true
    };

    // Opening a link never overwrites an already occupied seat (Section 3)!
    let mut transitioned_to_playing = false;
    let assigned_side = if red_id.as_deref() == Some(joiner_id.as_str()) {
        Some(Side::Red)
    } else if blk_id.as_deref() == Some(joiner_id.as_str()) {
        Some(Side::Black)
    } else if !eligible_to_claim_seat {
        None
    } else if red_id.is_none() {
        let now = now_utc_ms();
        let next_status = if blk_id.is_some() {
            "playing"
        } else {
            "waiting"
        };
        let turn_utc = if next_status == "playing" {
            Some(now)
        } else {
            None
        };
        let res = sqlx::query(
            "UPDATE matches SET red_account_id = ?, status = ?, turn_started_utc_ms = COALESCE(turn_started_utc_ms, ?), updated_at_ms = ? WHERE match_id = ? AND red_account_id IS NULL",
        )
        .bind(&joiner_id)
        .bind(next_status)
        .bind(turn_utc)
        .bind(now)
        .bind(&match_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;
        if res.rows_affected() == 1 {
            transitioned_to_playing = next_status == "playing";
            Some(Side::Red)
        } else {
            None
        }
    } else if blk_id.is_none() {
        let now = now_utc_ms();
        let next_status = if red_id.is_some() {
            "playing"
        } else {
            "waiting"
        };
        let turn_utc = if next_status == "playing" {
            Some(now)
        } else {
            None
        };
        let res = sqlx::query(
            "UPDATE matches SET black_account_id = ?, status = ?, turn_started_utc_ms = COALESCE(turn_started_utc_ms, ?), updated_at_ms = ? WHERE match_id = ? AND black_account_id IS NULL",
        )
        .bind(&joiner_id)
        .bind(next_status)
        .bind(turn_utc)
        .bind(now)
        .bind(&match_id)
        .execute(&state.db)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;
        if res.rows_affected() == 1 {
            transitioned_to_playing = next_status == "playing";
            Some(Side::Black)
        } else {
            None
        }
    } else {
        None
    };

    if assigned_side.is_none() {
        check_match_read_access(&state.db, &match_id, Some(&joiner_id)).await?;
    }

    let actor = ensure_match_actor(&state, &match_id).await?;
    if transitioned_to_playing {
        let _ = actor
            .tx
            .send(ActorMsg::StartPlaying {
                started_at: Instant::now(),
            })
            .await;
    }
    let snapshot = load_match_snapshot(&state.db, &match_id, None).await?;
    if assigned_side.is_some() {
        let _ = actor.bcast.send(snapshot.clone());
    }
    let snapshot = redact_snapshot_for_viewer(&state.db, snapshot, Some(&joiner_id)).await;
    Ok(Json(JoinInvitationResponse {
        match_id,
        assigned_side,
        snapshot,
    }))
}

#[derive(Deserialize)]
struct ReconnectQuery {
    since_seq: Option<u64>,
}

async fn get_match_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(match_id): Path<String>,
    Query(q): Query<ReconnectQuery>,
) -> Result<Json<MatchSnapshotResponse>, ApiError> {
    let viewer = authenticate_request(&state, &headers, false).await.ok();
    // Enforce private-by-default access check (Section 3 & 9)!
    check_match_read_access(&state.db, &match_id, viewer.as_deref()).await?;
    let snap = load_match_snapshot(&state.db, &match_id, q.since_seq).await?;
    let snap = redact_snapshot_for_viewer(&state.db, snap, viewer.as_deref()).await;
    Ok(Json(snap))
}

async fn is_match_participant(
    db: &SqlitePool,
    match_id: &str,
    viewer_account_id: Option<&str>,
) -> bool {
    let Some(v) = viewer_account_id else {
        return false;
    };
    let Ok(Some(row)) =
        sqlx::query("SELECT red_account_id, black_account_id FROM matches WHERE match_id = ?")
            .bind(match_id)
            .fetch_optional(db)
            .await
    else {
        return false;
    };
    let red: Option<String> = row.get("red_account_id");
    let blk: Option<String> = row.get("black_account_id");
    red.as_deref() == Some(v) || blk.as_deref() == Some(v)
}

async fn redact_snapshot_for_viewer(
    db: &SqlitePool,
    mut snap: MatchSnapshotResponse,
    viewer_account_id: Option<&str>,
) -> MatchSnapshotResponse {
    if snap.next_rematch_invitation_code.is_some()
        && !is_match_participant(db, &snap.match_id, viewer_account_id).await
    {
        snap.next_rematch_invitation_code = None;
    }
    snap
}

async fn check_match_read_access(
    db: &SqlitePool,
    match_id: &str,
    viewer_account_id: Option<&str>,
) -> Result<(), ApiError> {
    let row = sqlx::query(
        "SELECT visibility, red_account_id, black_account_id FROM matches WHERE match_id = ?",
    )
    .bind(match_id)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?
    .ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        code: "match_not_found",
        message: "Match not found".to_string(),
    })?;

    let vis: String = row.get("visibility");
    if vis == "public" || vis == "unlisted" {
        return Ok(());
    }
    let red: Option<String> = row.get("red_account_id");
    let blk: Option<String> = row.get("black_account_id");
    if let Some(v) = viewer_account_id
        && (red.as_deref() == Some(v) || blk.as_deref() == Some(v))
    {
        return Ok(());
    }
    Err(ApiError {
        status: StatusCode::FORBIDDEN,
        code: "private_match",
        message:
            "Personal matches are private by default unless explicitly shared by both participants"
                .to_string(),
    })
}

async fn post_match_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(match_id): Path<String>,
    Json(cmd): Json<MatchCommandRequest>,
) -> Result<Json<MatchSnapshotResponse>, ApiError> {
    let actor_account_id = authenticate_request(&state, &headers, true).await?;
    let actor = ensure_match_actor(&state, &match_id).await?;
    let (reply_tx, reply_rx) = oneshot::channel();
    actor
        .tx
        .send(ActorMsg::Command(MailboxEnvelope {
            actor_account_id,
            command: cmd,
            received_at: Instant::now(),
            received_utc_ms: now_utc_ms(),
            reply: reply_tx,
        }))
        .await
        .map_err(|_| ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "mailbox_closed",
            message: "Match command stream unavailable".to_string(),
        })?;

    let res = reply_rx.await.map_err(|_| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "mailbox_dropped",
        message: "Match dispatcher dropped response".to_string(),
    })??;
    Ok(Json(res))
}

async fn update_match_privacy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(match_id): Path<String>,
    Json(req): Json<PrivacyUpdateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = authenticate_request(&state, &headers, true).await?;
    let parse_vis = |s: &str| match s {
        "public" => Visibility::Public,
        "unlisted" => Visibility::Unlisted,
        _ => Visibility::Private,
    };
    let fmt_vis = |v: Visibility| match v {
        Visibility::Public => "public",
        Visibility::Unlisted => "unlisted",
        Visibility::Private => "private",
    };

    let mut tx = state.db.begin().await.map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let seats =
        sqlx::query("SELECT red_account_id, black_account_id FROM matches WHERE match_id = ?")
            .bind(&match_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "db_error",
                message: e.to_string(),
            })?
            .ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                code: "match_not_found",
                message: "Match not found".to_string(),
            })?;

    let red: Option<String> = seats.get("red_account_id");
    let blk: Option<String> = seats.get("black_account_id");
    let now = now_utc_ms();

    // Update ONLY the caller's columns inside the transaction so concurrent updates never clobber the other player's revocations!
    if red.as_deref() == Some(actor.as_str()) {
        sqlx::query(
            "UPDATE matches SET
                red_visibility_choice = ?,
                red_consent_training = ?,
                red_consent_research = ?,
                red_consent_dataset = ?,
                updated_at_ms = ?
             WHERE match_id = ?",
        )
        .bind(fmt_vis(req.visibility))
        .bind(i64::from(req.allow_internal_training))
        .bind(i64::from(req.allow_published_research))
        .bind(i64::from(req.allow_dataset_release))
        .bind(now)
        .bind(&match_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;
    } else if blk.as_deref() == Some(actor.as_str()) {
        sqlx::query(
            "UPDATE matches SET
                black_visibility_choice = ?,
                black_consent_training = ?,
                black_consent_research = ?,
                black_consent_dataset = ?,
                updated_at_ms = ?
             WHERE match_id = ?",
        )
        .bind(fmt_vis(req.visibility))
        .bind(i64::from(req.allow_internal_training))
        .bind(i64::from(req.allow_published_research))
        .bind(i64::from(req.allow_dataset_release))
        .bind(now)
        .bind(&match_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;
    } else {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "not_a_participant",
            message: "Only match participants can update match privacy or consent".to_string(),
        });
    }

    let row = sqlx::query(
        "SELECT red_visibility_choice, black_visibility_choice,
                red_consent_training, black_consent_training,
                red_consent_research, black_consent_research,
                red_consent_dataset, black_consent_dataset
         FROM matches WHERE match_id = ?",
    )
    .bind(&match_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let policy = MatchPrivacyConsent {
        red_visibility: parse_vis(&row.get::<String, _>("red_visibility_choice")),
        black_visibility: parse_vis(&row.get::<String, _>("black_visibility_choice")),
        red_allow_internal_training: row.get::<i64, _>("red_consent_training") != 0,
        black_allow_internal_training: row.get::<i64, _>("black_consent_training") != 0,
        red_allow_published_research: row.get::<i64, _>("red_consent_research") != 0,
        black_allow_published_research: row.get::<i64, _>("black_consent_research") != 0,
        red_allow_dataset_release: row.get::<i64, _>("red_consent_dataset") != 0,
        black_allow_dataset_release: row.get::<i64, _>("black_consent_dataset") != 0,
    };
    let effective = policy.effective_visibility();

    sqlx::query("UPDATE matches SET visibility = ?, updated_at_ms = ? WHERE match_id = ?")
        .bind(fmt_vis(effective))
        .bind(now)
        .bind(&match_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;

    tx.commit().await.map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    // Broadcast the updated visibility so nonparticipant spectator WebSockets revalidate & disconnect immediately if revoked!
    if let Ok(actor_handle) = ensure_match_actor(&state, &match_id).await
        && let Ok(snap) = load_match_snapshot(&state.db, &match_id, None).await
    {
        let _ = actor_handle.bcast.send(snap);
    }

    Ok(Json(serde_json::json!({
        "effective_visibility": effective,
        "eligible_for_internal_training": policy.eligible_for_internal_training(),
        "eligible_for_published_research": policy.eligible_for_published_research(),
        "eligible_for_dataset_release": policy.eligible_for_dataset_release(),
    })))
}

async fn ws_match_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(match_id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, ApiError> {
    // Validate Origin when present (browser WebSocket), and require explicit authentication (missing Origin is NEVER authentication — Section 8)!
    if let Some(origin) = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        && !state.allowed_origins.iter().any(|o| o == origin)
    {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "invalid_ws_origin",
            message: format!("Disallowed WebSocket Origin: {origin}"),
        });
    }
    let account_id = authenticate_request(&state, &headers, false).await?;
    check_match_read_access(&state.db, &match_id, Some(&account_id)).await?;
    let actor = ensure_match_actor(&state, &match_id).await?;
    let init_snap = load_match_snapshot(&state.db, &match_id, None).await?;
    let init_snap = redact_snapshot_for_viewer(&state.db, init_snap, Some(&account_id)).await;

    let db = state.db.clone();
    Ok(ws.on_upgrade(move |socket| handle_ws_client(socket, db, actor, account_id, init_snap)))
}

async fn handle_ws_client(
    mut socket: WebSocket,
    db: SqlitePool,
    actor: MatchActorHandle,
    account_id: String,
    initial_snapshot: MatchSnapshotResponse,
) {
    let mut bcast_rx = actor.bcast.subscribe();
    let _ = socket
        .send(Message::Text(
            serde_json::to_string(&initial_snapshot).unwrap().into(),
        ))
        .await;

    loop {
        tokio::select! {
            bmsg = bcast_rx.recv() => {
                let Ok(snap) = bmsg else {
                    break;
                };
                // Revalidate read access on every broadcast so spectators disconnect immediately if privacy is restored!
                if check_match_read_access(&db, &snap.match_id, Some(&account_id)).await.is_err() {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                let snap = redact_snapshot_for_viewer(&db, snap, Some(&account_id)).await;
                if let Ok(json) = serde_json::to_string(&snap)
                    && socket.send(Message::Text(json.into())).await.is_err()
                {
                    break;
                }
            }
            incoming = socket.recv() => {
                let Some(Ok(msg)) = incoming else {
                    break;
                };
                if let Message::Text(txt) = msg
                    && let Ok(cmd) = serde_json::from_str::<MatchCommandRequest>(&txt)
                {
                    let (rtx, rrx) = oneshot::channel();
                    if actor
                        .tx
                        .send(ActorMsg::Command(MailboxEnvelope {
                            actor_account_id: account_id.clone(),
                            command: cmd,
                            received_at: Instant::now(),
                            received_utc_ms: now_utc_ms(),
                            reply: rtx,
                        }))
                        .await
                        .is_ok()
                        && let Ok(Err(err)) = rrx.await
                    {
                        let out = serde_json::json!({
                            "error": { "code": err.code, "message": err.message }
                        })
                        .to_string();
                        if socket.send(Message::Text(out.into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }
}

async fn ensure_match_actor(
    state: &AppState,
    match_id: &str,
) -> Result<MatchActorHandle, ApiError> {
    {
        let guard = state.matches.read().await;
        if let Some(handle) = guard.get(match_id)
            && !handle.tx.is_closed()
        {
            return Ok(handle.clone());
        }
    }

    let mut write_guard = state.matches.write().await;
    if let Some(handle) = write_guard.get(match_id)
        && !handle.tx.is_closed()
    {
        return Ok(handle.clone());
    }

    // Reconstruct authoritative GameState, pending offers, and clock instant from SQLite/Postgres!
    let (mut game_state, initial_turn_instant, initial_draw_offer, initial_takeback_offer) =
        reconstruct_game_from_db(&state.db, match_id).await?;

    // Bounded command mailbox per active match (Section 8).
    let (tx, mut rx) = mpsc::channel::<ActorMsg>(64);
    let (bcast_tx, _) = broadcast::channel::<MatchSnapshotResponse>(64);
    let bcast_for_loop = bcast_tx.clone();
    let db = state.db.clone();
    let mid = match_id.to_string();

    tokio::spawn(async move {
        let mut turn_started_mono: Option<Instant> = initial_turn_instant;
        let mut draw_offered_by: Option<Side> = initial_draw_offer;
        let mut takeback_requested_by: Option<Side> = initial_takeback_offer;
        let mut active_deadline =
            compute_actor_deadline(&db, &mid, game_state.side_to_move(), turn_started_mono).await;

        loop {
            let dl_opt = active_deadline;
            tokio::select! {
                msg_opt = rx.recv() => {
                    let Some(msg) = msg_opt else {
                        break;
                    };
                    match msg {
                        ActorMsg::StartPlaying { started_at } => {
                            if turn_started_mono.is_none() {
                                turn_started_mono = Some(started_at);
                            }
                            active_deadline = compute_actor_deadline(
                                &db,
                                &mid,
                                game_state.side_to_move(),
                                turn_started_mono,
                            )
                            .await;
                        }
                        ActorMsg::Command(env) => {
                            let res = process_single_command(
                                &db,
                                &mid,
                                &mut game_state,
                                &mut turn_started_mono,
                                &mut draw_offered_by,
                                &mut takeback_requested_by,
                                &env.actor_account_id,
                                env.command,
                                env.received_at,
                                env.received_utc_ms,
                            )
                            .await;
                            active_deadline = compute_actor_deadline(
                                &db,
                                &mid,
                                game_state.side_to_move(),
                                turn_started_mono,
                            )
                            .await;
                            if let Ok(ref snap) = res {
                                let _ = bcast_for_loop.send(snap.clone());
                            }
                            let _ = env.reply.send(res);
                        }
                    }
                }
                _ = async {
                    match dl_opt {
                        Some(dl) => tokio::time::sleep_until(dl).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    if let Ok(Some(snap)) = expire_active_clock_if_due(
                        &db,
                        &mid,
                        &game_state,
                        &mut turn_started_mono,
                        &mut draw_offered_by,
                    )
                    .await
                    {
                        takeback_requested_by = None;
                        active_deadline = None;
                        let _ = bcast_for_loop.send(snap);
                    } else {
                        active_deadline = compute_actor_deadline(
                            &db,
                            &mid,
                            game_state.side_to_move(),
                            turn_started_mono,
                        )
                        .await;
                    }
                }
            }
        }
    });

    let handle = MatchActorHandle {
        tx,
        bcast: bcast_tx,
    };
    write_guard.insert(match_id.to_string(), handle.clone());
    Ok(handle)
}

async fn compute_actor_deadline(
    db: &SqlitePool,
    match_id: &str,
    side_to_move: Side,
    turn_started_mono: Option<Instant>,
) -> Option<tokio::time::Instant> {
    let started = turn_started_mono?;
    let row = sqlx::query(
        "SELECT status, red_remaining_ms, black_remaining_ms FROM matches WHERE match_id = ?",
    )
    .bind(match_id)
    .fetch_optional(db)
    .await
    .ok()??;

    let status: String = row.get("status");
    if status != "playing" {
        return None;
    }
    let rem_ms: Option<i64> = match side_to_move {
        Side::Red => row.get("red_remaining_ms"),
        Side::Black => row.get("black_remaining_ms"),
    };
    let rem = rem_ms?;
    Some(tokio::time::Instant::from_std(started) + Duration::from_millis(rem.max(0) as u64))
}

async fn expire_active_clock_if_due(
    db: &SqlitePool,
    match_id: &str,
    game: &GameState,
    turn_started_mono: &mut Option<Instant>,
    draw_offered_by: &mut Option<Side>,
) -> Result<Option<MatchSnapshotResponse>, ApiError> {
    let Some(started) = *turn_started_mono else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT current_seq, status, red_remaining_ms, black_remaining_ms FROM matches WHERE match_id = ?",
    )
    .bind(match_id)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;
    let Some(mrow) = row else {
        return Ok(None);
    };
    let status: String = mrow.get("status");
    if status != "playing" {
        *turn_started_mono = None;
        return Ok(None);
    }

    let current_seq = mrow.get::<i64, _>("current_seq") as u64;
    let mut red_rem: Option<i64> = mrow.get("red_remaining_ms");
    let mut blk_rem: Option<i64> = mrow.get("black_remaining_ms");
    let stm = game.side_to_move();
    let elapsed_ms = Instant::now()
        .saturating_duration_since(started)
        .as_millis() as i64;
    let active_rem = match stm {
        Side::Red => &mut red_rem,
        Side::Black => &mut blk_rem,
    };
    let Some(rem) = active_rem else {
        return Ok(None);
    };
    if *rem - elapsed_ms > 0 {
        return Ok(None);
    }

    *rem = 0;
    let winner = match stm.opposite() {
        Side::Red => "red_win",
        Side::Black => "black_win",
    };
    let cmd_id = format!("auto_timeout_seq_{}", current_seq + 1);
    commit_match_event_tx(
        db,
        match_id,
        current_seq + 1,
        &cmd_id,
        Some("system"),
        "timeout",
        &serde_json::json!({ "timed_out": stm.as_str() }),
        &game.to_fen(),
        "finished",
        red_rem,
        blk_rem,
        Some(winner),
        Some("timeout"),
        now_utc_ms(),
    )
    .await?;
    *turn_started_mono = None;
    *draw_offered_by = None;
    load_match_snapshot(db, match_id, None).await.map(Some)
}

async fn reconstruct_game_from_db(
    db: &SqlitePool,
    match_id: &str,
) -> Result<(GameState, Option<Instant>, Option<Side>, Option<Side>), ApiError> {
    let mrow = sqlx::query(
        "SELECT initial_fen, rules_profile, status, turn_started_utc_ms FROM matches WHERE match_id = ?",
    )
    .bind(match_id)
    .fetch_optional(db)
    .await
    .map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "db_error",
                message: e.to_string(),
            })?
            .ok_or_else(|| ApiError {
                status: StatusCode::NOT_FOUND,
                code: "match_not_found",
                message: "Match not found".to_string(),
            })?;

    let initial_fen: String = mrow.get("initial_fen");
    let profile: String = mrow.get("rules_profile");
    let status: String = mrow.get("status");
    let mut game = GameState::from_fen(&initial_fen, &profile).map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "corrupt_match_state",
        message: e.to_string(),
    })?;

    let events = sqlx::query(
        "SELECT event_type, payload_json FROM match_events WHERE match_id = ? ORDER BY seq ASC",
    )
    .bind(match_id)
    .fetch_all(db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let mut pending_draw: Option<Side> = None;
    let mut pending_takeback: Option<Side> = None;
    for ev in events {
        let etype: String = ev.get("event_type");
        let pjson: String = ev.get("payload_json");
        let payload = serde_json::from_str::<serde_json::Value>(&pjson).unwrap_or_default();
        if etype == "move" {
            pending_draw = None;
            pending_takeback = None;
            if let Some(uci) = payload["move_uci"].as_str()
                && let Ok(mv) = Move::from_uci_or_iccs(uci)
            {
                let _ = game.apply_move(mv);
            }
        } else if etype == "draw_offered" {
            pending_draw = match payload["offered_by"].as_str() {
                Some("red") => Some(Side::Red),
                Some("black") => Some(Side::Black),
                _ => None,
            };
        } else if etype == "takeback_requested" {
            pending_takeback = match payload["requested_by"].as_str() {
                Some("red") => Some(Side::Red),
                Some("black") => Some(Side::Black),
                _ => None,
            };
        } else if etype == "takeback_accepted" {
            pending_draw = None;
            pending_takeback = None;
            let _ = game.undo_move();
        } else {
            pending_draw = None;
            pending_takeback = None;
        }
    }

    let mono = if status == "playing" {
        let already_elapsed_ms = mrow
            .get::<Option<i64>, _>("turn_started_utc_ms")
            .map(|t| (now_utc_ms() - t).max(0) as u64)
            .unwrap_or(0);
        Instant::now()
            .checked_sub(Duration::from_millis(already_elapsed_ms))
            .or_else(|| Some(Instant::now()))
    } else {
        None
    };
    Ok((game, mono, pending_draw, pending_takeback))
}

#[allow(clippy::too_many_arguments)]
async fn process_single_command(
    db: &SqlitePool,
    match_id: &str,
    game: &mut GameState,
    turn_started_mono: &mut Option<Instant>,
    draw_offered_by: &mut Option<Side>,
    takeback_requested_by: &mut Option<Side>,
    actor_account_id: &str,
    command: MatchCommandRequest,
    received_at: Instant,
    received_utc_ms: i64,
) -> Result<MatchSnapshotResponse, ApiError> {
    let (cmd_id, expected_seq) = match &command {
        MatchCommandRequest::Move {
            command_id,
            expected_seq,
            ..
        }
        | MatchCommandRequest::Resign {
            command_id,
            expected_seq,
            ..
        }
        | MatchCommandRequest::OfferDraw {
            command_id,
            expected_seq,
            ..
        }
        | MatchCommandRequest::AcceptDraw {
            command_id,
            expected_seq,
            ..
        }
        | MatchCommandRequest::RequestTakeback {
            command_id,
            expected_seq,
            ..
        }
        | MatchCommandRequest::AcceptTakeback {
            command_id,
            expected_seq,
            ..
        } => (command_id.as_str(), *expected_seq),
    };

    // 1. Idempotency check: if `command_id` was ALREADY committed on this match, return current snapshot
    //    without applying move twice or awarding extra clock increment (Section 8)!
    let existing =
        sqlx::query("SELECT seq FROM match_events WHERE match_id = ? AND command_id = ?")
            .bind(match_id)
            .bind(cmd_id)
            .fetch_optional(db)
            .await
            .map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "db_error",
                message: e.to_string(),
            })?;
    if existing.is_some() {
        return load_match_snapshot(db, match_id, None).await;
    }

    // 2. Load current match seats, sequence, status, and clocks
    let mrow = sqlx::query(
        "SELECT current_seq, status, is_friendly, red_account_id, black_account_id, clock_initial_ms, clock_increment_ms, red_remaining_ms, black_remaining_ms
         FROM matches WHERE match_id = ?",
    )
    .bind(match_id)
    .fetch_one(db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let current_seq = mrow.get::<i64, _>("current_seq") as u64;
    let status: String = mrow.get("status");
    let is_friendly = mrow.get::<i64, _>("is_friendly") != 0;
    let red_acct: Option<String> = mrow.get("red_account_id");
    let blk_acct: Option<String> = mrow.get("black_account_id");
    let clock_inc: i64 = mrow.get("clock_increment_ms");
    let mut red_rem: Option<i64> = mrow.get("red_remaining_ms");
    let mut blk_rem: Option<i64> = mrow.get("black_remaining_ms");

    // 3. Resolve actor seat (spectators have zero player authority!)
    let actor_side = if red_acct.as_deref() == Some(actor_account_id) {
        Side::Red
    } else if blk_acct.as_deref() == Some(actor_account_id) {
        Side::Black
    } else {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "spectator_no_authority",
            message: "Spectators cannot submit match commands".to_string(),
        });
    };

    if status != "playing" {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "match_not_playing",
            message: format!("Match is currently in '{status}' state"),
        });
    }

    // 4. Stale/mismatched sequence check
    if expected_seq != current_seq {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "sequence_mismatch",
            message: format!("Expected seq {current_seq}, client sent {expected_seq}"),
        });
    }

    // 5. Monotonic clock check at receipt time (`received_at`) before accepting move!
    let stm = game.side_to_move();
    if let Some(started) = *turn_started_mono {
        let elapsed_ms = received_at.saturating_duration_since(started).as_millis() as i64;
        let active_rem = match stm {
            Side::Red => &mut red_rem,
            Side::Black => &mut blk_rem,
        };
        if let Some(rem) = active_rem {
            *rem -= elapsed_ms;
            if *rem <= 0 {
                *rem = 0;
                // Flag fall timeout!
                let winner = match stm.opposite() {
                    Side::Red => "red_win",
                    Side::Black => "black_win",
                };
                commit_match_event_tx(
                    db,
                    match_id,
                    current_seq + 1,
                    cmd_id,
                    Some("system"),
                    "timeout",
                    &serde_json::json!({ "timed_out": stm.as_str() }),
                    &game.to_fen(),
                    "finished",
                    red_rem,
                    blk_rem,
                    Some(winner),
                    Some("timeout"),
                    received_utc_ms,
                )
                .await?;
                *turn_started_mono = None;
                return load_match_snapshot(db, match_id, None).await;
            }
        }
    }

    match command {
        MatchCommandRequest::Move { mv, .. } => {
            if actor_side != stm {
                return Err(ApiError {
                    status: StatusCode::CONFLICT,
                    code: "not_your_turn",
                    message: format!("It is {}'s turn", stm.as_str()),
                });
            }
            let (Some(from), Some(to)) = (
                xiangqi_core::Square::from_file_rank(mv.from[0], mv.from[1]),
                xiangqi_core::Square::from_file_rank(mv.to[0], mv.to[1]),
            ) else {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    code: "invalid_coordinates",
                    message: "Coordinates out of 9x10 bounds".to_string(),
                });
            };
            let cmv = Move::new(from, to);
            let mut staged_game = game.clone();
            let zh =
                format_chinese_move(staged_game.board(), cmv, false).map_err(|e| ApiError {
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                    code: "illegal_move",
                    message: e.to_string(),
                })?;
            let verdict = staged_game.apply_move(cmv).map_err(|e| ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "illegal_move",
                message: e.to_string(),
            })?;

            // Apply increment to mover on accepted move
            match actor_side {
                Side::Red => {
                    if let Some(ref mut r) = red_rem {
                        *r += clock_inc;
                    }
                }
                Side::Black => {
                    if let Some(ref mut b) = blk_rem {
                        *b += clock_inc;
                    }
                }
            }

            let (next_status, outcome_str, reason_str) = map_verdict(&verdict);
            commit_match_event_tx(
                db,
                match_id,
                current_seq + 1,
                cmd_id,
                Some(actor_side.as_str()),
                "move",
                &serde_json::json!({
                    "move_uci": cmv.to_uci(),
                    "move_chinese": zh,
                    "from": mv.from,
                    "to": mv.to,
                }),
                &staged_game.to_fen(),
                next_status,
                red_rem,
                blk_rem,
                outcome_str.as_deref(),
                reason_str.as_deref(),
                received_utc_ms,
            )
            .await?;

            // Mutate in-memory actor state ONLY after the database transaction has durably committed!
            *game = staged_game;
            *draw_offered_by = None;
            *takeback_requested_by = None;
            // Next turn's clock starts AFTER commit so DB latency is never charged to opponent (Section 8)!
            *turn_started_mono = if next_status == "playing" {
                Some(Instant::now())
            } else {
                None
            };
        }
        MatchCommandRequest::Resign { .. } => {
            let winner = match actor_side.opposite() {
                Side::Red => "red_win",
                Side::Black => "black_win",
            };
            commit_match_event_tx(
                db,
                match_id,
                current_seq + 1,
                cmd_id,
                Some(actor_side.as_str()),
                "resigned",
                &serde_json::json!({ "resigned_side": actor_side.as_str() }),
                &game.to_fen(),
                "finished",
                red_rem,
                blk_rem,
                Some(winner),
                Some("resignation"),
                received_utc_ms,
            )
            .await?;
            *draw_offered_by = None;
            *turn_started_mono = None;
        }
        MatchCommandRequest::OfferDraw { .. } => {
            commit_match_event_tx(
                db,
                match_id,
                current_seq + 1,
                cmd_id,
                Some(actor_side.as_str()),
                "draw_offered",
                &serde_json::json!({ "offered_by": actor_side.as_str() }),
                &game.to_fen(),
                "playing",
                red_rem,
                blk_rem,
                None,
                None,
                received_utc_ms,
            )
            .await?;
            *draw_offered_by = Some(actor_side);
            *turn_started_mono = Some(Instant::now());
        }
        MatchCommandRequest::AcceptDraw { .. } => {
            if *draw_offered_by != Some(actor_side.opposite()) {
                return Err(ApiError {
                    status: StatusCode::CONFLICT,
                    code: "no_pending_draw_offer",
                    message: "No draw offer from opponent is pending".to_string(),
                });
            }
            commit_match_event_tx(
                db,
                match_id,
                current_seq + 1,
                cmd_id,
                Some(actor_side.as_str()),
                "draw_accepted",
                &serde_json::json!({ "accepted_by": actor_side.as_str() }),
                &game.to_fen(),
                "finished",
                red_rem,
                blk_rem,
                Some("draw"),
                Some("agreed_draw"),
                received_utc_ms,
            )
            .await?;
            *draw_offered_by = None;
            *takeback_requested_by = None;
            *turn_started_mono = None;
        }
        MatchCommandRequest::RequestTakeback { .. } => {
            if !is_friendly {
                return Err(ApiError {
                    status: StatusCode::FORBIDDEN,
                    code: "takeback_friendly_only",
                    message: "Takebacks are available only in friendly rooms".to_string(),
                });
            }
            if game.ply_count() == 0 {
                return Err(ApiError {
                    status: StatusCode::CONFLICT,
                    code: "no_moves_to_take_back",
                    message: "No committed moves to take back".to_string(),
                });
            }
            commit_match_event_tx(
                db,
                match_id,
                current_seq + 1,
                cmd_id,
                Some(actor_side.as_str()),
                "takeback_requested",
                &serde_json::json!({ "requested_by": actor_side.as_str() }),
                &game.to_fen(),
                "playing",
                red_rem,
                blk_rem,
                None,
                None,
                received_utc_ms,
            )
            .await?;
            *takeback_requested_by = Some(actor_side);
            *turn_started_mono = Some(Instant::now());
        }
        MatchCommandRequest::AcceptTakeback { .. } => {
            if !is_friendly {
                return Err(ApiError {
                    status: StatusCode::FORBIDDEN,
                    code: "takeback_friendly_only",
                    message: "Takebacks are available only in friendly rooms".to_string(),
                });
            }
            if *takeback_requested_by != Some(actor_side.opposite()) {
                return Err(ApiError {
                    status: StatusCode::CONFLICT,
                    code: "no_pending_takeback_request",
                    message: "No pending takeback request from opponent".to_string(),
                });
            }
            let mut staged_game = game.clone();
            let undone = staged_game.undo_move().map_err(|_| ApiError {
                status: StatusCode::CONFLICT,
                code: "no_moves_to_take_back",
                message: "No committed moves to take back".to_string(),
            })?;
            commit_match_event_tx(
                db,
                match_id,
                current_seq + 1,
                cmd_id,
                Some(actor_side.as_str()),
                "takeback_accepted",
                &serde_json::json!({
                    "accepted_by": actor_side.as_str(),
                    "undone_uci": undone.to_uci(),
                }),
                &staged_game.to_fen(),
                "playing",
                red_rem,
                blk_rem,
                None,
                None,
                received_utc_ms,
            )
            .await?;
            *game = staged_game;
            *draw_offered_by = None;
            *takeback_requested_by = None;
            *turn_started_mono = Some(Instant::now());
        }
    }

    let mut snap = load_match_snapshot(db, match_id, None).await?;
    snap.draw_offered_by = *draw_offered_by;
    snap.takeback_requested_by = *takeback_requested_by;
    Ok(snap)
}

fn map_verdict(verdict: &Option<Adjudication>) -> (&'static str, Option<String>, Option<String>) {
    let Some(v) = verdict else {
        return ("playing", None, None);
    };
    let out = match v.outcome {
        GameOutcome::RedWin => "red_win",
        GameOutcome::BlackWin => "black_win",
        GameOutcome::Draw => "draw",
    };
    let reason = serde_json::to_value(v.reason)
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    ("finished", Some(out.to_string()), Some(reason))
}

#[allow(clippy::too_many_arguments)]
async fn commit_match_event_tx(
    db: &SqlitePool,
    match_id: &str,
    next_seq: u64,
    command_id: &str,
    actor_side: Option<&str>,
    event_type: &str,
    payload: &serde_json::Value,
    next_fen: &str,
    next_status: &str,
    red_rem: Option<i64>,
    blk_rem: Option<i64>,
    outcome: Option<&str>,
    reason_code: Option<&str>,
    utc_ms: i64,
) -> Result<(), ApiError> {
    let mut tx = db.begin().await.map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    sqlx::query(
        "INSERT INTO match_events (match_id, seq, command_id, actor_side, event_type, payload_json, created_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(match_id)
    .bind(next_seq as i64)
    .bind(command_id)
    .bind(actor_side)
    .bind(event_type)
    .bind(payload.to_string())
    .bind(utc_ms)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    sqlx::query(
        "UPDATE matches SET
            current_seq = ?, current_fen = ?, status = ?,
            red_remaining_ms = ?, black_remaining_ms = ?,
            turn_started_utc_ms = ?, outcome = ?, reason_code = ?, updated_at_ms = ?
         WHERE match_id = ?",
    )
    .bind(next_seq as i64)
    .bind(next_fen)
    .bind(next_status)
    .bind(red_rem)
    .bind(blk_rem)
    .bind(utc_ms)
    .bind(outcome)
    .bind(reason_code)
    .bind(utc_ms)
    .bind(match_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    tx.commit().await.map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })
}

async fn load_match_snapshot(
    db: &SqlitePool,
    match_id: &str,
    since_seq: Option<u64>,
) -> Result<MatchSnapshotResponse, ApiError> {
    let m = sqlx::query("SELECT * FROM matches WHERE match_id = ?")
        .bind(match_id)
        .fetch_optional(db)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?
        .ok_or_else(|| ApiError {
            status: StatusCode::NOT_FOUND,
            code: "match_not_found",
            message: "Match not found".to_string(),
        })?;

    let all_events = sqlx::query(
        "SELECT seq, command_id, actor_side, event_type, payload_json, created_at_ms FROM match_events WHERE match_id = ? ORDER BY seq ASC",
    )
    .bind(match_id)
    .fetch_all(db)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let mut moves_uci = Vec::new();
    let mut moves_chinese = Vec::new();
    let mut missing_events = Vec::new();
    let mut pending_draw: Option<Side> = None;
    let mut pending_takeback: Option<Side> = None;

    for ev in all_events {
        let seq = ev.get::<i64, _>("seq") as u64;
        let etype: String = ev.get("event_type");
        let payload_str: String = ev.get("payload_json");
        let payload: serde_json::Value =
            serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
        if etype == "move" {
            pending_draw = None;
            pending_takeback = None;
            if let Some(u) = payload["move_uci"].as_str() {
                moves_uci.push(u.to_string());
            }
            if let Some(z) = payload["move_chinese"].as_str() {
                moves_chinese.push(z.to_string());
            }
        } else if etype == "draw_offered" {
            pending_draw = match payload["offered_by"].as_str() {
                Some("red") => Some(Side::Red),
                Some("black") => Some(Side::Black),
                _ => None,
            };
        } else if etype == "takeback_requested" {
            pending_takeback = match payload["requested_by"].as_str() {
                Some("red") => Some(Side::Red),
                Some("black") => Some(Side::Black),
                _ => None,
            };
        } else if etype == "takeback_accepted" {
            pending_draw = None;
            pending_takeback = None;
            let _ = moves_uci.pop();
            let _ = moves_chinese.pop();
        } else {
            pending_draw = None;
            pending_takeback = None;
        }
        if since_seq.is_some_and(|s| seq > s) {
            missing_events.push(CommittedEventRecord {
                seq,
                command_id: ev.get("command_id"),
                actor_side: ev.get("actor_side"),
                event_type: etype,
                payload,
                created_at_ms: ev.get("created_at_ms"),
            });
        }
    }

    let current_fen: String = m.get("current_fen");
    let status_str: String = m.get("status");
    let board = xiangqi_core::Board::from_fen(&current_fen).unwrap_or_default();
    let stm = board.side_to_move();
    let vis = match m.get::<String, _>("visibility").as_str() {
        "public" => Visibility::Public,
        "unlisted" => Visibility::Unlisted,
        _ => Visibility::Private,
    };

    let mut red_remaining_ms: Option<i64> = m.get("red_remaining_ms");
    let mut black_remaining_ms: Option<i64> = m.get("black_remaining_ms");
    if status_str == "playing"
        && let Some(started_utc) = m.get::<Option<i64>, _>("turn_started_utc_ms")
    {
        let elapsed = (now_utc_ms() - started_utc).max(0);
        match stm {
            Side::Red => {
                if let Some(ref mut r) = red_remaining_ms {
                    *r = (*r - elapsed).max(0);
                }
            }
            Side::Black => {
                if let Some(ref mut b) = black_remaining_ms {
                    *b = (*b - elapsed).max(0);
                }
            }
        }
    }

    let next_rematch_code = if let Some(next_mid) = m.get::<Option<String>, _>("next_rematch_id") {
        sqlx::query_scalar::<_, String>("SELECT invitation_code FROM matches WHERE match_id = ?")
            .bind(next_mid)
            .fetch_optional(db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    Ok(MatchSnapshotResponse {
        match_id: match_id.to_string(),
        invitation_code: m.get("invitation_code"),
        rules_profile: m.get("rules_profile"),
        status: status_str,
        seq: m.get::<i64, _>("current_seq") as u64,
        fen: current_fen,
        side_to_move: stm,
        moves_uci,
        moves_chinese,
        red_seated: m.get::<Option<String>, _>("red_account_id").is_some(),
        black_seated: m.get::<Option<String>, _>("black_account_id").is_some(),
        clock_initial_ms: m.get("clock_initial_ms"),
        clock_increment_ms: m.get("clock_increment_ms"),
        red_remaining_ms,
        black_remaining_ms,
        is_friendly: m.get::<i64, _>("is_friendly") != 0,
        draw_offered_by: pending_draw,
        takeback_requested_by: pending_takeback,
        next_rematch_invitation_code: next_rematch_code,
        outcome: m.get("outcome"),
        reason_code: m.get("reason_code"),
        visibility: vis,
        missing_events,
    })
}

async fn create_or_get_rematch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(match_id): Path<String>,
) -> Result<Json<InvitationCreatedResponse>, ApiError> {
    let caller_id = authenticate_request(&state, &headers, true).await?;
    // Serialize rematch lookup + creation so simultaneous requests from both seats never race on `next_rematch_id`!
    let _rematch_guard = state.rematch_lock.lock().await;
    let mut tx = state.db.begin().await.map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    let m = sqlx::query(
        "SELECT status, is_friendly, red_account_id, black_account_id, clock_initial_ms, clock_increment_ms, next_rematch_id
         FROM matches WHERE match_id = ?",
    )
    .bind(&match_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?
    .ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        code: "match_not_found",
        message: "Match not found".to_string(),
    })?;

    let status: String = m.get("status");
    if status != "finished" && status != "interrupted" {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            code: "match_still_active",
            message: "Rematch can be started once the current game finishes".to_string(),
        });
    }

    let prev_red: Option<String> = m.get("red_account_id");
    let prev_blk: Option<String> = m.get("black_account_id");
    let caller_was_red = prev_red.as_deref() == Some(caller_id.as_str());
    let caller_was_blk = prev_blk.as_deref() == Some(caller_id.as_str());
    if !caller_was_red && !caller_was_blk {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            code: "not_a_participant",
            message: "Only participants can start a rematch".to_string(),
        });
    }

    let clock_initial_ms: Option<i64> = m.get("clock_initial_ms");
    let clock_increment_ms: i64 = m.get("clock_increment_ms");

    if let Some(existing_rematch_id) = m.get::<Option<String>, _>("next_rematch_id")
        && let Some(existing_code) = sqlx::query_scalar::<_, String>(
            "SELECT invitation_code FROM matches WHERE match_id = ?",
        )
        .bind(&existing_rematch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?
    {
        tx.commit().await.map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;
        return Ok(Json(InvitationCreatedResponse {
            match_id: existing_rematch_id,
            share_path: format!("/invite/{existing_code}"),
            invitation_code: existing_code,
            rules_profile: ADOPTED_RULES_PROFILE.to_string(),
            clock_initial_ms,
            clock_increment_ms,
        }));
    }

    let new_match_id = format!("m_{}", Uuid::new_v4().simple());
    let invitation_code = Uuid::new_v4().simple().to_string()[..10].to_string();
    // Reverse colors from previous match!
    let (new_red, new_blk) = if caller_was_red {
        (None, Some(caller_id))
    } else {
        (Some(caller_id), None)
    };
    let ts = now_utc_ms();
    let start_fen = xiangqi_core::STARTING_FEN;

    sqlx::query(
        "INSERT INTO matches (
            match_id, invitation_code, rules_profile, is_friendly, initial_fen, current_fen,
            current_seq, status, red_account_id, black_account_id, clock_initial_ms, clock_increment_ms,
            red_remaining_ms, black_remaining_ms, visibility, rematch_of_match_id, created_at_ms, updated_at_ms
        ) VALUES (?, ?, ?, ?, ?, ?, 0, 'waiting', ?, ?, ?, ?, ?, ?, 'private', ?, ?, ?)",
    )
    .bind(&new_match_id)
    .bind(&invitation_code)
    .bind(ADOPTED_RULES_PROFILE)
    .bind(m.get::<i64, _>("is_friendly"))
    .bind(start_fen)
    .bind(start_fen)
    .bind(&new_red)
    .bind(&new_blk)
    .bind(clock_initial_ms)
    .bind(clock_increment_ms)
    .bind(clock_initial_ms)
    .bind(clock_initial_ms)
    .bind(&match_id)
    .bind(ts)
    .bind(ts)
    .execute(&mut *tx)
    .await
    .map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    sqlx::query("UPDATE matches SET next_rematch_id = ?, updated_at_ms = ? WHERE match_id = ?")
        .bind(&new_match_id)
        .bind(ts)
        .bind(&match_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "db_error",
            message: e.to_string(),
        })?;

    tx.commit().await.map_err(|e| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: "db_error",
        message: e.to_string(),
    })?;

    ensure_match_actor(&state, &new_match_id).await?;

    // Broadcast updated snapshot on finished match so opponent sees in-room "Join Rematch" immediately!
    if let Ok(prev_actor) = ensure_match_actor(&state, &match_id).await
        && let Ok(prev_snap) = load_match_snapshot(&state.db, &match_id, None).await
    {
        let _ = prev_actor.bcast.send(prev_snap);
    }

    Ok(Json(InvitationCreatedResponse {
        match_id: new_match_id,
        share_path: format!("/invite/{invitation_code}"),
        invitation_code,
        rules_profile: ADOPTED_RULES_PROFILE.to_string(),
        clock_initial_ms,
        clock_increment_ms,
    }))
}

async fn export_match_pgn(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(match_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let viewer = authenticate_request(&state, &headers, false).await.ok();
    check_match_read_access(&state.db, &match_id, viewer.as_deref()).await?;
    let snap = load_match_snapshot(&state.db, &match_id, None).await?;

    let pgn_res = match snap.outcome.as_deref() {
        Some("red_win") => "1-0",
        Some("black_win") => "0-1",
        Some("draw") => "1/2-1/2",
        _ => "*",
    };

    let mut pgn = String::new();
    pgn.push_str("[Event \"Jade Friendly Xiangqi Match\"]\n");
    pgn.push_str(&format!("[MatchId \"{}\"]\n", snap.match_id));
    pgn.push_str(&format!("[RulesProfile \"{}\"]\n", snap.rules_profile));
    if let Some(init_ms) = snap.clock_initial_ms {
        pgn.push_str(&format!(
            "[TimeControl \"{}+{}\"]\n",
            init_ms / 1000,
            snap.clock_increment_ms / 1000
        ));
    } else {
        pgn.push_str("[TimeControl \"-\"]\n");
    }
    pgn.push_str(&format!("[Result \"{pgn_res}\"]\n"));
    if let Some(ref r) = snap.reason_code {
        pgn.push_str(&format!("[Termination \"{r}\"]\n"));
    }
    pgn.push('\n');

    for (idx, uci) in snap.moves_uci.iter().enumerate() {
        let zh = snap
            .moves_chinese
            .get(idx)
            .map(String::as_str)
            .unwrap_or("");
        if idx % 2 == 0 {
            pgn.push_str(&format!("{}. {uci} {{{zh}}} ", idx / 2 + 1));
        } else {
            pgn.push_str(&format!("{uci} {{{zh}}} "));
        }
    }
    pgn.push_str(pgn_res);
    pgn.push('\n');

    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-chess-pgn; charset=utf-8"),
        )],
        pgn,
    ))
}

#[derive(Deserialize)]
struct PlayerSearchQuery {
    q: Option<String>,
}

async fn archive_search_players(
    State(state): State<AppState>,
    Query(q): Query<PlayerSearchQuery>,
) -> impl IntoResponse {
    let query_str = q.q.unwrap_or_default();
    Json(serde_json::json!({
        "dataset_id": state.archive.manifest().dataset_id,
        "is_synthetic_fixture": state.archive.manifest().is_synthetic_development_fixture,
        "provenance_banner": state.archive.manifest().provenance_banner,
        "players": state.archive.search_players(&query_str),
    }))
}

async fn archive_get_game(
    State(state): State<AppState>,
    Path(game_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let game = state.archive.get_game(&game_id).ok_or_else(|| ApiError {
        status: StatusCode::NOT_FOUND,
        code: "game_not_found",
        message: "Archive game not found".to_string(),
    })?;
    Ok(Json(game.clone()))
}

#[derive(Deserialize)]
struct PositionQuery {
    fen: Option<String>,
}

async fn archive_query_position(
    State(state): State<AppState>,
    Query(q): Query<PositionQuery>,
) -> Result<impl IntoResponse, ApiError> {
    let fen = q
        .fen
        .unwrap_or_else(|| xiangqi_core::STARTING_FEN.to_string());
    let res = state
        .archive
        .query_position(&fen)
        .map_err(|message| ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_fen",
            message,
        })?;
    Ok(Json(res))
}

pub fn server_ready() -> bool {
    true
}
