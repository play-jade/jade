use archive_client::Visibility;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use jade_match_server::{
    AllowedOrigins, CoordMovePayload, CurrentSessionResponse, GuestSessionResponse,
    InvitationCreatedResponse, JoinInvitationResponse, MatchCommandRequest, MatchSnapshotResponse,
    PrivacyUpdateRequest, build_app_state, build_router, mark_interrupted_timed_matches_on_startup,
};
use tokio::io::DuplexStream;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tower::ServiceExt;
use xiangqi_core::{ADOPTED_RULES_PROFILE, Side};

async fn connect_ws_duplex(
    app: &axum::Router,
    req: axum::http::Request<()>,
) -> Result<WebSocketStream<DuplexStream>, Box<tokio_tungstenite::tungstenite::Error>> {
    let (client_io, server_io) = tokio::io::duplex(65_536);
    let svc = TowerToHyperService::new(app.clone());
    tokio::spawn(async move {
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(TokioIo::new(server_io), svc)
            .with_upgrades()
            .await;
    });
    let (ws, _) = tokio_tungstenite::client_async(req, client_io)
        .await
        .map_err(Box::new)?;
    Ok(ws)
}

static TEST_DB_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A test's database, removed when the test ends: a PostgreSQL database dropped from its server, or a SQLite file's
/// directory deleted. A private in-memory SQLite database needs neither.
struct TestDb {
    url: String,
    postgres: Option<(String, String)>,
    dir: Option<std::path::PathBuf>,
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
        if let Some((admin, name)) = self.postgres.take() {
            // Dropping runs outside the test's runtime, on a thread of its own; FORCE ends sessions still open.
            let leftover = name.clone();
            let dropped = std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                runtime.block_on(async {
                    let mut conn = <sqlx::AnyConnection as sqlx::Connection>::connect(&admin)
                        .await
                        .map_err(|e| e.to_string())?;
                    sqlx::query(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
                        .execute(&mut conn)
                        .await
                        .map(|_| ())
                        .map_err(|e| format!("{name}: {e}"))
                })
            })
            .join()
            .unwrap_or_else(|_| Err("the cleanup thread panicked".to_string()));
            if let Err(error) = dropped {
                let report = format!("test database {leftover} was not dropped: {error}");
                // A test already failing reports the leftover; a passing one fails, naming it.
                if std::thread::panicking() {
                    eprintln!("{report}");
                } else {
                    panic!("{report}");
                }
            }
        }
    }
}

/// Sets query options on a URL, replacing any it already has with the same names as the URL parser reads them.
fn with_query(url: &str, set: &[(&str, &str)]) -> String {
    let mut parsed = url::Url::parse(url).expect("a database URL");
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !set.iter().any(|(name, _)| key.eq_ignore_ascii_case(name)))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    {
        let mut query = parsed.query_pairs_mut();
        query.clear();
        query.extend_pairs(kept);
        query.extend_pairs(set.iter().copied());
    }
    parsed.to_string()
}

/// The URL of database `name` on the server `admin` names: only the database changes, and a `dbname` option, which
/// would otherwise override the path, is removed however it is spelled.
fn fixture_url(admin: &str, name: &str) -> String {
    let mut parsed = url::Url::parse(admin).expect("a database URL");
    parsed.set_path(&format!("/{name}"));
    let kept: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("dbname"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.query_pairs_mut().clear().extend_pairs(kept);
    }
    parsed.to_string()
}

#[test]
fn a_fixture_url_names_only_its_own_database() {
    let url = fixture_url(
        "postgres://jade@127.0.0.1:5432/postgres?db%6Eame=postgres&application_name=t",
        "jade_x",
    );
    let parsed = url::Url::parse(&url).unwrap();
    assert_eq!(parsed.path(), "/jade_x");
    assert!(
        parsed
            .query_pairs()
            .all(|(k, _)| !k.eq_ignore_ascii_case("dbname")),
        "{url}"
    );
    assert!(
        parsed
            .query_pairs()
            .any(|(k, v)| k == "application_name" && v == "t")
    );
    // An existing option, however it is spelled, is replaced rather than joined by a second.
    let replaced = with_query(
        &format!("{url}&SSLMode=disable"),
        &[("sslmode", "verify-full")],
    );
    let modes: Vec<(String, String)> = url::Url::parse(&replaced)
        .unwrap()
        .query_pairs()
        .filter(|(k, _)| k.eq_ignore_ascii_case("sslmode"))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(
        modes,
        vec![("sslmode".to_string(), "verify-full".to_string())]
    );
}

/// A fresh database on the PostgreSQL server `JADE_TEST_POSTGRES_URL` names (a URL ending in a database to connect to
/// for administration, such as `postgres://jade@127.0.0.1:55432/postgres`), or `None` when it names none.
async fn fresh_postgres() -> Option<TestDb> {
    let admin = std::env::var("JADE_TEST_POSTGRES_URL")
        .ok()
        .filter(|u| !u.is_empty())?;
    sqlx::any::install_default_drivers();
    let seq = TEST_DB_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Unique across runs as well as within one, so a reused process ID never meets an earlier run's database.
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros();
    let name = format!("jade_test_{}_{started}_{seq}", std::process::id());
    let mut conn = <sqlx::AnyConnection as sqlx::Connection>::connect(&admin)
        .await
        .expect("connect to the PostgreSQL test server");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&mut conn)
        .await
        .expect("create a test database");
    let url = fixture_url(&admin, &name);
    let db = TestDb {
        url,
        postgres: Some((admin, name.clone())),
        dir: None,
    };
    // No test receives a fixture that reaches any database but its own.
    let mut check = <sqlx::AnyConnection as sqlx::Connection>::connect(&db.url)
        .await
        .unwrap();
    let reached: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut check)
        .await
        .unwrap();
    assert_eq!(reached, name, "the fixture URL reaches its own database");
    Some(db)
}

/// The database a test runs on: a fresh PostgreSQL database when one is configured, else private in-memory SQLite.
async fn test_db() -> TestDb {
    match fresh_postgres().await {
        Some(db) => db,
        None => TestDb {
            url: "sqlite::memory:".to_string(),
            postgres: None,
            dir: None,
        },
    }
}

/// A database that outlives one server, for restart and concurrent-writer tests: a fresh PostgreSQL database when one
/// is configured, else a new SQLite file.
async fn durable_test_db(label: &str) -> TestDb {
    if let Some(db) = fresh_postgres().await {
        return db;
    }
    let seq = TEST_DB_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jade-{label}-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.sqlite3");
    TestDb {
        url: format!("sqlite://{}?mode=rwc", path.display()),
        postgres: None,
        dir: Some(dir),
    }
}

/// A started 10+5 match between two new guests on a fresh test database, with direct access to that database.
struct TestMatch {
    test_db: TestDb,
    db: sqlx::AnyPool,
    app: axum::Router,
    red: GuestSessionResponse,
    match_id: String,
}

async fn test_match() -> TestMatch {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let db = state.db.clone();
    let app = build_router(state);
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    TestMatch {
        test_db,
        db,
        app,
        red,
        match_id,
    }
}

async fn json_body<T: serde::de::DeserializeOwned>(resp: axum::response::Response) -> T {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

/// Posts `cmd` to the match's command endpoint as the bearer of `token`.
async fn post_command(
    app: &axum::Router,
    token: &str,
    match_id: &str,
    cmd: &MatchCommandRequest,
) -> axum::response::Response {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{match_id}/commands"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(cmd).unwrap()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// Asserts that a nonparticipant replaying `cmd` is refused and receives no snapshot.
async fn assert_replay_refused(
    app: &axum::Router,
    token: &str,
    match_id: &str,
    cmd: &MatchCommandRequest,
) {
    let resp = post_command(app, token, match_id, cmd).await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = json_body(resp).await;
    assert!(
        body.get("moves_uci").is_none(),
        "a nonparticipant must not receive a snapshot, got {body}"
    );
}

/// Sets one participant's visibility with every research consent withheld.
async fn set_visibility(
    app: &axum::Router,
    token: &str,
    match_id: &str,
    visibility: Visibility,
) -> serde_json::Value {
    json_body(save_choices(app, token, match_id, visibility, [false; 3]).await).await
}

/// Posts `body` as JSON to `uri` as the bearer of `token` and reads the answer.
async fn post_json<T: serde::de::DeserializeOwned>(
    app: &axum::Router,
    token: &str,
    uri: String,
    body: serde_json::Value,
) -> T {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    json_body(app.clone().oneshot(req).await.unwrap()).await
}

/// Red invites and Black joins a 10+5 match; returns the match ID.
async fn start_match(app: &axum::Router, red_token: &str, blk_token: &str) -> String {
    let terms = serde_json::json!({ "preferred_side": "red", "clock_initial_ms": 600_000, "clock_increment_ms": 5_000 });
    let invite: InvitationCreatedResponse =
        post_json(app, red_token, "/api/v1/invitations".to_string(), terms).await;
    let path = format!("/api/v1/invitations/{}/join", invite.invitation_code);
    let joined: JoinInvitationResponse =
        post_json(app, blk_token, path, serde_json::json!({})).await;
    joined.match_id
}

/// Saves all four of a participant's choices against their current revision: visibility, then training, research,
/// and dataset consent.
async fn save_choices(
    app: &axum::Router,
    token: &str,
    match_id: &str,
    visibility: Visibility,
    consents: [bool; 3],
) -> axum::response::Response {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{match_id}/privacy"))
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&PrivacyUpdateRequest {
                visibility: Some(visibility),
                allow_internal_training: Some(consents[0]),
                allow_published_research: Some(consents[1]),
                allow_dataset_release: Some(consents[2]),
                expected_revision: Some(current_revision(app, token, match_id).await),
            })
            .unwrap(),
        ))
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// The caller's own choices revision for a match, read from their snapshot; 0 when they hold no seat.
async fn current_revision(app: &axum::Router, token: &str, match_id: &str) -> i64 {
    let req = Request::builder()
        .uri(format!("/api/v1/matches/{match_id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    if !resp.status().is_success() {
        return 0;
    }
    let snap: MatchSnapshotResponse = json_body(resp).await;
    snap.viewer_choices.map(|c| c.revision).unwrap_or(0)
}

async fn new_guest(app: &axum::Router, name: &str) -> (GuestSessionResponse, String) {
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/guest")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "display_name": name, "transport": "web_cookie" }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    // The cookie outlives a closed browser for exactly the session's lifetime (7 days).
    assert!(set_cookie.contains("Max-Age=604800"), "{set_cookie}");
    let cookie_pair = set_cookie.split(';').next().unwrap().to_string();
    let guest: GuestSessionResponse = json_body(resp).await;
    (guest, cookie_pair)
}

#[tokio::test]
async fn cookie_csrf_enforcement_and_friendly_invitation_lifecycle() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state);

    let (red_guest, red_cookie) = new_guest(&app, "Red Host").await;
    let (blk_guest, _blk_cookie) = new_guest(&app, "Black Joiner").await;
    let (spec_guest, _spec_cookie) = new_guest(&app, "Spectator").await;

    // 1. Cookie-authenticated mutation WITHOUT X-CSRF-Token must fail 403!
    let missing_csrf = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("cookie", &red_cookie)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "preferred_side": "red",
                "clock_initial_ms": 600_000,
                "clock_increment_ms": 5_000
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(missing_csrf).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 2. Cookie-authenticated mutation WITH valid X-CSRF-Token creates friendly 10+5 invitation!
    let valid_create = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("cookie", &red_cookie)
        .header("x-csrf-token", &red_guest.csrf_token)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "preferred_side": "red",
                "clock_initial_ms": 600_000,
                "clock_increment_ms": 5_000
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(valid_create).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let created: InvitationCreatedResponse = json_body(resp).await;
    assert_eq!(created.rules_profile, ADOPTED_RULES_PROFILE);
    assert_eq!(created.clock_initial_ms, Some(600_000));
    assert_eq!(created.clock_increment_ms, 5_000);
    // Shareable link must carry NO seat credentials!
    assert_eq!(
        created.share_path,
        format!("/invite/{}", created.invitation_code)
    );
    assert!(!created.share_path.contains("jtok_"));

    // Also verify untimed invitation creation works cleanly (`clock_initial_ms: null`)
    let untimed_create = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header(
            "authorization",
            format!("Bearer {}", red_guest.access_token),
        )
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "preferred_side": "red",
                "clock_initial_ms": null
            })
            .to_string(),
        ))
        .unwrap();
    let untimed_resp = app.clone().oneshot(untimed_create).await.unwrap();
    assert_eq!(untimed_resp.status(), StatusCode::OK);
    let untimed_inv: InvitationCreatedResponse = json_body(untimed_resp).await;
    assert_eq!(untimed_inv.clock_initial_ms, None);
    assert_eq!(untimed_inv.clock_increment_ms, 0);

    // 3. Re-opening invitation as Red preserves Red seat; Black joins into Black; 3rd user becomes Spectator!
    let reopen_red = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/invitations/{}/join",
            created.invitation_code
        ))
        .header(
            "authorization",
            format!("Bearer {}", red_guest.access_token),
        )
        .body(Body::empty())
        .unwrap();
    let red_join: JoinInvitationResponse =
        json_body(app.clone().oneshot(reopen_red).await.unwrap()).await;
    assert_eq!(red_join.assigned_side, Some(Side::Red));
    assert_eq!(red_join.snapshot.status, "waiting");

    let join_blk = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/invitations/{}/join",
            created.invitation_code
        ))
        .header(
            "authorization",
            format!("Bearer {}", blk_guest.access_token),
        )
        .body(Body::empty())
        .unwrap();
    let blk_join: JoinInvitationResponse =
        json_body(app.clone().oneshot(join_blk).await.unwrap()).await;
    assert_eq!(blk_join.assigned_side, Some(Side::Black));
    assert_eq!(blk_join.snapshot.status, "playing");

    // 3b. While match is private (default), an unseated 3rd user MUST be denied snapshot access on join!
    let join_spec_private = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/invitations/{}/join",
            created.invitation_code
        ))
        .header(
            "authorization",
            format!("Bearer {}", spec_guest.access_token),
        )
        .body(Body::empty())
        .unwrap();
    let spec_priv_resp = app.clone().oneshot(join_spec_private).await.unwrap();
    assert_eq!(spec_priv_resp.status(), StatusCode::FORBIDDEN);

    // Once both participants opt into `unlisted`, 3rd user joins as Spectator (`assigned_side == None`)!
    for token in [&red_guest.access_token, &blk_guest.access_token] {
        assert_eq!(
            save_choices(
                &app,
                token,
                &created.match_id,
                Visibility::Unlisted,
                [false; 3]
            )
            .await
            .status(),
            StatusCode::OK
        );
    }

    let join_spec = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/invitations/{}/join",
            created.invitation_code
        ))
        .header(
            "authorization",
            format!("Bearer {}", spec_guest.access_token),
        )
        .body(Body::empty())
        .unwrap();
    let spec_join: JoinInvitationResponse =
        json_body(app.clone().oneshot(join_spec).await.unwrap()).await;
    assert_eq!(spec_join.assigned_side, None);

    // 4. Spectator has zero command authority (403)!
    let spec_cmd = MatchCommandRequest::Move {
        protocol: 1,
        match_id: created.match_id.clone(),
        command_id: "cmd_spec_1".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 2],
            to: [4, 2],
        },
    };
    let spec_move_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", created.match_id))
        .header(
            "authorization",
            format!("Bearer {}", spec_guest.access_token),
        )
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&spec_cmd).unwrap()))
        .unwrap();
    let spec_move_resp = app.clone().oneshot(spec_move_req).await.unwrap();
    assert_eq!(spec_move_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn duplicate_idempotency_stale_seq_reconnect_gap_and_dual_privacy_consent() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state);

    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let (outsider, _) = new_guest(&app, "Outsider").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "preferred_side": "red",
                "clock_initial_ms": 600_000,
                "clock_increment_ms": 5_000
            })
            .to_string(),
        ))
        .unwrap();
    let inv: InvitationCreatedResponse =
        json_body(app.clone().oneshot(create_req).await.unwrap()).await;

    let join_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/invitations/{}/join", inv.invitation_code))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let _: JoinInvitationResponse = json_body(app.clone().oneshot(join_req).await.unwrap()).await;

    // 1. Red plays Cannon two-five (`h2e2` = `[7,2] -> [4,2]`)
    let move1 = MatchCommandRequest::Move {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "cmd_red_m1".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 2],
            to: [4, 2],
        },
    };
    let m1_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&move1).unwrap()))
        .unwrap();
    let snap1: MatchSnapshotResponse = json_body(app.clone().oneshot(m1_req).await.unwrap()).await;
    assert_eq!(snap1.seq, 1);
    assert_eq!(snap1.moves_uci, vec!["h2e2"]);
    assert_eq!(snap1.moves_chinese, vec!["炮二平五"]);
    let red_rem_after_first = snap1.red_remaining_ms.unwrap();
    assert!(
        red_rem_after_first > 600_000,
        "5s increment should be added"
    );

    // 2. Duplicate `command_id` retry MUST be idempotent (seq stays 1, clock NOT incremented twice)!
    let dup_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&move1).unwrap()))
        .unwrap();
    let dup_resp = app.clone().oneshot(dup_req).await.unwrap();
    assert_eq!(dup_resp.status(), StatusCode::OK);
    let snap_dup: MatchSnapshotResponse = json_body(dup_resp).await;
    assert_eq!(snap_dup.seq, 1);
    assert_eq!(snap_dup.red_remaining_ms, Some(red_rem_after_first));

    // 2b. A nonparticipant replaying a known `command_id` on a private match receives no snapshot!
    assert_replay_refused(&app, &outsider.access_token, &inv.match_id, &move1).await;

    // 3. Stale `expected_seq` (Black sends `expected_seq: 0` when match is at `seq: 1`) MUST be rejected 409!
    let stale_blk = MatchCommandRequest::Move {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "cmd_blk_stale".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 9],
            to: [6, 7],
        },
    };
    let stale_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&stale_blk).unwrap()))
        .unwrap();
    let stale_resp = app.clone().oneshot(stale_req).await.unwrap();
    assert_eq!(stale_resp.status(), StatusCode::CONFLICT);

    // 4. Black plays `h9g7` (`[7,9] -> [6,7]`) at `expected_seq: 1` -> `seq: 2`
    let valid_blk = MatchCommandRequest::Move {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "cmd_blk_m1".to_string(),
        expected_seq: 1,
        mv: CoordMovePayload {
            from: [7, 9],
            to: [6, 7],
        },
    };
    let blk_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&valid_blk).unwrap()))
        .unwrap();
    let snap2: MatchSnapshotResponse = json_body(app.clone().oneshot(blk_req).await.unwrap()).await;
    assert_eq!(snap2.seq, 2);
    assert_eq!(snap2.moves_uci, vec!["h2e2", "h9g7"]);

    // 5. Reconnect gap repair: `since_seq=1` returns committed event `seq=2`!
    let gap_req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/matches/{}?since_seq=1", inv.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .body(Body::empty())
        .unwrap();
    let gap_snap: MatchSnapshotResponse =
        json_body(app.clone().oneshot(gap_req).await.unwrap()).await;
    assert_eq!(gap_snap.missing_events.len(), 1);
    assert_eq!(gap_snap.missing_events[0].seq, 2);
    assert_eq!(gap_snap.missing_events[0].command_id, "cmd_blk_m1");

    // 6. Private-by-default enforcement + Dual-participant sharing/consent rules!
    let outsider_read = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/matches/{}", inv.match_id))
        .header("authorization", format!("Bearer {}", outsider.access_token))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(outsider_read).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );

    // Only Red sets `public` + consents -> match MUST stay `private` and ineligible!
    let red_priv_json: serde_json::Value = json_body(
        save_choices(
            &app,
            &red.access_token,
            &inv.match_id,
            Visibility::Public,
            [true; 3],
        )
        .await,
    )
    .await;
    assert_eq!(red_priv_json["effective_visibility"], "private");
    assert_eq!(red_priv_json["eligible_for_internal_training"], false);
    assert_eq!(red_priv_json["eligible_for_dataset_release"], false);

    // Now Black ALSO opts in -> match becomes `public` and eligible!
    let blk_priv_json: serde_json::Value = json_body(
        save_choices(
            &app,
            &blk.access_token,
            &inv.match_id,
            Visibility::Public,
            [true; 3],
        )
        .await,
    )
    .await;
    assert_eq!(blk_priv_json["effective_visibility"], "public");
    assert_eq!(blk_priv_json["eligible_for_internal_training"], true);
    assert_eq!(blk_priv_json["eligible_for_dataset_release"], true);

    // Outsider can now read the explicitly shared public match
    let outsider_read_ok = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/matches/{}", inv.match_id))
        .header("authorization", format!("Bearer {}", outsider.access_token))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(outsider_read_ok)
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    // 7. Draw offer + accept flow!
    let offer = MatchCommandRequest::OfferDraw {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "cmd_offer_1".to_string(),
        expected_seq: 2,
    };
    let offer_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&offer).unwrap()))
        .unwrap();
    let snap_offer: MatchSnapshotResponse =
        json_body(app.clone().oneshot(offer_req).await.unwrap()).await;
    assert_eq!(snap_offer.seq, 3);
    assert_eq!(snap_offer.draw_offered_by, Some(Side::Red));

    let accept = MatchCommandRequest::AcceptDraw {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "cmd_accept_1".to_string(),
        expected_seq: 3,
    };
    let accept_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&accept).unwrap()))
        .unwrap();
    let snap_done: MatchSnapshotResponse =
        json_body(app.clone().oneshot(accept_req).await.unwrap()).await;
    assert_eq!(snap_done.status, "finished");
    assert_eq!(snap_done.outcome.as_deref(), Some("draw"));
    assert_eq!(snap_done.reason_code.as_deref(), Some("agreed_draw"));

    // 8. A participant's retry after the match finished stays idempotent and still receives the snapshot!
    let accept_retry_resp = post_command(&app, &blk.access_token, &inv.match_id, &accept).await;
    assert_eq!(accept_retry_resp.status(), StatusCode::OK);
    let snap_retry: MatchSnapshotResponse = json_body(accept_retry_resp).await;
    assert_eq!(snap_retry.seq, snap_done.seq);
    assert_eq!(snap_retry.red_remaining_ms, snap_done.red_remaining_ms);
    assert_eq!(snap_retry.black_remaining_ms, snap_done.black_remaining_ms);

    // 8b. Sharing grants read access, never command authority: while the match is public, the
    //     outsider replaying a known `command_id` still receives no snapshot!
    assert_replay_refused(&app, &outsider.access_token, &inv.match_id, &accept).await;

    // 9. Red revokes sharing; the outsider replaying a known `command_id` still receives no snapshot!
    let revoke_json =
        set_visibility(&app, &red.access_token, &inv.match_id, Visibility::Private).await;
    assert_eq!(revoke_json["effective_visibility"], "private");
    assert_replay_refused(&app, &outsider.access_token, &inv.match_id, &accept).await;
}

#[tokio::test]
async fn live_websocket_two_client_fanout_origin_check_and_resignation() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state);

    let (red, _) = new_guest(&app, "WS Red").await;
    let (blk, _) = new_guest(&app, "WS Black").await;

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "preferred_side": "red", "clock_initial_ms": 600_000 }).to_string(),
        ))
        .unwrap();
    let inv: InvitationCreatedResponse =
        json_body(app.clone().oneshot(create_req).await.unwrap()).await;

    let join_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/invitations/{}/join", inv.invitation_code))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let _: JoinInvitationResponse = json_body(app.clone().oneshot(join_req).await.unwrap()).await;

    // 1. Unauthenticated WebSocket upgrade without Origin MUST fail 401 (missing Origin is NEVER auth)!
    let ws_url = format!("ws://localhost:3000/api/v1/matches/{}/ws", inv.match_id);
    let unauth_req = ws_url.clone().into_client_request().unwrap();
    let unauth_res = connect_ws_duplex(&app, unauth_req).await;
    assert!(unauth_res.is_err(), "unauthenticated WS must be rejected");

    // 2. Disallowed Origin MUST fail 403!
    let mut bad_origin_req = ws_url.clone().into_client_request().unwrap();
    bad_origin_req
        .headers_mut()
        .insert("Origin", "https://evil.example.com".parse().unwrap());
    bad_origin_req.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", red.access_token).parse().unwrap(),
    );
    assert!(
        connect_ws_duplex(&app, bad_origin_req).await.is_err(),
        "disallowed WS Origin must be rejected"
    );

    // 3. Connect both Red and Black over WebSockets and verify live fanout + resignation!
    let mut red_ws_req = ws_url.clone().into_client_request().unwrap();
    red_ws_req
        .headers_mut()
        .insert("Origin", "http://localhost:3000".parse().unwrap());
    red_ws_req.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", red.access_token).parse().unwrap(),
    );
    let mut red_ws = connect_ws_duplex(&app, red_ws_req).await.unwrap();

    let mut blk_ws_req = ws_url.into_client_request().unwrap();
    blk_ws_req.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", blk.access_token).parse().unwrap(),
    );
    let mut blk_ws = connect_ws_duplex(&app, blk_ws_req).await.unwrap();

    // Consume initial snapshots
    let _ = red_ws.next().await.unwrap().unwrap();
    let _ = blk_ws.next().await.unwrap().unwrap();

    // Red submits move over WS -> Black receives live broadcast over WS!
    let cmd_move = MatchCommandRequest::Move {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "ws_m1".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 0],
            to: [6, 2],
        },
    };
    red_ws
        .send(Message::Text(
            serde_json::to_string(&cmd_move).unwrap().into(),
        ))
        .await
        .unwrap();

    let blk_live_msg = blk_ws.next().await.unwrap().unwrap();
    let blk_snap: MatchSnapshotResponse =
        serde_json::from_str(blk_live_msg.to_text().unwrap()).unwrap();
    assert_eq!(blk_snap.seq, 1);
    assert_eq!(blk_snap.moves_uci, vec!["h0g2"]);
    let _ = red_ws.next().await.unwrap().unwrap();

    // Black resigns over WS -> Red receives `finished` (`red_win` by `resignation`)!
    let cmd_resign = MatchCommandRequest::Resign {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "ws_resign".to_string(),
        expected_seq: 1,
    };
    blk_ws
        .send(Message::Text(
            serde_json::to_string(&cmd_resign).unwrap().into(),
        ))
        .await
        .unwrap();

    let red_fin_msg = red_ws.next().await.unwrap().unwrap();
    let red_fin_snap: MatchSnapshotResponse =
        serde_json::from_str(red_fin_msg.to_text().unwrap()).unwrap();
    assert_eq!(red_fin_snap.status, "finished");
    assert_eq!(red_fin_snap.outcome.as_deref(), Some("red_win"));
    assert_eq!(red_fin_snap.reason_code.as_deref(), Some("resignation"));
}

#[tokio::test]
async fn proactive_flag_fall_draw_offer_recovery_and_spectator_ws_revocation() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state.clone());

    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let (spec, _) = new_guest(&app, "Spectator").await;

    // 1. Proactive automatic flag-fall timeout with zero incoming commands (`80ms` initial clock)!
    let fast_create = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "preferred_side": "red",
                "clock_initial_ms": 80,
                "clock_increment_ms": 0
            })
            .to_string(),
        ))
        .unwrap();
    let fast_inv: InvitationCreatedResponse =
        json_body(app.clone().oneshot(fast_create).await.unwrap()).await;

    let fast_join = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/invitations/{}/join",
            fast_inv.invitation_code
        ))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let _: JoinInvitationResponse = json_body(app.clone().oneshot(fast_join).await.unwrap()).await;

    // Sleep past 80ms without sending any commands; actor deadline timer must commit & broadcast timeout!
    tokio::time::sleep(std::time::Duration::from_millis(160)).await;

    let snap_req = Request::builder()
        .method("GET")
        .uri(format!("/api/v1/matches/{}", fast_inv.match_id))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let timed_out: MatchSnapshotResponse =
        json_body(app.clone().oneshot(snap_req).await.unwrap()).await;
    assert_eq!(timed_out.status, "finished");
    assert_eq!(timed_out.outcome.as_deref(), Some("black_win"));
    assert_eq!(timed_out.reason_code.as_deref(), Some("timeout"));

    // 2. Draw offer recovery across actor eviction + spectator WebSocket revocation on privacy withdrawal!
    let g2_create = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({ "preferred_side": "red", "clock_initial_ms": 600_000 }).to_string(),
        ))
        .unwrap();
    let g2: InvitationCreatedResponse =
        json_body(app.clone().oneshot(g2_create).await.unwrap()).await;
    let g2_join = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/invitations/{}/join", g2.invitation_code))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let _: JoinInvitationResponse = json_body(app.clone().oneshot(g2_join).await.unwrap()).await;

    // Both opt into `public` so Spectator can connect a WebSocket
    for tok in [&red.access_token, &blk.access_token] {
        let p = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/matches/{}/privacy", g2.match_id))
            .header("authorization", format!("Bearer {tok}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&PrivacyUpdateRequest {
                    visibility: Some(Visibility::Public),
                    allow_internal_training: Some(true),
                    allow_published_research: Some(false),
                    allow_dataset_release: Some(false),
                    expected_revision: Some(current_revision(&app, tok, &g2.match_id).await),
                })
                .unwrap(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(p).await.unwrap().status(),
            StatusCode::OK
        );
    }

    let ws_url = format!("ws://localhost:3000/api/v1/matches/{}/ws", g2.match_id);
    let mut spec_ws_req = ws_url.into_client_request().unwrap();
    spec_ws_req.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", spec.access_token).parse().unwrap(),
    );
    let mut spec_ws = connect_ws_duplex(&app, spec_ws_req).await.unwrap();
    let _initial = spec_ws.next().await.unwrap().unwrap();

    // Red revokes visibility back to `Private`: Spectator WS must be closed without receiving private updates!
    let revoke_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/privacy", g2.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&PrivacyUpdateRequest {
                visibility: Some(Visibility::Private),
                allow_internal_training: Some(false),
                allow_published_research: Some(false),
                allow_dataset_release: Some(false),
                expected_revision: Some(
                    current_revision(&app, &red.access_token, &g2.match_id).await,
                ),
            })
            .unwrap(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(revoke_req).await.unwrap().status(),
        StatusCode::OK
    );

    let next_spec_frame = spec_ws.next().await;
    assert!(
        matches!(
            next_spec_frame,
            None | Some(Ok(Message::Close(_))) | Some(Err(_))
        ),
        "spectator WS must close when match becomes private, got {next_spec_frame:?}"
    );

    // Red offers draw, then we evict in-memory actors (`state.matches.write().await.clear()`) to simulate restart!
    let offer_cmd = MatchCommandRequest::OfferDraw {
        protocol: 1,
        match_id: g2.match_id.clone(),
        command_id: "offer_1".to_string(),
        expected_seq: 0,
    };
    let offer_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", g2.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&offer_cmd).unwrap()))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(offer_req).await.unwrap().status(),
        StatusCode::OK
    );

    state.matches.write().await.clear();

    // Black accepts draw against freshly reconstructed actor -> MUST succeed!
    let accept_cmd = MatchCommandRequest::AcceptDraw {
        protocol: 1,
        match_id: g2.match_id.clone(),
        command_id: "accept_1".to_string(),
        expected_seq: 1,
    };
    let accept_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/commands", g2.match_id))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&accept_cmd).unwrap()))
        .unwrap();
    let accept_resp = app.clone().oneshot(accept_req).await.unwrap();
    assert_eq!(accept_resp.status(), StatusCode::OK);
    let fin_snap: MatchSnapshotResponse = json_body(accept_resp).await;
    assert_eq!(fin_snap.status, "finished");
    assert_eq!(fin_snap.reason_code.as_deref(), Some("agreed_draw"));
}

#[tokio::test]
async fn slice_b_takeback_rematch_pgn_export_app_links_and_outage_interruption() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state.clone());

    // 1. Universal Links & Android App Links `.well-known` association endpoints
    let apple_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/apple-app-site-association")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(apple_resp.status(), StatusCode::OK);
    let android_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/.well-known/assetlinks.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(android_resp.status(), StatusCode::OK);

    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;

    // 2. Friendly match: play `h2e2`, mutual takeback (`RequestTakeback` + `AcceptTakeback`) restores startpos!
    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/invitations")
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({
                "preferred_side_mode": "red",
                "clock_initial_ms": 300_000,
                "clock_increment_ms": 3_000,
                "is_friendly": true
            })
            .to_string(),
        ))
        .unwrap();
    let inv: InvitationCreatedResponse =
        json_body(app.clone().oneshot(create_req).await.unwrap()).await;

    let join_req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/invitations/{}/join", inv.invitation_code))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let _: JoinInvitationResponse = json_body(app.clone().oneshot(join_req).await.unwrap()).await;

    let m1 = MatchCommandRequest::Move {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "m1_tb".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 2],
            to: [4, 2],
        },
    };
    let m1_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
                .header("authorization", format!("Bearer {}", red.access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&m1).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(m1_resp.status(), StatusCode::OK);

    let tb_req = MatchCommandRequest::RequestTakeback {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "tb_req_1".to_string(),
        expected_seq: 1,
    };
    let tb_req_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
                .header("authorization", format!("Bearer {}", red.access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&tb_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let tb_snap: MatchSnapshotResponse = json_body(tb_req_resp).await;
    assert_eq!(tb_snap.takeback_requested_by, Some(Side::Red));

    let tb_acc = MatchCommandRequest::AcceptTakeback {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "tb_acc_1".to_string(),
        expected_seq: 2,
    };
    let tb_acc_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
                .header("authorization", format!("Bearer {}", blk.access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&tb_acc).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let undone_snap: MatchSnapshotResponse = json_body(tb_acc_resp).await;
    assert!(undone_snap.moves_uci.is_empty());
    assert_eq!(undone_snap.side_to_move, Side::Red);

    // 3. Re-play move, export participant's own match PGN, then simulate server outage interruption + linked side-reversed rematch!
    let m2 = MatchCommandRequest::Move {
        protocol: 1,
        match_id: inv.match_id.clone(),
        command_id: "m2_after_tb".to_string(),
        expected_seq: 3,
        mv: CoordMovePayload {
            from: [7, 2],
            to: [4, 2],
        },
    };
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/matches/{}/commands", inv.match_id))
                    .header("authorization", format!("Bearer {}", red.access_token))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&m2).unwrap()))
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let pgn_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/matches/{}/export.pgn", inv.match_id))
                .header("authorization", format!("Bearer {}", red.access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pgn_resp.status(), StatusCode::OK);
    let pgn_bytes = pgn_resp.into_body().collect().await.unwrap().to_bytes();
    let pgn_txt = std::str::from_utf8(&pgn_bytes).unwrap();
    assert!(pgn_txt.contains("h2e2 {炮二平五}"));

    // Simulate server outage restart: active timed match is marked `interrupted` without awarding a loss!
    let interrupted_count = jade_match_server::mark_interrupted_timed_matches_on_startup(&state.db)
        .await
        .unwrap();
    assert_eq!(interrupted_count, 1);

    // Red and Black both click Rematch -> idempotently land in the SAME rematch with reversed seats!
    let rem_red: InvitationCreatedResponse = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/matches/{}/rematch", inv.match_id))
                    .header("authorization", format!("Bearer {}", red.access_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let rem_blk: InvitationCreatedResponse = json_body(
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/matches/{}/rematch", inv.match_id))
                    .header("authorization", format!("Bearer {}", blk.access_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(rem_red.match_id, rem_blk.match_id);

    // Verify former Red was seated as Black in the rematch, and former Black joins into Red!
    let join_rem_blk: JoinInvitationResponse = json_body(
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/invitations/{}/join",
                    rem_red.invitation_code
                ))
                .header("authorization", format!("Bearer {}", blk.access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(join_rem_blk.assigned_side, Some(Side::Red));
    assert_eq!(join_rem_blk.snapshot.status, "playing");
}

#[tokio::test]
async fn exclusive_db_ownership_concurrent_rematch_and_spectator_rematch_protection() {
    let db = durable_test_db("owner").await;
    let sqlite_url = db.url.clone();

    let state1 = build_app_state(&sqlite_url).await.unwrap();
    let app1 = build_router(state1.clone());

    let (red, _) = new_guest(&app1, "Owner Red").await;
    let (blk, _) = new_guest(&app1, "Owner Black").await;
    let (spec, _) = new_guest(&app1, "Spectator").await;

    // Start a timed match on state1
    let inv: InvitationCreatedResponse = json_body(
        app1.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/invitations")
                    .header("authorization", format!("Bearer {}", red.access_token))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "preferred_side": "red",
                            "clock_initial_ms": 300_000,
                            "clock_increment_ms": 3_000
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    let _: JoinInvitationResponse = json_body(
        app1.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/invitations/{}/join", inv.invitation_code))
                    .header("authorization", format!("Bearer {}", blk.access_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;

    // 1. Duplicate server startup against the live DB MUST fail before interrupting active matches!
    let dup_res = build_app_state(&sqlite_url).await;
    assert!(
        dup_res.is_err(),
        "duplicate build_app_state against owned DB must fail"
    );
    let live_snap: MatchSnapshotResponse = json_body(
        app1.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/matches/{}", inv.match_id))
                    .header("authorization", format!("Bearer {}", red.access_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        live_snap.status, "playing",
        "failed duplicate startup must not interrupt live match"
    );

    // Make match `public` so spectators can view it once finished/interrupted
    for tok in [&red.access_token, &blk.access_token] {
        let _ = save_choices(&app1, tok, &inv.match_id, Visibility::Public, [false; 3]).await;
    }

    // Now drop `app1` and `state1` (simulating process exit) -> restarted server acquires lock & interrupts orphaned timed match!
    drop(app1);
    drop(state1);

    let state2 = build_app_state(&sqlite_url).await.unwrap();
    let app2 = build_router(state2);

    // 2. Simultaneous rematch requests from Red and Black serialize cleanly without SQLITE_BUSY!
    let fut_red = app2.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/matches/{}/rematch", inv.match_id))
            .header("authorization", format!("Bearer {}", red.access_token))
            .body(Body::empty())
            .unwrap(),
    );
    let fut_blk = app2.clone().oneshot(
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/matches/{}/rematch", inv.match_id))
            .header("authorization", format!("Bearer {}", blk.access_token))
            .body(Body::empty())
            .unwrap(),
    );
    let (r_red, r_blk) = tokio::join!(fut_red, fut_blk);
    let r_red = r_red.unwrap();
    let r_blk = r_blk.unwrap();
    assert_eq!(r_red.status(), StatusCode::OK);
    assert_eq!(r_blk.status(), StatusCode::OK);
    let c_red: InvitationCreatedResponse = json_body(r_red).await;
    let c_blk: InvitationCreatedResponse = json_body(r_blk).await;
    assert_eq!(c_red.match_id, c_blk.match_id);
    assert_eq!(c_red.invitation_code, c_blk.invitation_code);

    // 3. Spectator reading the public finished match sees `next_rematch_invitation_code == None`,
    // whereas participants see `Some(code)`, AND spectator cannot claim a seat in the private rematch!
    let spec_snap: MatchSnapshotResponse = json_body(
        app2.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/matches/{}", inv.match_id))
                    .header("authorization", format!("Bearer {}", spec.access_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(spec_snap.next_rematch_invitation_code, None);

    let part_snap: MatchSnapshotResponse = json_body(
        app2.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/matches/{}", inv.match_id))
                    .header("authorization", format!("Bearer {}", blk.access_token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        part_snap.next_rematch_invitation_code.as_deref(),
        Some(c_red.invitation_code.as_str())
    );

    let hijack_resp = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v1/invitations/{}/join",
                    c_red.invitation_code
                ))
                .header("authorization", format!("Bearer {}", spec.access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(hijack_resp.status(), StatusCode::FORBIDDEN);

    drop(db);
}

/// Reads a match snapshot as the holder of `cookie` (or anonymously).
async fn snapshot_as(
    app: &axum::Router,
    cookie: Option<&str>,
    match_id: &str,
) -> axum::response::Response {
    let mut req = Request::builder().uri(format!("/api/v1/matches/{match_id}"));
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn session_as(app: &axum::Router, cookie: Option<&str>) -> axum::response::Response {
    let mut req = Request::builder().uri("/api/v1/auth/session");
    if let Some(cookie) = cookie {
        req = req.header("cookie", cookie);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn browser_session_recovery_expiry_and_each_participants_saved_choices() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let db = state.db.clone();
    let app = build_router(state);

    let (red, red_cookie) = new_guest(&app, "Red Host").await;
    let (blk, blk_cookie) = new_guest(&app, "Black Joiner").await;
    let (_spec, spec_cookie) = new_guest(&app, "Spectator").await;

    // 1. A returning tab recovers its own session and CSRF token from the cookie, creating nothing. The answer is
    //    never cached, and it reissues the cookie with the session's remaining lifetime, not a fresh one.
    let resp = session_as(&app, Some(&red_cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("cache-control").unwrap(), "no-store");
    let reissued = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let max_age: i64 = reissued.split("Max-Age=").nth(1).unwrap().parse().unwrap();
    assert!(max_age > 0 && max_age <= 604_800, "{reissued}");
    assert!(
        reissued.starts_with(&red_cookie),
        "the same session cookie is reissued: {reissued}"
    );
    let recovered: CurrentSessionResponse = json_body(resp).await;
    assert_eq!(recovered.account_id, red.account_id);
    assert_eq!(recovered.csrf_token, red.csrf_token);
    assert_eq!(recovered.display_name, "Red Host");
    assert!(recovered.expires_at_ms > 0);
    assert_eq!(
        session_as(&app, None).await.status(),
        StatusCode::UNAUTHORIZED
    );

    // Recovery reads the cookie alone: another guest's bearer token on the same request changes nothing.
    let mixed = Request::builder()
        .uri("/api/v1/auth/session")
        .header("cookie", &red_cookie)
        .header("authorization", format!("Bearer {}", blk.access_token))
        .body(Body::empty())
        .unwrap();
    let mixed: CurrentSessionResponse = json_body(app.clone().oneshot(mixed).await.unwrap()).await;
    assert_eq!(mixed.account_id, red.account_id);
    assert_eq!(mixed.csrf_token, red.csrf_token);

    // 2. A match between the two guests; each saves different choices.
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    assert_eq!(
        save_choices(
            &app,
            &red.access_token,
            &match_id,
            Visibility::Public,
            [true, false, true]
        )
        .await
        .status(),
        StatusCode::OK
    );

    // 3. Each participant sees their own saved choices; the effective visibility still needs both. A snapshot is
    //    personal, so it is never cached.
    let red_resp = snapshot_as(&app, Some(&red_cookie), &match_id).await;
    assert_eq!(red_resp.headers().get("cache-control").unwrap(), "no-store");
    let red_view: MatchSnapshotResponse = json_body(red_resp).await;
    let red_choices = red_view.viewer_choices.expect("red's own choices");
    assert_eq!(red_choices.visibility, Visibility::Public);
    assert!(red_choices.allow_internal_training && !red_choices.allow_published_research);
    assert!(red_choices.allow_dataset_release);
    assert_eq!(red_view.visibility, Visibility::Private);
    let blk_view: MatchSnapshotResponse =
        json_body(snapshot_as(&app, Some(&blk_cookie), &match_id).await).await;
    let blk_choices = blk_view.viewer_choices.expect("black's own choices");
    assert_eq!(blk_choices.visibility, Visibility::Private);
    assert!(!blk_choices.allow_internal_training);

    // A move's reply is the mover's own view, with their choices.
    let move1 = MatchCommandRequest::Move {
        protocol: 1,
        match_id: match_id.clone(),
        command_id: "cmd_choices_1".to_string(),
        expected_seq: red_view.seq,
        mv: CoordMovePayload {
            from: [7, 2],
            to: [4, 2],
        },
    };
    let moved = post_command(&app, &red.access_token, &match_id, &move1).await;
    assert_eq!(moved.status(), StatusCode::OK);
    let moved: MatchSnapshotResponse = json_body(moved).await;
    assert_eq!(
        moved
            .viewer_choices
            .expect("the mover's choices")
            .visibility,
        Visibility::Public
    );

    // 4. Once both choose public, a spectator may read it but sees no participant's choices.
    set_visibility(&app, &blk.access_token, &match_id, Visibility::Public).await;
    let spec_resp = snapshot_as(&app, Some(&spec_cookie), &match_id).await;
    assert_eq!(spec_resp.status(), StatusCode::OK);
    let spec_view: MatchSnapshotResponse = json_body(spec_resp).await;
    assert_eq!(spec_view.visibility, Visibility::Public);
    assert!(spec_view.viewer_choices.is_none());

    // A save names only what changed: withdrawing one consent keeps the others and the visibility as saved, so a
    // save from another tab that changed something else cannot bring a withdrawn consent back.
    let privacy_post = |body: serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/matches/{match_id}/privacy"))
            .header("authorization", format!("Bearer {}", red.access_token))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let revision = current_revision(&app, &red.access_token, &match_id).await;
    let withdraw = app
        .clone()
        .oneshot(privacy_post(
            serde_json::json!({ "allow_internal_training": false, "expected_revision": revision }),
        ))
        .await
        .unwrap();
    assert_eq!(withdraw.status(), StatusCode::OK);

    // A patch with a misspelled field or an explicit null is refused whole, even beside a valid change, and so is
    // a JSON array (which would assign choices by position) or an object naming no choice.
    let revision = current_revision(&app, &red.access_token, &match_id).await;
    for body in [
        serde_json::json!({ "allow_dataset_release": false, "allow_dataset_releases": false, "expected_revision": revision }),
        serde_json::json!({ "allow_dataset_release": false, "allow_published_research": null, "expected_revision": revision }),
        serde_json::json!(["private", false, false, false, revision]),
    ] {
        let refused = app
            .clone()
            .oneshot(privacy_post(body.clone()))
            .await
            .unwrap();
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{body}");
        let refused: serde_json::Value = json_body(refused).await;
        assert_eq!(refused["error"]["code"], "invalid_privacy_update", "{body}");
    }
    // A valid object sent with another media type is refused too.
    let wrong_type = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{match_id}/privacy"))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "text/plain")
        .body(Body::from(
            serde_json::json!({ "allow_dataset_release": false, "expected_revision": revision })
                .to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(wrong_type).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    let revision = current_revision(&app, &red.access_token, &match_id).await;
    let empty = app
        .clone()
        .oneshot(privacy_post(
            serde_json::json!({ "expected_revision": revision }),
        ))
        .await
        .unwrap();
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    let empty: serde_json::Value = json_body(empty).await;
    assert_eq!(empty["error"]["code"], "no_choice_named");

    // A save without a revision is refused, so a page from before revisions existed cannot write unordered.
    let unordered = app
        .clone()
        .oneshot(privacy_post(
            serde_json::json!({ "allow_dataset_release": false }),
        ))
        .await
        .unwrap();
    assert_eq!(unordered.status(), StatusCode::PRECONDITION_REQUIRED);

    // A choice named twice is ambiguous and refused; a JSON value cannot hold the duplicate, so the body is raw.
    let duplicate = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{match_id}/privacy"))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(format!(
            r#"{{"allow_dataset_release": true, "allow_dataset_release": false, "expected_revision": {revision}}}"#
        )))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(duplicate).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let unchanged: MatchSnapshotResponse =
        json_body(snapshot_as(&app, Some(&red_cookie), &match_id).await).await;
    let unchanged = unchanged.viewer_choices.expect("red's own choices");
    assert!(
        unchanged.allow_dataset_release,
        "a refused patch applies nothing"
    );

    // A save naming an older revision is refused, so a delayed save cannot overwrite a newer one.
    let current = unchanged.revision;
    let stale = app
        .clone()
        .oneshot(privacy_post(
            serde_json::json!({ "allow_dataset_release": false, "expected_revision": current - 1 }),
        ))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale: serde_json::Value = json_body(stale).await;
    assert_eq!(stale["error"]["code"], "stale_choices");
    let kept: MatchSnapshotResponse =
        json_body(snapshot_as(&app, Some(&red_cookie), &match_id).await).await;
    let kept = kept.viewer_choices.expect("red's own choices");
    assert!(
        kept.allow_dataset_release,
        "a refused stale save changes nothing"
    );
    assert_eq!(kept.revision, current);
    let fresh = app
        .clone()
        .oneshot(privacy_post(
            serde_json::json!({ "allow_dataset_release": false, "expected_revision": current }),
        ))
        .await
        .unwrap();
    assert_eq!(fresh.status(), StatusCode::OK);
    let fresh: serde_json::Value = json_body(fresh).await;
    assert_eq!(fresh["choices_revision"], current + 1);
    let restore = app
        .clone()
        .oneshot(privacy_post(
            serde_json::json!({ "allow_dataset_release": true, "expected_revision": current + 1 }),
        ))
        .await
        .unwrap();
    assert_eq!(restore.status(), StatusCode::OK);
    let after: MatchSnapshotResponse =
        json_body(snapshot_as(&app, Some(&red_cookie), &match_id).await).await;
    let after = after.viewer_choices.expect("red's own choices");
    assert!(!after.allow_internal_training);
    assert!(after.allow_dataset_release);
    assert_eq!(after.visibility, Visibility::Public);

    // 5. Revocation by either participant removes the spectator's access again.
    set_visibility(&app, &blk.access_token, &match_id, Visibility::Private).await;
    assert_eq!(
        snapshot_as(&app, Some(&spec_cookie), &match_id)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );

    // 6. A revoked cookie session recovers nothing.
    sqlx::query("UPDATE auth_sessions SET revoked_at_ms = 1 WHERE account_id = $1")
        .bind(&blk.account_id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        session_as(&app, Some(&blk_cookie)).await.status(),
        StatusCode::UNAUTHORIZED
    );

    // A partly elapsed session's cookie is reissued with what remains, and the server's expiry is not extended.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let soon = now_ms + 90_000;
    sqlx::query("UPDATE auth_sessions SET expires_at_ms = $1 WHERE account_id = $2")
        .bind(soon)
        .bind(&red.account_id)
        .execute(&db)
        .await
        .unwrap();
    let partial = session_as(&app, Some(&red_cookie)).await;
    let reissued = partial
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let max_age: i64 = reissued.split("Max-Age=").nth(1).unwrap().parse().unwrap();
    assert!((1..=90).contains(&max_age), "{reissued}");
    let partial: CurrentSessionResponse = json_body(partial).await;
    assert_eq!(partial.expires_at_ms, soon);

    // 7. An expired session recovers nothing and no longer reads the private replay.
    sqlx::query("UPDATE auth_sessions SET expires_at_ms = 0 WHERE account_id = $1")
        .bind(&red.account_id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        session_as(&app, Some(&red_cookie)).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        snapshot_as(&app, Some(&red_cookie), &match_id)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_existing_database_gains_the_choice_revisions_at_startup() {
    // A database created by the schema before the revisions existed: the same tables, without the two columns.
    let dir = std::env::temp_dir().join(format!("jade-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.sqlite3");
    let _ = std::fs::remove_file(&path);
    let old_schema = include_str!("../../../db/game/0001_initial_game_schema.sql")
        .lines()
        .filter(|line| {
            !line.contains("_choices_revision")
                && !line.contains("advance a revision")
                && !line.contains("state_version")
                && !line.contains("Advances with every change to the match")
                && !line.contains("turn_generation")
                && !line.contains("Advances only when a new turn begins")
        })
        .collect::<Vec<_>>()
        .join("\n");
    {
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(&url)
            .await
            .unwrap();
        sqlx::raw_sql(&old_schema).execute(&pool).await.unwrap();
        let before: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('matches')")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(!before.iter().any(|c| c.ends_with("_choices_revision")));
        pool.close().await;
    }
    let state = build_app_state(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    let after: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('matches')")
        .fetch_all(&state.db)
        .await
        .unwrap();
    assert!(after.iter().any(|c| c == "red_choices_revision"));
    assert!(after.iter().any(|c| c == "black_choices_revision"));
    assert!(after.iter().any(|c| c == "state_version"));
    assert!(after.iter().any(|c| c == "turn_generation"));
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn concurrent_saves_of_one_revision_apply_once() {
    // A durable database, so the two saves run on separate connections and compete for the write lock.
    let db = durable_test_db("concurrent").await;
    let state = build_app_state(&db.url).await.unwrap();
    let app = build_router(state);
    let (red, _red_cookie) = new_guest(&app, "Red").await;
    let (blk, _blk_cookie) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    let revision = current_revision(&app, &red.access_token, &match_id).await;
    let save = |consent: bool| {
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/matches/{match_id}/privacy"))
            .header("authorization", format!("Bearer {}", red.access_token))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "allow_dataset_release": consent, "expected_revision": revision }).to_string(),
            ))
            .unwrap()
    };
    let (first, second) = tokio::join!(
        app.clone().oneshot(save(true)),
        app.clone().oneshot(save(false))
    );
    let mut statuses = [first.unwrap().status(), second.unwrap().status()];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::OK, StatusCode::CONFLICT],
        "one applies, the other is stale, none errors"
    );
    assert_eq!(
        current_revision(&app, &red.access_token, &match_id).await,
        revision + 1
    );
    drop(app);
    drop(db);
}

#[tokio::test]
async fn the_state_version_advances_with_every_change_by_either_participant() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state);
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    let joined = snapshot_for(&app, &red.access_token, &match_id)
        .await
        .state_version;
    assert!(joined > 0, "a seat joining is a change");

    // Red's save advances Red's own revision and the match's version; the reply carries the version it wrote.
    let saved: serde_json::Value = json_body(
        save_choices(
            &app,
            &red.access_token,
            &match_id,
            Visibility::Public,
            [false; 3],
        )
        .await,
    )
    .await;
    assert_eq!(saved["choices_revision"], 1);
    let after_red = saved["state_version"].as_i64().unwrap();
    assert!(after_red > joined);
    assert_eq!(
        snapshot_for(&app, &red.access_token, &match_id)
            .await
            .state_version,
        after_red
    );

    // Black's save changes the effective visibility without touching Red's revision; Red's next snapshot still
    // carries a newer version, so a client can order it after the earlier one.
    save_choices(
        &app,
        &blk.access_token,
        &match_id,
        Visibility::Public,
        [false; 3],
    )
    .await;
    let red_view = snapshot_for(&app, &red.access_token, &match_id).await;
    assert_eq!(red_view.visibility, Visibility::Public);
    assert!(red_view.state_version > after_red);
    assert_eq!(red_view.viewer_choices.unwrap().revision, 1);

    // Black withdraws: the match is private again, at a newer version than the public snapshot.
    let public_version = red_view.state_version;
    save_choices(
        &app,
        &blk.access_token,
        &match_id,
        Visibility::Private,
        [false; 3],
    )
    .await;
    let red_view = snapshot_for(&app, &red.access_token, &match_id).await;
    assert_eq!(red_view.visibility, Visibility::Private);
    assert!(red_view.state_version > public_version);
    assert_eq!(red_view.viewer_choices.unwrap().revision, 1);

    // A move is a change too.
    let before_move = red_view.state_version;
    let mv = MatchCommandRequest::Move {
        protocol: 1,
        match_id: match_id.clone(),
        command_id: "v1".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 0],
            to: [6, 2],
        },
    };
    assert_eq!(
        post_command(&app, &red.access_token, &match_id, &mv)
            .await
            .status(),
        StatusCode::OK
    );
    assert!(
        snapshot_for(&app, &red.access_token, &match_id)
            .await
            .state_version
            > before_move
    );
}

/// Opens the match's live stream as the bearer of `token` and reads its first snapshot.
async fn open_stream(
    app: &axum::Router,
    token: &str,
    match_id: &str,
) -> WebSocketStream<DuplexStream> {
    let mut req = format!("ws://localhost:3180/api/v1/matches/{match_id}/ws")
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse().unwrap());
    let mut ws = connect_ws_duplex(app, req).await.unwrap();
    let first = ws.next().await.unwrap().unwrap();
    assert!(first.is_text(), "the stream opens with a snapshot");
    ws
}

/// What a live stream delivers next: a snapshot's sequence number, or `None` once the server closes or ends the
/// stream. Ping and pong frames are passed over; any other frame fails the test.
async fn next_delivery(ws: &mut WebSocketStream<DuplexStream>) -> Option<u64> {
    // One deadline for the whole wait, so control frames cannot extend it.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    let snap: MatchSnapshotResponse = serde_json::from_str(&text).unwrap();
                    return Some(snap.seq);
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return None,
                Some(Ok(other)) => panic!("unexpected frame on a match stream: {other:?}"),
            }
        }
    })
    .await
    .expect("the server answers within five seconds")
}

#[tokio::test]
async fn an_open_stream_ends_with_its_session() {
    use sha2::Digest;
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let db = state.db.clone();
    let app = build_router(state);
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let hash = |token: &str| hex::encode(sha2::Sha256::digest(token.as_bytes()));
    let mv = |match_id: &str, command_id: &str, seq: u64, from: [u8; 2], to: [u8; 2]| {
        MatchCommandRequest::Move {
            protocol: 1,
            match_id: match_id.to_string(),
            command_id: command_id.to_string(),
            expected_seq: seq,
            mv: CoordMovePayload { from, to },
        }
    };

    // 1. Red's session expires while Red's stream is open: Black's move reaches Black's stream, never Red's.
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    let mut red_ws = open_stream(&app, &red.access_token, &match_id).await;
    let mut blk_ws = open_stream(&app, &blk.access_token, &match_id).await;
    assert_eq!(
        post_command(
            &app,
            &red.access_token,
            &match_id,
            &mv(&match_id, "r1", 0, [7, 0], [6, 2])
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(next_delivery(&mut red_ws).await, Some(1));
    assert_eq!(next_delivery(&mut blk_ws).await, Some(1));
    sqlx::query("UPDATE auth_sessions SET expires_at_ms = 0 WHERE session_id_hash = $1")
        .bind(hash(&red.access_token))
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        post_command(
            &app,
            &blk.access_token,
            &match_id,
            &mv(&match_id, "b1", 1, [7, 9], [6, 7])
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(next_delivery(&mut blk_ws).await, Some(2));
    assert_eq!(
        next_delivery(&mut red_ws).await,
        None,
        "an expired session receives no further state"
    );

    // 2. Black's session is revoked while Black's stream is open: a command on it is not acted on for Black.
    let (red2, _) = new_guest(&app, "Red 2").await;
    let (blk2, _) = new_guest(&app, "Black 2").await;
    let match_id = start_match(&app, &red2.access_token, &blk2.access_token).await;
    let mut blk_ws = open_stream(&app, &blk2.access_token, &match_id).await;
    assert_eq!(
        post_command(
            &app,
            &red2.access_token,
            &match_id,
            &mv(&match_id, "r1", 0, [7, 0], [6, 2])
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(next_delivery(&mut blk_ws).await, Some(1));
    sqlx::query("UPDATE auth_sessions SET revoked_at_ms = 1 WHERE session_id_hash = $1")
        .bind(hash(&blk2.access_token))
        .execute(&db)
        .await
        .unwrap();
    blk_ws
        .send(Message::Text(
            serde_json::to_string(&mv(&match_id, "b1", 1, [7, 9], [6, 7]))
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        next_delivery(&mut blk_ws).await,
        None,
        "a revoked session's stream ends"
    );
    let after = snapshot_for(&app, &red2.access_token, &match_id).await;
    assert_eq!(after.seq, 1, "the revoked session's move was not played");
}

/// The match snapshot as the bearer of `token` sees it.
async fn snapshot_for(app: &axum::Router, token: &str, match_id: &str) -> MatchSnapshotResponse {
    let req = Request::builder()
        .uri(format!("/api/v1/matches/{match_id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    json_body(app.clone().oneshot(req).await.unwrap()).await
}

#[tokio::test]
async fn a_session_that_ends_during_the_upgrade_gets_no_opening_snapshot() {
    use sha2::Digest;
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let db = state.db.clone();
    let app = build_router(state.clone());
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    // The session is revoked after the upgrade is accepted and before the 101 reaches the client, so the stream's
    // handler starts with a session that has already ended.
    let blk_hash = hex::encode(sha2::Sha256::digest(blk.access_token.as_bytes()));
    let revoking = build_router(state).layer(axum::middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let db = db.clone();
            let hash = blk_hash.clone();
            async move {
                let resp = next.run(req).await;
                if resp.status() == StatusCode::SWITCHING_PROTOCOLS {
                    sqlx::query(
                        "UPDATE auth_sessions SET revoked_at_ms = 1 WHERE session_id_hash = $1",
                    )
                    .bind(hash)
                    .execute(&db)
                    .await
                    .unwrap();
                }
                resp
            }
        },
    ));
    let mut req = format!("ws://localhost:3180/api/v1/matches/{match_id}/ws")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", blk.access_token).parse().unwrap(),
    );
    let mut ws = connect_ws_duplex(&revoking, req).await.unwrap();
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
        .await
        .expect("the server answers within five seconds");
    assert!(
        matches!(first, Some(Ok(Message::Close(_)))),
        "the first frame for a session that ended is a close, not {first:?}"
    );
}

#[tokio::test]
async fn a_read_only_session_check_sets_no_cookie() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url).await.unwrap();
    let app = build_router(state);
    let (_guest, cookie) = new_guest(&app, "Guest").await;
    let check = |query: &'static str| {
        Request::builder()
            .uri(format!("/api/v1/auth/session{query}"))
            .header("cookie", cookie.clone())
            .body(Body::empty())
            .unwrap()
    };
    let reissued = app.clone().oneshot(check("")).await.unwrap();
    assert_eq!(reissued.status(), StatusCode::OK);
    assert!(reissued.headers().contains_key("set-cookie"));
    let read_only = app.clone().oneshot(check("?reissue=false")).await.unwrap();
    assert_eq!(read_only.status(), StatusCode::OK);
    assert!(!read_only.headers().contains_key("set-cookie"));
}

#[tokio::test]
async fn a_snapshot_lists_events_only_up_to_its_own_sequence() {
    let TestMatch {
        test_db: _test_db,
        db,
        app,
        red,
        match_id,
    } = test_match().await;
    let mv = MatchCommandRequest::Move {
        protocol: 1,
        match_id: match_id.clone(),
        command_id: "r1".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 0],
            to: [6, 2],
        },
    };
    assert_eq!(
        post_command(&app, &red.access_token, &match_id, &mv)
            .await
            .status(),
        StatusCode::OK
    );
    // An event past the row's sequence, as a commit landing between a snapshot's read of the row and of its events
    // would leave it.
    sqlx::query(
        "INSERT INTO match_events (match_id, seq, command_id, actor_side, event_type, payload_json, created_at_ms)
         SELECT match_id, seq + 1, 'planted', actor_side, event_type, payload_json, created_at_ms
         FROM match_events WHERE match_id = $1 AND seq = 1",
    )
    .bind(&match_id)
    .execute(&db)
    .await
    .unwrap();
    let req = Request::builder()
        .uri(format!("/api/v1/matches/{match_id}?since_seq=0"))
        .header("authorization", format!("Bearer {}", red.access_token))
        .body(Body::empty())
        .unwrap();
    let snap: MatchSnapshotResponse = json_body(app.clone().oneshot(req).await.unwrap()).await;
    assert_eq!(snap.seq, 1);
    assert_eq!(
        snap.moves_uci,
        vec!["h0g2"],
        "the move list stops at the row's sequence"
    );
    assert!(
        snap.missing_events.iter().all(|e| e.seq <= 1),
        "so do the missing events"
    );
}

#[tokio::test]
async fn the_turn_generation_advances_only_when_a_new_turn_begins_and_survives_a_restart() {
    let db = durable_test_db("turns").await;
    let url = db.url.clone();
    let state = build_app_state(&url).await.unwrap();
    let app = build_router(state);
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    let generation = |snap: &MatchSnapshotResponse| snap.turn_generation;
    let view = |app: axum::Router| {
        let token = red.access_token.clone();
        let id = match_id.clone();
        async move { snapshot_for(&app, &token, &id).await }
    };
    assert_eq!(
        generation(&view(app.clone()).await),
        1,
        "play starting is the first turn"
    );
    let command = |cmd: MatchCommandRequest, token: String| {
        let app = app.clone();
        let id = match_id.clone();
        async move {
            assert_eq!(
                post_command(&app, &token, &id, &cmd).await.status(),
                StatusCode::OK
            )
        }
    };
    let mv = |command_id: &str, seq: u64| MatchCommandRequest::Move {
        protocol: 1,
        match_id: match_id.clone(),
        command_id: command_id.to_string(),
        expected_seq: seq,
        mv: CoordMovePayload {
            from: [7, 0],
            to: [6, 2],
        },
    };

    // A move starts Black's turn.
    command(mv("r1", 0), red.access_token.clone()).await;
    let after_move = view(app.clone()).await;
    assert_eq!(generation(&after_move), 2);

    // A privacy save and a takeback request each change the match but not the turn.
    assert_eq!(
        save_choices(
            &app,
            &red.access_token,
            &match_id,
            Visibility::Public,
            [false; 3]
        )
        .await
        .status(),
        StatusCode::OK
    );
    let saved = view(app.clone()).await;
    assert!(saved.state_version > after_move.state_version);
    assert_eq!(generation(&saved), 2);
    command(
        MatchCommandRequest::RequestTakeback {
            protocol: 1,
            match_id: match_id.clone(),
            command_id: "tb1".to_string(),
            expected_seq: 1,
        },
        red.access_token.clone(),
    )
    .await;
    let requested = view(app.clone()).await;
    assert!(requested.state_version > saved.state_version);
    assert_eq!(generation(&requested), 2);

    // The accepted takeback returns the turn to Red, and the same move played again starts Black's turn anew: the
    // snapshot has the side to move and move count it had before, and a later turn generation.
    command(
        MatchCommandRequest::AcceptTakeback {
            protocol: 1,
            match_id: match_id.clone(),
            command_id: "tb1-accept".to_string(),
            expected_seq: 2,
        },
        blk.access_token.clone(),
    )
    .await;
    assert_eq!(generation(&view(app.clone()).await), 3);
    command(mv("r1-again", 3), red.access_token.clone()).await;
    let replayed = view(app.clone()).await;
    assert_eq!(replayed.side_to_move, after_move.side_to_move);
    assert_eq!(replayed.moves_uci, after_move.moves_uci);
    assert_eq!(generation(&replayed), 4);

    // The generation is stored with the match, so a restarted server reports the same turn.
    drop(app);
    let restarted = build_router(build_app_state(&url).await.unwrap());
    let after_restart = snapshot_for(&restarted, &red.access_token, &match_id).await;
    assert_eq!(generation(&after_restart), 4);
    drop(restarted);
    drop(db);
}

#[tokio::test]
async fn an_existing_postgres_database_gains_the_added_counters_at_startup() {
    // Runs only against a configured PostgreSQL server; the SQLite form of this check is the test above.
    let Some(pg) = fresh_postgres().await else {
        return;
    };
    let url = pg.url.clone();
    let old_schema = include_str!("../../../db/game/0001_initial_game_schema.sql")
        .lines()
        .filter(|line| {
            !line.contains("_choices_revision")
                && !line.contains("advance a revision")
                && !line.contains("state_version")
                && !line.contains("Advances with every change to the match")
                && !line.contains("turn_generation")
                && !line.contains("Advances only when a new turn begins")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let columns = |pool: sqlx::AnyPool| async move {
        sqlx::query_scalar::<_, String>(
            "SELECT column_name::text FROM information_schema.columns WHERE table_name = 'matches'",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
    };
    {
        let pool = sqlx::AnyPool::connect(&url).await.unwrap();
        sqlx::raw_sql(&old_schema).execute(&pool).await.unwrap();
        let before = columns(pool.clone()).await;
        assert!(
            !before
                .iter()
                .any(|c| c.ends_with("_choices_revision") || c == "state_version")
        );
        pool.close().await;
    }
    let state = build_app_state(&url).await.unwrap();
    let after = columns(state.db.clone()).await;
    for added in [
        "red_choices_revision",
        "black_choices_revision",
        "state_version",
        "turn_generation",
    ] {
        assert!(after.iter().any(|c| c == added), "{added}");
    }
}

#[tokio::test]
async fn a_server_that_lost_postgres_ownership_writes_nothing_after_a_new_owner_starts() {
    let Some(pg) = fresh_postgres().await else {
        return;
    };
    let url = pg.url.clone();
    let first_state = build_app_state(&url).await.unwrap();
    let first_db = first_state.db.clone();
    let first = build_router(first_state);
    let (red, _) = new_guest(&first, "Red").await;
    let (blk, _) = new_guest(&first, "Black").await;
    // An untimed match, so the new owner's startup recovery leaves it playing and only the fence can refuse the write.
    let invite: InvitationCreatedResponse = post_json(
        &first,
        &red.access_token,
        "/api/v1/invitations".to_string(),
        serde_json::json!({ "preferred_side": "red" }),
    )
    .await;
    let joined: JoinInvitationResponse = post_json(
        &first,
        &blk.access_token,
        format!("/api/v1/invitations/{}/join", invite.invitation_code),
        serde_json::json!({}),
    )
    .await;
    let match_id = joined.match_id;

    // The first server's lock session ends, as a database restart or an idle timeout would end it, and a second
    // server takes ownership.
    let mut admin = <sqlx::AnyConnection as sqlx::Connection>::connect(&url)
        .await
        .unwrap();
    // The session runs in this test's own database before anything is ended.
    let here: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut admin)
        .await
        .unwrap();
    assert_eq!(
        pg.postgres.as_ref().map(|(_, name)| name.as_str()),
        Some(here.as_str())
    );
    // Each owner session must have ended, not just been signalled, before the second server starts.
    let ended: i64 = sqlx::query_scalar(
        "SELECT count(*) FILTER (WHERE ended) FROM (SELECT pg_terminate_backend(pid, 5000) AS ended FROM pg_locks
         WHERE locktype = 'advisory' AND granted AND pid <> pg_backend_pid()
           AND database = (SELECT oid FROM pg_database WHERE datname = current_database())) ended",
    )
    .fetch_one(&mut admin)
    .await
    .unwrap();
    assert_eq!(ended, 1, "the owner's lock session ends");
    let second = build_router(build_app_state(&url).await.unwrap());

    // The first server still answers, but its writes of match state are refused and change nothing.
    let mv = MatchCommandRequest::Move {
        protocol: 1,
        match_id: match_id.clone(),
        command_id: "stale-owner".to_string(),
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 0],
            to: [6, 2],
        },
    };
    let choices_before = snapshot_for(&second, &red.access_token, &match_id)
        .await
        .viewer_choices
        .unwrap();
    let refused = post_command(&first, &red.access_token, &match_id, &mv).await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    let refused: serde_json::Value = json_body(refused).await;
    assert_eq!(refused["error"]["code"], "ownership_lost");
    assert_eq!(
        save_choices(
            &first,
            &red.access_token,
            &match_id,
            Visibility::Public,
            [false; 3]
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let now = snapshot_for(&second, &red.access_token, &match_id).await;
    assert_eq!(now.seq, 0);
    assert_eq!(now.visibility, Visibility::Private);
    let choices_after = now.viewer_choices.unwrap();
    assert_eq!(
        choices_after.revision, choices_before.revision,
        "Red's choices were not saved"
    );
    assert_eq!(choices_after.visibility, choices_before.visibility);
    // Startup recovery run through the stale server's pool interrupts nothing the new owner is timing.
    let (red2, _) = new_guest(&second, "Red 2").await;
    let (blk2, _) = new_guest(&second, "Black 2").await;
    let timed = start_match(&second, &red2.access_token, &blk2.access_token).await;
    assert!(
        mark_interrupted_timed_matches_on_startup(&first_db)
            .await
            .is_err()
    );
    let timed_now = snapshot_for(&second, &red2.access_token, &timed).await;
    assert_eq!(timed_now.status, "playing");
    assert_eq!(timed_now.seq, 0);
    // The new owner plays the same move.
    assert_eq!(
        post_command(&second, &red.access_token, &match_id, &mv)
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn command_ids_have_one_bound_on_every_backend() {
    let TestMatch {
        test_db: _test_db,
        db,
        app,
        red,
        match_id,
    } = test_match().await;
    let mv = |id: String| MatchCommandRequest::Move {
        protocol: 1,
        match_id: match_id.clone(),
        command_id: id,
        expected_seq: 0,
        mv: CoordMovePayload {
            from: [7, 0],
            to: [6, 2],
        },
    };
    let too_long = post_command(&app, &red.access_token, &match_id, &mv("x".repeat(129))).await;
    assert_eq!(too_long.status(), StatusCode::BAD_REQUEST);
    let too_long: serde_json::Value = json_body(too_long).await;
    assert_eq!(too_long["error"]["code"], "invalid_command_id");
    assert_eq!(
        post_command(&app, &red.access_token, &match_id, &mv("x".repeat(128)))
            .await
            .status(),
        StatusCode::OK
    );
    let nul = post_command(
        &app,
        &red.access_token,
        &match_id,
        &mv("a\u{0}b".to_string()),
    )
    .await;
    assert_eq!(nul.status(), StatusCode::BAD_REQUEST);
    // An ID committed before the bound existed is still a retry: its snapshot comes back, nothing is appended.
    sqlx::query("UPDATE match_events SET command_id = $1 WHERE match_id = $2 AND seq = 1")
        .bind("y".repeat(200))
        .bind(&match_id)
        .execute(&db)
        .await
        .unwrap();
    let retried = post_command(&app, &red.access_token, &match_id, &mv("y".repeat(200))).await;
    assert_eq!(retried.status(), StatusCode::OK);
    let retried: MatchSnapshotResponse = json_body(retried).await;
    assert_eq!(retried.seq, 1);
}

#[tokio::test]
async fn a_bare_sqlite_filename_setting_still_opens_and_reopens_its_database() {
    let seq = TEST_DB_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("jade-bare-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let setting = format!("{}?mode=rwc", dir.join("game.db").display());
    let app = build_router(build_app_state(&setting).await.unwrap());
    let (guest, cookie) = new_guest(&app, "Guest").await;
    drop(app);
    let reopened = build_router(build_app_state(&setting).await.unwrap());
    let recovered = session_as(&reopened, Some(&cookie)).await;
    assert_eq!(recovered.status(), StatusCode::OK);
    let recovered: CurrentSessionResponse = json_body(recovered).await;
    assert_eq!(
        recovered.account_id, guest.account_id,
        "the reopened server reads the same file"
    );
    // While it runs, the same file spelled another way is the same database, and a second server is refused.
    let respelled = format!("{}?mode=rwc", dir.join("game%2Edb").display());
    assert!(build_app_state(&respelled).await.is_err());
    // So is the same file behind a repeated scheme prefix, which meets the running owner's lock; a trailing space,
    // which the pool and the SQLite parser read differently, is refused as a setting.
    let contended = |result: Result<_, sqlx::Error>| matches!(result, Err(sqlx::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock);
    let reprefixed = format!("sqlite:sqlite:{}?mode=rwc", dir.join("game.db").display());
    assert!(contended(build_app_state(&reprefixed).await));
    let spaced = format!("sqlite:{} ", dir.join("game.db").display());
    assert!(matches!(
        build_app_state(&spaced).await,
        Err(sqlx::Error::Configuration(_))
    ));
    drop(reopened);
    // A `file:` URI, which SQLite would open as the same file under a lock the server could not follow, is refused as
    // a setting, with no server running, before any lock is taken.
    let uri = format!("sqlite:file:{}?mode=rwc", dir.join("game.db").display());
    assert!(matches!(
        build_app_state(&uri).await,
        Err(sqlx::Error::Configuration(_))
    ));
    let reprefixed_uri = format!(
        "sqlite:sqlite:file:{}?mode=rwc",
        dir.join("game.db").display()
    );
    assert!(matches!(
        build_app_state(&reprefixed_uri).await,
        Err(sqlx::Error::Configuration(_))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_hosted_postgres_connection_is_encrypted_and_certificate_checked() {
    // Runs only against a TLS-enabled PostgreSQL server whose certificate authority `JADE_TEST_POSTGRES_CA` names.
    let Some(ca) = std::env::var("JADE_TEST_POSTGRES_CA")
        .ok()
        .filter(|p| !p.is_empty())
    else {
        return;
    };
    let Some(pg) = fresh_postgres().await else {
        return;
    };
    let url = pg.url.clone();
    // Checked against the server's authority, the connection is made and is encrypted.
    let state = build_app_state(&with_query(
        &url,
        &[("sslmode", "verify-full"), ("sslrootcert", &ca)],
    ))
    .await
    .unwrap();
    let encrypted: bool =
        sqlx::query_scalar("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(encrypted);
    drop(state);
    // Checked against an unrelated authority, the server's certificate is refused during verification.
    let other_ca = std::env::var("JADE_TEST_POSTGRES_OTHER_CA")
        .expect("JADE_TEST_POSTGRES_OTHER_CA names an authority unrelated to the server's");
    let refused = build_app_state(&with_query(
        &url,
        &[("sslmode", "verify-full"), ("sslrootcert", &other_ca)],
    ))
    .await
    .err()
    .expect("a certificate from another authority is refused");
    assert!(
        refused.to_string().contains("invalid peer certificate"),
        "{refused}"
    );
}

#[tokio::test]
async fn every_in_memory_spelling_is_one_database_shared_by_the_pool() {
    for spelling in [
        "sqlite::memory:",
        "sqlite://:memory:",
        "sqlite:",
        "sqlite:sqlite::memory:",
    ] {
        let state = build_app_state(spelling).await.unwrap();
        // Two connections held at once: what one writes, the other reads.
        let mut writer = state.db.acquire().await.unwrap();
        let mut reader = state.db.acquire().await.unwrap();
        sqlx::query(
            "INSERT INTO accounts (account_id, display_name, created_at_ms) VALUES ('shared', 'Shared', 0)",
        )
        .execute(&mut *writer)
        .await
        .unwrap();
        let seen: i64 =
            sqlx::query_scalar("SELECT count(*) FROM accounts WHERE account_id = 'shared'")
                .fetch_one(&mut *reader)
                .await
                .unwrap();
        assert_eq!(seen, 1, "{spelling}");
    }
}

#[tokio::test]
async fn a_configured_origin_replaces_the_local_ones() {
    let test_db = test_db().await;
    let state = build_app_state(&test_db.url)
        .await
        .unwrap()
        .with_allowed_origins(AllowedOrigins::parse("https://play.example").unwrap());
    let app = build_router(state);
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
    let stream_from = |origin: &str| {
        let mut req = format!("ws://play.example/api/v1/matches/{match_id}/ws")
            .into_client_request()
            .unwrap();
        req.headers_mut().insert("Origin", origin.parse().unwrap());
        req.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", red.access_token).parse().unwrap(),
        );
        req
    };
    assert!(
        connect_ws_duplex(&app, stream_from("https://play.example"))
            .await
            .is_ok()
    );
    assert!(
        connect_ws_duplex(&app, stream_from("http://localhost:3180"))
            .await
            .is_err(),
        "the local default is no longer accepted"
    );
    // HTTP answers the configured origin's preflight, and no longer names the replaced local one.
    let preflight = |origin: &str| {
        Request::builder()
            .method("OPTIONS")
            .uri("/api/v1/invitations")
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .body(Body::empty())
            .unwrap()
    };
    let allowed = app
        .clone()
        .oneshot(preflight("https://play.example"))
        .await
        .unwrap();
    assert_eq!(
        allowed
            .headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap()),
        Some("https://play.example")
    );
    let replaced = app
        .clone()
        .oneshot(preflight("http://localhost:3180"))
        .await
        .unwrap();
    assert!(
        replaced
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}
