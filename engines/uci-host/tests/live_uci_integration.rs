use jade_uci_host::{SearchBudget, SearchRequest, SupervisedUciProcess};
use std::path::PathBuf;
use std::time::Duration;
use xiangqi_core::{ADOPTED_RULES_PROFILE, STARTING_FEN};

fn uci_binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("mobile-bridge/build/jade-uci-engine")
}

#[tokio::test]
async fn supervised_desktop_uci_process_starts_cancels_and_returns_verified_legal_move() {
    let bin = uci_binary_path();
    assert!(
        bin.exists(),
        "Expected compiled C++26 UCI binary at {} (run ./dev/build-engines.sh)",
        bin.display()
    );

    let mut host = SupervisedUciProcess::spawn(bin.to_str().unwrap(), Duration::from_secs(3))
        .await
        .expect("spawn jade-uci-engine");
    assert!(host.reported_name().is_some());

    // 1. Depth-bounded search returning a canonical `xiangqi-core`-verified legal continuation
    let req_bounded = SearchRequest {
        search_id: "slice-a-desktop-1".to_string(),
        state_id: "startpos".to_string(),
        engine_id: "jade-uci-engine-cpp26".to_string(),
        binary_sha256: "local-dev-build".to_string(),
        network_sha256: None,
        rules_profile: ADOPTED_RULES_PROFILE.to_string(),
        initial_fen: STARTING_FEN.to_string(),
        moves_uci: vec!["h2e2".to_string()],
        multipv: 2,
        threads: 1,
        hash_mb: 16,
        budget: SearchBudget {
            depth: Some(4),
            movetime_ms: None,
            nodes: None,
        },
    };
    let manifest = host
        .run_search(&req_bounded, None, Duration::from_secs(5))
        .await
        .expect("bounded UCI search");
    assert!(!manifest.cancelled);
    assert!(manifest.bestmove_uci.is_some());
    assert!(!manifest.infos.is_empty());
    assert!(!manifest.infos.last().unwrap().validated_pv_uci.is_empty());

    // 2. Infinite search stopped cleanly via `stop` within 40 ms
    let req_infinite = SearchRequest {
        search_id: "slice-a-desktop-cancel".to_string(),
        budget: SearchBudget {
            depth: None,
            movetime_ms: None,
            nodes: None,
        },
        ..req_bounded
    };
    let cancelled_manifest = host
        .run_search(
            &req_infinite,
            Some(Duration::from_millis(35)),
            Duration::from_secs(3),
        )
        .await
        .expect("cancelled UCI search");
    assert!(cancelled_manifest.cancelled);
    assert!(cancelled_manifest.bestmove_uci.is_some());

    host.shutdown().await;
}
