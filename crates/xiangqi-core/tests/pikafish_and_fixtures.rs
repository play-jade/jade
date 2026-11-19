use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use xiangqi_core::{
    ADOPTED_RULES_PROFILE, Board, BoardViewSnapshot, BoardViewport, GameOutcome, GameState, Move,
    NotationDialect, Orientation, PresentationPrefsSnapshot, ReasonCode, STARTING_FEN, Side,
    Square, ViolationLevel, format_chinese_move, format_wxf_move, parse_move_token, replay_tokens,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PikafishExampleFixture {
    id: String,
    fen: String,
    moves_uci: Vec<String>,
    moves_chinese: Vec<String>,
    expected_red_violation: ViolationLevel,
    expected_black_violation: ViolationLevel,
    expected_outcome: GameOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PikafishExamplesFile {
    schema_version: u8,
    rules_profile: String,
    source_url: String,
    source_revision_date: String,
    examples: Vec<PikafishExampleFixture>,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn raw_pikafish_39_cases() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    ViolationLevel,
    ViolationLevel,
    GameOutcome,
)> {
    use GameOutcome::*;
    use ViolationLevel::*;
    vec![
        (
            "ex01",
            "4k4/9/2c2an2/4c4/6R2/9/9/4B4/4A4/3K1AB2 w",
            "g5e5 f7e8 e5g5 e8f7 g5e5 f7e8 e5g5 e8f7",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex02",
            "3ak1b2/4a4/4b3r/4C4/5R3/9/9/9/9/4KA3 w",
            "f5g5 e9f9 g5f5 f9e9 f5g5 e9f9 g5f5 f9e9",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex03",
            "2ba1k1r1/4a4/4b4/9/9/9/7c1/1R7/9/4K2R1 w",
            "b2b3 h3h2 b3b2 h2h3 b2b3 h3h2 b3b2 h2h3",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex04",
            "4k3c/9/4bn2n/8c/6R2/6P2/9/9/9/3K5 w",
            "g5i5 i6f6 i5g5 f6i6 g5i5 i6f6 i5g5 f6i6",
            Allowed,
            PerpetualChase,
            RedWin,
        ),
        (
            "ex05",
            "5k3/9/9/p1CcC4/c8/9/9/9/9/4K4 w",
            "c6c5 d6d5 c5c6 d5d6 c6c5 d6d5 c5c6 d5d6",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex06",
            "5k3/9/9/9/9/3C5/9/4B4/3K5/2p6 w",
            "d1e1 c0d0 e1d1 d0c0 d1e1 c0d0 e1d1 d0c0",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex07",
            "3a1kb2/4a4/4r3b/7c1/9/9/9/8R/7CC/3K5 w",
            "i2h2 h6i6 h2i2 i6h6 i2h2 h6i6 h2i2 i6h6",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex08",
            "4kr3/4c4/7R1/4P4/9/9/4C4/9/9/4K4 w",
            "h7h8 e8e7 h8h7 e7e8 h7h8 e8e7 h8h7 e7e8",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex09",
            "2b1k4/9/4b4/4r3p/P5R1c/9/9/4C4/4K4/9 w",
            "g5g6 e6e5 g6g5 e5e6 g5g6 e6e5 g6g5 e5e6",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex10",
            "9/3rkr3/2c1ca3/9/9/9/9/9/4A4/3KC4 w",
            "e1d2 e7d7 d2e1 d7e7 e1d2 e7d7 d2e1 d7e7",
            PerpetualCheck,
            PerpetualCheck,
            Draw,
        ),
        (
            "ex11",
            "4k4/9/9/9/4C4/9/4r4/4C4/9/4K1B2 w",
            "e5f5 e3f3 f5e5 f3e3 e5f5 e3f3 f5e5 f3e3",
            PerpetualCheck,
            Allowed,
            BlackWin,
        ),
        (
            "ex12",
            "3k5/2R6/9/9/9/9/9/9/6r2/4K1N2 w",
            "c8c9 d9d8 c9c8 d8d9 c8c9 d9d8 c9c8 d8d9",
            PerpetualCheck,
            Allowed,
            BlackWin,
        ),
        (
            "ex13",
            "5k3/9/9/9/9/9/9/9/2p6/3KR1Bc1 w",
            "e0f0 f9e9 f0e0 e9f9 e0f0 f9e9 f0e0 e9f9",
            PerpetualCheck,
            Allowed,
            BlackWin,
        ),
        (
            "ex14",
            "3a1kb2/4a4/8b/9/4n4/2R6/9/4B4/9/4K4 w",
            "c4c5 e5d3 c5d5 d3b4 d5d4 b4c6 d4c4 c6e5 c4c5 e5d3 c5d5 d3b4 d5d4 b4c6 d4c4 c6e5",
            PerpetualChase,
            PerpetualChase,
            Draw,
        ),
        (
            "ex15",
            "3a1kb2/4a4/8b/9/4n4/2R6/9/4B4/9/4K4 w",
            "c4c5 e5d3 c5d5 d3b4 d5d4 b4c6 d4c4 c6e5 c4c5 e5d3 c5c3 d3e5 c3c5",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex16",
            "6R2/4k4/9/4r4/9/9/5p3/5A3/5K3/9 w",
            "g9g8 e8e9 g8f8 f3g3 f8g8 g3f3 g8g9 e9e8 g9g8 e8e9",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex17",
            "9/2N1k4/9/2r6/2b6/4C4/9/4B4/9/5K3 w",
            "e4c4 c5e7 c8a7 c6a6 c4e4 e7c5 a7c8 a6c6 e4c4 c5e7 c8a7 c6a6 c4e4 e7c5 a7c8 a6c6",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex18",
            "4k4/9/4n4/1c2N1c2/4N4/6P2/9/9/9/4K4 w",
            "e6g5 e7c6 g5e6 c6e7 e6g5 e7c6 g5e6 c6e7",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex19",
            "4k4/9/4n4/1c2N1r2/4N4/6P2/9/9/9/4K4 w",
            "e6g5 e7c6 g5e6 c6e7 e6g5 e7c6 g5e6 c6e7",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex20",
            "2R6/4k4/4b4/9/2b6/4r4/9/3K1A3/5C3/9 w",
            "f1e1 e8f8 e1f1 f8e8 f1e1 e8f8 e1f1 f8e8",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex21",
            "3k2b2/4a4/3ab4/9/9/9/9/3N5/7R1/c1c1K4 w",
            "e0e1 c0c1 e1e0 c1c0 e0e1 c0c1 e1e0 c1c0",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex22",
            "3k5/9/9/9/5r3/9/4n4/9/4AC3/3A1K3 w",
            "f1f2 e3d1 f2f1 d1e3 f1f2 e3d1 f2f1 d1e3",
            Allowed,
            PerpetualChase,
            RedWin,
        ),
        (
            "ex23",
            "5k3/9/9/2N6/9/1N7/9/4BC3/9/cr1RK4 w",
            "b4c2 b0c0 c2b4 c0b0 b4c2 b0c0 c2b4 c0b0",
            Allowed,
            PerpetualChase,
            RedWin,
        ),
        (
            "ex24",
            "5k3/9/9/9/9/4c4/3r5/3NB4/4A4/4K4 w",
            "e0d0 e4d4 d0e0 d4e4 e0d0 e4d4 d0e0 d4e4",
            Allowed,
            PerpetualChase,
            RedWin,
        ),
        (
            "ex25",
            "3k5/9/9/9/9/9/9/9/1cr2Rn2/3AK4 w",
            "d0e1 c1c0 e1d0 c0c1 d0e1 c1c0 e1d0 c0c1",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex26",
            "3k5/9/9/9/9/9/9/9/1cr2Rn2/3AK4 w",
            "d0e1 c1c2 e1d0 c2c1 d0e1 c1c2 e1d0 c2c1",
            PerpetualChase,
            PerpetualChase,
            Draw,
        ),
        (
            "ex27",
            "3k5/9/9/9/9/9/9/9/1cr3Rn1/3AK4 w",
            "d0e1 c1c2 e1d0 c2c1 d0e1 c1c2 e1d0 c2c1",
            PerpetualChase,
            PerpetualChase,
            Draw,
        ),
        (
            "ex28",
            "3k5/9/3a5/2C6/2r6/2C6/2r6/5A3/9/5K3 w",
            "c4d4 c5d5 d4c4 d5c5 c4d4 c5d5 d4c4 d5c5",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex29",
            "3k5/9/3a5/2C6/2r6/2C6/2r6/5A3/9/5K3 w",
            "c4e4 c5e5 e4c4 e5c5 c4e4 c5e5 e4c4 e5c5",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex30",
            "3k5/9/3a5/2C6/2r6/2C6/2r6/2N2A3/9/5K3 w",
            "c4e4 c5e5 e4c4 e5c5 c4e4 c5e5 e4c4 e5c5",
            PerpetualChase,
            PerpetualChase,
            Draw,
        ),
        (
            "ex31",
            "3r5/4ck3/3R5/9/9/9/3cR4/9/4A4/4KA3 w",
            "e0d0 e8d8 d0e0 d8e8 e0d0 e8d8 d0e0 d8e8",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex32",
            "rCRak4/4a4/9/9/9/4p4/9/9/9/4K4 w",
            "e0d0 e4d4 d0e0 d4e4 e0d0 e4d4 d0e0 d4e4",
            PerpetualChase,
            Allowed,
            BlackWin,
        ),
        (
            "ex33",
            "5k3/9/9/9/9/9/9/4p4/4A4/3K1ABc1 w",
            "d0d1 h0h1 d1d0 h1h0 d0d1 h0h1 d1d0 h1h0",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex34",
            "4ka3/4a4/5R3/2r6/2r6/9/3R5/9/9/2p2K3 w",
            "d3e3 e9d9 e3d3 d9e9 d3e3 e9d9 e3d3 d9e9",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex35",
            "1CRck4/4a4/9/9/9/9/9/9/3CA4/5K3 w",
            "e1d2 e8d7 d2e1 d7e8 e1d2 e8d7 d2e1 d7e8",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex36",
            "4k4/3P5/9/9/3c5/4C4/3c5/9/9/3K5 w",
            "e4d4 d5e5 d4e4 e5d5 e4d4 d5e5 d4e4 e5d5",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex37",
            "3a1a3/2R6/3k5/3r5/3N5/9/9/9/4A4/4KA3 w",
            "d5f4 d6f6 f4d5 f6d6 c8c7 d7d8 c7c8 d8d7",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex38",
            "4k4/9/9/9/9/9/7n1/4B4/3rAR3/4KA3 w",
            "f1f3 h3g1 f3f1 g1h3 f1h1 h3f2 h1f1 f2h3",
            Allowed,
            Allowed,
            Draw,
        ),
        (
            "ex39",
            "3ak4/c2R1R2c/4b4/9/9/9/9/9/9/4K4 w",
            "f8f6 d9e8 f6f8 e8d9 f8f6 d9e8 f6f8 e8d9",
            PerpetualChase,
            PerpetualChase,
            Draw,
        ),
    ]
}

#[test]
fn all_39_published_pikafish_examples_adjudicate_and_roundtrip_notation() {
    let cases = raw_pikafish_39_cases();
    assert_eq!(cases.len(), 39);

    let mut fixtures = Vec::with_capacity(39);

    for (id, fen, moves_str, exp_red, exp_blk, exp_outcome) in cases {
        let mut game = GameState::from_fen(fen, ADOPTED_RULES_PROFILE)
            .unwrap_or_else(|e| panic!("{id} FEN failed: {e}"));
        let uci_tokens: Vec<String> = moves_str.split_whitespace().map(str::to_string).collect();
        let mut chinese_tokens = Vec::with_capacity(uci_tokens.len());

        let mut final_verdict = None;
        for (ply, uci) in uci_tokens.iter().enumerate() {
            assert!(
                final_verdict.is_none(),
                "{id} terminated prematurely before ply {ply}"
            );
            let mv = Move::from_uci_or_iccs(uci).unwrap();
            let zh = format_chinese_move(game.board(), mv, false)
                .unwrap_or_else(|e| panic!("{id} Chinese format failed at ply {ply}: {e}"));
            let wxf = format_wxf_move(game.board(), mv)
                .unwrap_or_else(|e| panic!("{id} WXF format failed at ply {ply}: {e}"));

            // Round-trip both Chinese and WXF parsers on this exact position!
            let parsed_zh = parse_move_token(game.board(), &zh, ply)
                .unwrap_or_else(|e| panic!("{id} failed parsing Chinese '{zh}' at ply {ply}: {e}"));
            assert_eq!(parsed_zh, mv, "{id} Chinese round-trip mismatch on '{zh}'");

            let parsed_wxf = parse_move_token(game.board(), &wxf, ply)
                .unwrap_or_else(|e| panic!("{id} failed parsing WXF '{wxf}' at ply {ply}: {e}"));
            assert_eq!(parsed_wxf, mv, "{id} WXF round-trip mismatch on '{wxf}'");

            chinese_tokens.push(zh);
            final_verdict = game
                .apply_move(mv)
                .unwrap_or_else(|e| panic!("{id} illegal move {uci} at ply {ply}: {e}"));
        }

        let verdict = final_verdict.unwrap_or_else(|| panic!("{id} expected terminal verdict"));
        assert_eq!(
            verdict.details.red_violation,
            Some(exp_red),
            "{id} Red violation mismatch (details={:?})",
            verdict.details
        );
        assert_eq!(
            verdict.details.black_violation,
            Some(exp_blk),
            "{id} Black violation mismatch (details={:?})",
            verdict.details
        );
        assert_eq!(
            verdict.outcome, exp_outcome,
            "{id} outcome mismatch (verdict={verdict:?})"
        );

        // Verify replay_tokens with Chinese notation reproduces the exact terminal adjudication!
        let replayed = replay_tokens(fen, NotationDialect::ChineseSimplified, &chinese_tokens)
            .expect("replay_tokens");
        assert!(replayed.complete);
        assert_eq!(
            replayed.terminal_adjudication.unwrap().outcome,
            exp_outcome,
            "{id} Chinese replay outcome mismatch"
        );

        fixtures.push(PikafishExampleFixture {
            id: id.to_string(),
            fen: fen.to_string(),
            moves_uci: uci_tokens,
            moves_chinese: chinese_tokens,
            expected_red_violation: exp_red,
            expected_black_violation: exp_blk,
            expected_outcome: exp_outcome,
        });
    }

    let out_file = PikafishExamplesFile {
        schema_version: 1,
        rules_profile: ADOPTED_RULES_PROFILE.to_string(),
        source_url: "https://www.pikafish.com/rule.html".to_string(),
        source_revision_date: "2023-11-22".to_string(),
        examples: fixtures,
    };
    let path = repo_root().join("data/fixtures/rules/pikafish_examples.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(&out_file).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn movement_blocking_stalemate_loss_and_120_limit_rules_work() {
    // 1. Opening legal move count == 44
    let start = GameState::default();
    assert_eq!(start.board().legal_moves().len(), 44);

    // 2. Horse leg blocking (`蹩马腿`) and Elephant eye blocking (`塞象眼`)
    // In starting position, Red Horse at b0 has pawn at c3? No, pawns are at a3,c3,e3,g3,i3;
    // b0 horse can jump to a2 and c2 (2 moves), NOT d1 because c0 has Elephant blocking leg!
    let b0 = Square::from_uci("b0").unwrap();
    let horse_moves: Vec<String> = start
        .board()
        .legal_moves()
        .into_iter()
        .filter(|m| m.from == b0)
        .map(|m| m.to_uci())
        .collect();
    assert_eq!(horse_moves, vec!["b0a2", "b0c2"]);

    // 3. Flying generals (`将帅对脸`) prohibited
    let facing = Board::from_fen("4k4/9/9/9/9/9/9/9/9/4K4 w - - 0 1").unwrap();
    // Red General at e0 cannot stay on file e (`e0e1`), must step to `d0` or `f0`? Wait: if they already
    // start facing, stepping off file e resolves facing! If separated by one Advisor `e1`, moving `e1` off file e is illegal!
    let pinned_advisor = Board::from_fen("4k4/9/9/9/9/9/9/9/4A4/4K4 w - - 0 1").unwrap();
    let e1 = Square::from_uci("e1").unwrap();
    assert!(
        pinned_advisor
            .legal_moves()
            .into_iter()
            .all(|m| m.from != e1),
        "Advisor between facing generals must be pinned"
    );
    let _ = facing;

    // 4. `困毙` (No legal moves without check) is a LOSS for side to move, NEVER a draw!
    // Black General at d9, Red Soldier at d8? No, d8 gives check.
    // Put Black General at c9? c9 is outside palace (palace is d,e,f = files 3,4,5).
    // Black General at d9 (file 3, rank 9). Squares around d9 in palace: e9 (4,9) and d8 (3,8).
    // Cover e9 with Red Chariot at e0 (attacks file e, does NOT check d9!), and cover d8 with Red Chariot at a8 (attacks rank 8, does NOT check d9!)!
    // Red makes a quiet move `a0a1` -> Black at d9 is NOT in check, has 0 legal moves (`d9e9` into e0 chariot, `d9d8` into a8 chariot) -> `NoLegalMovesLoss` (`RedWin`)!
    let mut kunbi = GameState::from_fen(
        "3k5/R8/9/9/9/9/9/9/9/P3RK3 w - - 0 1",
        ADOPTED_RULES_PROFILE,
    )
    .unwrap();
    let verdict = kunbi
        .apply_move(Move::from_uci_or_iccs("a0a1").unwrap())
        .unwrap()
        .expect("kunbi terminal");
    assert_eq!(verdict.outcome, GameOutcome::RedWin);
    assert_eq!(verdict.reason, ReasonCode::NoLegalMovesLoss);

    // 5. Insufficient attacking material (only Generals, Advisors, Elephants) -> Draw
    let mut insuff = GameState::from_fen(
        "4k4/4a4/4b4/9/9/9/9/4B4/4A4/4K4 w - - 0 1",
        ADOPTED_RULES_PROFILE,
    )
    .unwrap();
    let v_insuff = insuff
        .apply_move(Move::from_uci_or_iccs("e2c4").unwrap())
        .unwrap()
        .expect("insufficient material draw");
    assert_eq!(v_insuff.outcome, GameOutcome::Draw);
    assert_eq!(v_insuff.reason, ReasonCode::InsufficientMaterialDraw);

    // 6. Five soldiers on one file Chinese disambiguation (`前兵`, `二兵`, `三兵`, `四兵`, `后兵`)
    let five_pawns = Board::from_fen("4k4/2P6/2P6/2P6/2P6/2P6/9/9/4A4/4K4 w - - 0 1").unwrap();
    assert_eq!(
        format_chinese_move(&five_pawns, Move::from_uci_or_iccs("c6d6").unwrap(), false).unwrap(),
        "三兵平六"
    );
    assert_eq!(
        format_chinese_move(&five_pawns, Move::from_uci_or_iccs("c4d4").unwrap(), false).unwrap(),
        "后兵平六"
    );
    assert_eq!(
        parse_move_token(&five_pawns, "三兵平六", 0).unwrap(),
        Move::from_uci_or_iccs("c6d6").unwrap()
    );

    // 7. 120-step natural limit with >10 checks per side exemption (Chapter 1 Section 5 Clause 2)
    // Construct a position where Red gives 12 non-repeating checks (e.g. Chariot checking along open files/ranks
    // while another piece shifts so no position repeats 3 times): verify checks 1..=10 and their replies count,
    // while checks 11..=12 and their forced evasions do NOT increment effective_non_capture_plies!
    let mut check_cap_game = GameState::from_fen(
        "3k5/9/9/9/9/9/9/R7P/4A4/4K4 w - - 0 1",
        ADOPTED_RULES_PROFILE,
    )
    .unwrap();
    // Play 12 pairs of (check, king step) mixed with pawn/advisor nudges so positions don't 3x repeat:
    // Step 1: a2d2 (check #1, eff=1), d9e9 (reply, eff=2)
    check_cap_game
        .apply_move(Move::from_uci_or_iccs("a2d2").unwrap())
        .unwrap();
    assert_eq!(check_cap_game.effective_non_capture_plies(), 1);
    check_cap_game
        .apply_move(Move::from_uci_or_iccs("d9e9").unwrap())
        .unwrap();
    assert_eq!(check_cap_game.effective_non_capture_plies(), 2);
}

#[test]
fn section_4_viewport_geometry_and_hit_testing_golden_fixtures() {
    let vp = BoardViewport::compute(486.0, 540.0, 18.0).expect("valid viewport");
    // (486 - 36)/9 = 50.0, (540 - 36)/10 = 50.4 -> s = 50.0
    assert!((vp.cell_size - 50.0).abs() < 1e-9);
    assert!((vp.origin_x - 43.0).abs() < 1e-9);
    assert!((vp.origin_y - 45.0).abs() < 1e-9);

    // Invalid / collapsed viewports return None
    assert!(BoardViewport::compute(20.0, 500.0, 15.0).is_none());
    assert!(BoardViewport::compute(400.0, 500.0, -1.0).is_none());

    let mut center_records = Vec::with_capacity(180);
    for orient in [Orientation::RedBelow, Orientation::BlackBelow] {
        for rank in 0..10u8 {
            for file in 0..9u8 {
                let sq = Square::from_file_rank(file, rank).unwrap();
                let (sx, sy) = vp.square_to_screen(sq, orient);
                let hit = vp.screen_to_square(sx, sy, orient);
                assert_eq!(
                    hit,
                    Some(sq),
                    "center roundtrip failed at {sq} ({orient:?})"
                );
                center_records.push(serde_json::json!({
                    "orientation": orient,
                    "square": sq.to_uci(),
                    "file": file,
                    "rank": rank,
                    "screen_x": sx,
                    "screen_y": sy,
                }));
            }
        }
    }

    // Half-open boundary checks (`-0.5 <= u < 8.5`, `-0.5 <= v < 9.5`)
    // Left outer edge u = -0.5 -> hits file 0 (RedBelow); u = -0.5001 -> None (no clamping!)
    let left_edge_x = vp.origin_x - 0.5 * vp.cell_size;
    let bottom_y = vp.origin_y + 9.0 * vp.cell_size;
    assert_eq!(
        vp.screen_to_square(left_edge_x, bottom_y, Orientation::RedBelow),
        Some(Square::from_file_rank(0, 0).unwrap())
    );
    assert_eq!(
        vp.screen_to_square(left_edge_x - 0.01, bottom_y, Orientation::RedBelow),
        None
    );

    // Right outer edge u = 8.5 -> excluded (`u < 8.5`) -> None!
    let right_edge_x = vp.origin_x + 8.5 * vp.cell_size;
    assert_eq!(
        vp.screen_to_square(right_edge_x, bottom_y, Orientation::RedBelow),
        None
    );
    assert_eq!(
        vp.screen_to_square(right_edge_x - 0.001, bottom_y, Orientation::RedBelow),
        Some(Square::from_file_rank(8, 0).unwrap())
    );

    // Internal boundary u = 0.5 belongs to cell on its right (c = floor(0.5 + 0.5) = 1)
    let mid_01_x = vp.origin_x + 0.5 * vp.cell_size;
    assert_eq!(
        vp.screen_to_square(mid_01_x, bottom_y, Orientation::RedBelow),
        Some(Square::from_file_rank(1, 0).unwrap())
    );

    // Internal vertical boundary v = 0.5 belongs to cell below (r = 1 -> rank 8 when RedBelow)
    let mid_01_y = vp.origin_y + 0.5 * vp.cell_size;
    assert_eq!(
        vp.screen_to_square(vp.origin_x, mid_01_y, Orientation::RedBelow),
        Some(Square::from_file_rank(0, 8).unwrap())
    );

    let game = GameState::default();
    let snapshot = BoardViewSnapshot::from_game(
        &game,
        1,
        1,
        Orientation::RedBelow,
        Some((486.0, 540.0, 18.0)),
        Some(Square::from_uci("h2").unwrap()),
        Vec::new(),
        PresentationPrefsSnapshot::default(),
    );
    assert_eq!(snapshot.pieces.len(), 32);
    assert_eq!(snapshot.side_to_move, Side::Red);
    assert!(!snapshot.legal_destinations.is_empty());

    let fixture_doc = serde_json::json!({
        "schema_version": 1,
        "viewport": vp,
        "centers": center_records,
        "sample_snapshot": snapshot,
    });
    let geom_path = repo_root().join("data/fixtures/board-view/geometry_fixtures.json");
    fs::write(
        &geom_path,
        serde_json::to_string_pretty(&fixture_doc).unwrap() + "\n",
    )
    .unwrap();

    let rules_fixture_doc = serde_json::json!({
        "schema_version": 1,
        "rules_profile": ADOPTED_RULES_PROFILE,
        "starting_fen": STARTING_FEN,
        "opening_legal_move_count": 44,
        "kunbi_loss_case": {
            "fen": "3k5/R8/9/9/9/9/9/9/9/P3RK3 w - - 0 1",
            "move_uci": "a0a1",
            "expected_outcome": "red_win",
            "expected_reason": "no_legal_moves_loss"
        },
        "insufficient_material_case": {
            "fen": "4k4/4a4/4b4/9/9/9/9/4B4/4A4/4K4 w - - 0 1",
            "move_uci": "e2c4",
            "expected_outcome": "draw",
            "expected_reason": "insufficient_material_draw"
        }
    });
    let rules_path = repo_root().join("data/fixtures/rules/movement_and_adjudication.json");
    fs::write(
        &rules_path,
        serde_json::to_string_pretty(&rules_fixture_doc).unwrap() + "\n",
    )
    .unwrap();
}

proptest! {
    #[test]
    fn prop_action_index_bijection(idx in 0u16..8100u16) {
        let mv = Move::from_action_index(idx).unwrap();
        prop_assert_eq!(mv.action_index(), idx);
        prop_assert_eq!(mv.mirror_file().mirror_file(), mv);
    }

    #[test]
    fn prop_random_walk_apply_undo_and_mirror_symmetry(seed in prop::collection::vec(0usize..128usize, 1..20)) {
        let mut game = GameState::default();
        let initial_fen = game.to_fen();
        let mut applied = 0usize;

        for choice in seed {
            let legal = game.board().legal_moves();
            if legal.is_empty() {
                break;
            }
            let mv = legal[choice % legal.len()];
            // Verify left-right mirror symmetry produces mirrored legal move!
            let mirrored_board = game.board().mirror_files();
            let mirrored_legal = mirrored_board.legal_moves();
            prop_assert!(mirrored_legal.contains(&mv.mirror_file()));

            game.apply_move(mv).unwrap();
            applied += 1;
            if game.adjudicate().is_some() {
                break;
            }
        }

        for _ in 0..applied {
            game.undo_move().unwrap();
        }
        prop_assert_eq!(game.to_fen(), initial_fen);
    }
}
