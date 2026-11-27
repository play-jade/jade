use archive_client::Visibility;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use jade_match_server::{
    CoordMovePayload, CurrentSessionResponse, GuestSessionResponse, InvitationCreatedResponse,
    JoinInvitationResponse, MatchCommandRequest, MatchSnapshotResponse, PrivacyUpdateRequest,
    build_app_state, build_router,
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let tmp_db = format!(
        "/tmp/jade_owner_lock_test_{}.db",
        std::process::id()
            ^ (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos())
    );
    let sqlite_url = format!("sqlite:{tmp_db}?mode=rwc");

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

    let _ = std::fs::remove_file(&tmp_db);
    let _ = std::fs::remove_file(format!("{tmp_db}.owner.lock"));
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    sqlx::query("UPDATE auth_sessions SET revoked_at_ms = 1 WHERE account_id = ?")
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
    sqlx::query("UPDATE auth_sessions SET expires_at_ms = ? WHERE account_id = ?")
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
    sqlx::query("UPDATE auth_sessions SET expires_at_ms = 0 WHERE account_id = ?")
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
    // A file-backed database, so the two saves run on separate connections and compete for the write lock.
    let dir = std::env::temp_dir().join(format!("jade-concurrent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.sqlite3");
    let _ = std::fs::remove_file(&path);
    let state = build_app_state(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
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
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_state_version_advances_with_every_change_by_either_participant() {
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
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
    let state = build_app_state("sqlite::memory:").await.unwrap();
    let db = state.db.clone();
    let app = build_router(state);
    let (red, _) = new_guest(&app, "Red").await;
    let (blk, _) = new_guest(&app, "Black").await;
    let match_id = start_match(&app, &red.access_token, &blk.access_token).await;
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
         FROM match_events WHERE match_id = ? AND seq = 1",
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
    let dir = std::env::temp_dir().join(format!("jade-turns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("game.sqlite3");
    let _ = std::fs::remove_file(&path);
    let url = format!("sqlite://{}?mode=rwc", path.display());
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
    let _ = std::fs::remove_dir_all(&dir);
}
