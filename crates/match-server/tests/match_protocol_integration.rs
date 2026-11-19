use archive_client::Visibility;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use hyper_util::{rt::TokioIo, service::TowerToHyperService};
use jade_match_server::{
    CoordMovePayload, GuestSessionResponse, InvitationCreatedResponse, JoinInvitationResponse,
    MatchCommandRequest, MatchSnapshotResponse, PrivacyUpdateRequest, build_app_state,
    build_router,
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
        let share_req = Request::builder()
            .method("POST")
            .uri(format!("/api/v1/matches/{}/privacy", created.match_id))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&PrivacyUpdateRequest {
                    visibility: Visibility::Unlisted,
                    allow_internal_training: false,
                    allow_published_research: false,
                    allow_dataset_release: false,
                })
                .unwrap(),
            ))
            .unwrap();
        assert_eq!(
            app.clone().oneshot(share_req).await.unwrap().status(),
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
    let red_priv = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/privacy", inv.match_id))
        .header("authorization", format!("Bearer {}", red.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&PrivacyUpdateRequest {
                visibility: Visibility::Public,
                allow_internal_training: true,
                allow_published_research: true,
                allow_dataset_release: true,
            })
            .unwrap(),
        ))
        .unwrap();
    let red_priv_json: serde_json::Value =
        json_body(app.clone().oneshot(red_priv).await.unwrap()).await;
    assert_eq!(red_priv_json["effective_visibility"], "private");
    assert_eq!(red_priv_json["eligible_for_internal_training"], false);
    assert_eq!(red_priv_json["eligible_for_dataset_release"], false);

    // Now Black ALSO opts in -> match becomes `public` and eligible!
    let blk_priv = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/matches/{}/privacy", inv.match_id))
        .header("authorization", format!("Bearer {}", blk.access_token))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&PrivacyUpdateRequest {
                visibility: Visibility::Public,
                allow_internal_training: true,
                allow_published_research: true,
                allow_dataset_release: true,
            })
            .unwrap(),
        ))
        .unwrap();
    let blk_priv_json: serde_json::Value =
        json_body(app.clone().oneshot(blk_priv).await.unwrap()).await;
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
                    visibility: Visibility::Public,
                    allow_internal_training: true,
                    allow_published_research: false,
                    allow_dataset_release: false,
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
                visibility: Visibility::Private,
                allow_internal_training: false,
                allow_published_research: false,
                allow_dataset_release: false,
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
        let _ = app1
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v1/matches/{}/privacy", inv.match_id))
                    .header("authorization", format!("Bearer {tok}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_string(&PrivacyUpdateRequest {
                            visibility: Visibility::Public,
                            allow_internal_training: false,
                            allow_published_research: false,
                            allow_dataset_release: false,
                        })
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
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
