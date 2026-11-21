use archive_client::SampleArchiveProvider;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use xiangqi_core::{
    ADOPTED_RULES_PROFILE, BoardViewSnapshot, BoardViewport, GameState, Move, NotationDialect,
    Orientation, PresentationPrefsSnapshot, STARTING_FEN, Square, format_chinese_move,
    format_wxf_move, parse_move_token, replay_tokens,
};

#[derive(Serialize)]
struct JsGameStateSummary {
    fen: String,
    position_key_sha256: String,
    side_to_move: String,
    ply_count: usize,
    in_check: bool,
    legal_moves_uci: Vec<String>,
    adjudication: Option<xiangqi_core::Adjudication>,
}

#[wasm_bindgen]
pub struct WasmGameSession {
    inner: GameState,
}

impl Default for WasmGameSession {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl WasmGameSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: GameState::default(),
        }
    }

    pub fn from_fen(fen: &str) -> Result<WasmGameSession, JsError> {
        let inner = GameState::from_fen(fen, ADOPTED_RULES_PROFILE)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Self { inner })
    }

    pub fn apply_move_token(&mut self, token: &str) -> Result<JsValue, JsError> {
        let ply = self.inner.ply_count();
        let mv = parse_move_token(self.inner.board(), token, ply)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let zh = format_chinese_move(self.inner.board(), mv, false).unwrap_or_else(|_| mv.to_uci());
        let verdict = self
            .inner
            .apply_move(mv)
            .map_err(|e| JsError::new(&e.to_string()))?;
        serde_wasm_bindgen::to_value(&serde_json::json!({
            "move_uci": mv.to_uci(),
            "move_chinese": zh,
            "adjudication": verdict,
        }))
        .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn undo(&mut self) -> Result<String, JsError> {
        self.inner
            .undo_move()
            .map(|m| m.to_uci())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn state_summary(&self) -> Result<JsValue, JsError> {
        let board = self.inner.board();
        let stm = board.side_to_move();
        let summary = JsGameStateSummary {
            fen: self.inner.to_fen(),
            position_key_sha256: board.position_key().sha256_hex,
            side_to_move: stm.as_str().to_string(),
            ply_count: self.inner.ply_count(),
            in_check: board.is_in_check(stm),
            legal_moves_uci: board
                .legal_moves()
                .into_iter()
                .map(|m| m.to_uci())
                .collect(),
            adjudication: self.inner.adjudicate(),
        };
        serde_wasm_bindgen::to_value(&summary).map_err(|e| JsError::new(&e.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn board_view(
        &self,
        width: f64,
        height: f64,
        padding: f64,
        red_below: bool,
        selected_uci: Option<String>,
        traditional_glyphs: bool,
        material_id: &str,
        dark_surface: bool,
        reduced_motion: bool,
    ) -> Result<JsValue, JsError> {
        let sel = match selected_uci {
            Some(ref s) if !s.is_empty() => {
                Some(Square::from_uci(s).map_err(|e| JsError::new(&e.to_string()))?)
            }
            _ => None,
        };
        let orient = if red_below {
            Orientation::RedBelow
        } else {
            Orientation::BlackBelow
        };
        let prefs = PresentationPrefsSnapshot {
            material_id: material_id.to_string(),
            glyph_set_id: if traditional_glyphs {
                "traditional-prototype-v1".to_string()
            } else {
                "simplified-prototype-v1".to_string()
            },
            dark_surface,
            reduced_motion,
        };
        let snap = BoardViewSnapshot::from_game(
            &self.inner,
            self.inner.ply_count() as u64,
            1,
            orient,
            Some((width, height, padding)),
            sel,
            Vec::new(),
            prefs,
        );
        serde_wasm_bindgen::to_value(&snap).map_err(|e| JsError::new(&e.to_string()))
    }
}

#[wasm_bindgen]
pub fn wasm_hit_test(
    width: f64,
    height: f64,
    padding: f64,
    red_below: bool,
    px: f64,
    py: f64,
) -> Option<String> {
    let vp = BoardViewport::compute(width, height, padding)?;
    let orient = if red_below {
        Orientation::RedBelow
    } else {
        Orientation::BlackBelow
    };
    vp.screen_to_square(px, py, orient).map(|sq| sq.to_uci())
}

#[wasm_bindgen]
pub fn wasm_sample_search_players(query: &str) -> Result<JsValue, JsError> {
    let provider = SampleArchiveProvider::new_embedded();
    serde_wasm_bindgen::to_value(&provider.search_players(query))
        .map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn wasm_sample_query_position(fen: &str) -> Result<JsValue, JsError> {
    let provider = SampleArchiveProvider::new_embedded();
    let res = provider.query_position(fen).map_err(|e| JsError::new(&e))?;
    serde_wasm_bindgen::to_value(&res).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn wasm_replay_uci_list(
    initial_fen: &str,
    moves_space_separated: &str,
) -> Result<JsValue, JsError> {
    let tokens: Vec<String> = moves_space_separated
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let res = replay_tokens(initial_fen, NotationDialect::Uci, &tokens)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let _ = Move::from_action_index(0);
    serde_wasm_bindgen::to_value(&res).map_err(|e| JsError::new(&e.to_string()))
}

// =========================================================================
// Direct WebAssembly linear-memory JSON ABI (zero external bindgen needed)
// =========================================================================

thread_local! {
    static LAST_RESPONSE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn write_response<T: Serialize>(ok: bool, payload: Result<T, String>) -> i32 {
    let body = match payload {
        Ok(data) => serde_json::json!({ "ok": ok, "data": data }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    };
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    let status = if body["ok"].as_bool() == Some(true) {
        1
    } else {
        0
    };
    LAST_RESPONSE.with(|cell| {
        *cell.borrow_mut() = bytes;
    });
    status
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum DirectWasmRequest {
    BoardView {
        fen: Option<String>,
        #[serde(default)]
        moves_uci: Vec<String>,
        width: f64,
        height: f64,
        padding: f64,
        #[serde(default = "default_true")]
        red_below: bool,
        selected_uci: Option<String>,
        #[serde(default)]
        traditional_glyphs: bool,
        #[serde(default = "default_material")]
        material_id: String,
        #[serde(default)]
        dark_surface: bool,
        #[serde(default)]
        reduced_motion: bool,
    },
    ApplyMove {
        fen: Option<String>,
        #[serde(default)]
        moves_uci: Vec<String>,
        token: String,
        #[serde(default)]
        traditional_glyphs: bool,
    },
    HitTest {
        width: f64,
        height: f64,
        padding: f64,
        #[serde(default = "default_true")]
        red_below: bool,
        px: f64,
        py: f64,
    },
    ArchiveSearch {
        #[serde(default)]
        query: String,
    },
    ArchiveGame {
        game_id: String,
    },
    ArchivePosition {
        fen: Option<String>,
    },
    /// Notates a recorded move list in one dialect, stopping at the first ply that does not replay.
    NotateMoves {
        fen: Option<String>,
        #[serde(default)]
        moves_uci: Vec<String>,
        #[serde(default)]
        wxf: bool,
        #[serde(default)]
        traditional_glyphs: bool,
    },
}

fn default_true() -> bool {
    true
}

fn default_material() -> String {
    "pale-jade".to_string()
}

fn replay_game(
    initial_fen: Option<&str>,
    moves_uci: &[String],
    traditional: bool,
) -> Result<(GameState, Vec<String>), String> {
    let start = initial_fen
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(STARTING_FEN);
    let mut game = GameState::from_fen(start, ADOPTED_RULES_PROFILE).map_err(|e| e.to_string())?;
    let mut zh_moves = Vec::with_capacity(moves_uci.len());
    for (ply, tok) in moves_uci.iter().enumerate() {
        let mv = parse_move_token(game.board(), tok, ply).map_err(|e| e.to_string())?;
        let zh = format_chinese_move(game.board(), mv, traditional).unwrap_or_else(|_| mv.to_uci());
        game.apply_move(mv).map_err(|e| e.to_string())?;
        zh_moves.push(zh);
    }
    Ok((game, zh_moves))
}

/// Notates each recorded move until the first one that fails to parse or is illegal. The response holds the
/// notated prefix and, when the record breaks, the zero-based index of the first bad ply and its error.
fn notate_moves(
    initial_fen: Option<&str>,
    moves_uci: &[String],
    wxf: bool,
    traditional: bool,
) -> Result<serde_json::Value, String> {
    let start = initial_fen
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(STARTING_FEN);
    let mut game = GameState::from_fen(start, ADOPTED_RULES_PROFILE).map_err(|e| e.to_string())?;
    let mut notations = Vec::with_capacity(moves_uci.len());
    for (ply, tok) in moves_uci.iter().enumerate() {
        let step = parse_move_token(game.board(), tok, ply).and_then(|mv| {
            let text = if wxf {
                format_wxf_move(game.board(), mv)
            } else {
                format_chinese_move(game.board(), mv, traditional)
            }?;
            game.apply_move(mv)?;
            Ok(text)
        });
        match step {
            Ok(text) => notations.push(text),
            Err(e) => {
                return Ok(serde_json::json!({
                    "notations": notations,
                    "invalid_ply": ply,
                    "invalid_reason": e.to_string(),
                }));
            }
        }
    }
    Ok(serde_json::json!({ "notations": notations, "invalid_ply": null, "invalid_reason": null }))
}

pub fn handle_direct_json_request(input: &[u8]) -> i32 {
    let parsed: DirectWasmRequest = match serde_json::from_slice(input) {
        Ok(r) => r,
        Err(e) => return write_response::<serde_json::Value>(false, Err(e.to_string())),
    };

    match parsed {
        DirectWasmRequest::BoardView {
            fen,
            moves_uci,
            width,
            height,
            padding,
            red_below,
            selected_uci,
            traditional_glyphs,
            material_id,
            dark_surface,
            reduced_motion,
        } => {
            let res = (|| -> Result<serde_json::Value, String> {
                let (game, moves_chinese) =
                    replay_game(fen.as_deref(), &moves_uci, traditional_glyphs)?;
                let sel = match selected_uci {
                    Some(ref s) if !s.is_empty() => {
                        Some(Square::from_uci(s).map_err(|e| e.to_string())?)
                    }
                    _ => None,
                };
                let orient = if red_below {
                    Orientation::RedBelow
                } else {
                    Orientation::BlackBelow
                };
                let prefs = PresentationPrefsSnapshot {
                    material_id,
                    glyph_set_id: if traditional_glyphs {
                        "traditional-prototype-v1".to_string()
                    } else {
                        "simplified-prototype-v1".to_string()
                    },
                    dark_surface,
                    reduced_motion,
                };
                let snapshot = BoardViewSnapshot::from_game(
                    &game,
                    game.ply_count() as u64,
                    1,
                    orient,
                    Some((width, height, padding)),
                    sel,
                    Vec::new(),
                    prefs,
                );
                let legal_moves_uci: Vec<String> = game
                    .board()
                    .legal_moves()
                    .into_iter()
                    .map(|m| m.to_uci())
                    .collect();
                Ok(serde_json::json!({
                    "snapshot": snapshot,
                    "fen": game.to_fen(),
                    "moves_chinese": moves_chinese,
                    "legal_moves_uci": legal_moves_uci,
                }))
            })();
            write_response(true, res)
        }
        DirectWasmRequest::ApplyMove {
            fen,
            mut moves_uci,
            token,
            traditional_glyphs,
        } => {
            let res = (|| -> Result<serde_json::Value, String> {
                let (mut game, mut moves_chinese) =
                    replay_game(fen.as_deref(), &moves_uci, traditional_glyphs)?;
                let ply = game.ply_count();
                let mv = parse_move_token(game.board(), &token, ply).map_err(|e| e.to_string())?;
                let zh = format_chinese_move(game.board(), mv, traditional_glyphs)
                    .unwrap_or_else(|_| mv.to_uci());
                let verdict = game.apply_move(mv).map_err(|e| e.to_string())?;
                let uci = mv.to_uci();
                moves_uci.push(uci.clone());
                moves_chinese.push(zh.clone());
                let stm = game.side_to_move();
                let legal_moves_uci: Vec<String> = game
                    .board()
                    .legal_moves()
                    .into_iter()
                    .map(|m| m.to_uci())
                    .collect();
                Ok(serde_json::json!({
                    "move_uci": uci,
                    "move_chinese": zh,
                    "next_fen": game.to_fen(),
                    "moves_uci": moves_uci,
                    "moves_chinese": moves_chinese,
                    "side_to_move": stm.as_str(),
                    "in_check": game.board().is_in_check(stm),
                    "adjudication": verdict,
                    "legal_moves_uci": legal_moves_uci,
                }))
            })();
            write_response(true, res)
        }
        DirectWasmRequest::HitTest {
            width,
            height,
            padding,
            red_below,
            px,
            py,
        } => {
            let orient = if red_below {
                Orientation::RedBelow
            } else {
                Orientation::BlackBelow
            };
            let sq = BoardViewport::compute(width, height, padding)
                .and_then(|vp| vp.screen_to_square(px, py, orient))
                .map(|s| s.to_uci());
            write_response(true, Ok(serde_json::json!({ "square": sq })))
        }
        DirectWasmRequest::ArchiveSearch { query } => {
            let provider = SampleArchiveProvider::new_embedded();
            let manifest = provider.manifest();
            write_response(
                true,
                Ok(serde_json::json!({
                    "dataset_id": manifest.dataset_id,
                    "is_synthetic_fixture": manifest.is_synthetic_development_fixture,
                    "provenance_banner": manifest.provenance_banner,
                    "players": provider.search_players(&query),
                })),
            )
        }
        DirectWasmRequest::ArchiveGame { game_id } => {
            let provider = SampleArchiveProvider::new_embedded();
            match provider.get_game(&game_id) {
                Some(g) => write_response(true, Ok(serde_json::to_value(g).unwrap())),
                None => write_response::<serde_json::Value>(
                    false,
                    Err(format!("Archive game not found: {game_id}")),
                ),
            }
        }
        DirectWasmRequest::NotateMoves {
            fen,
            moves_uci,
            wxf,
            traditional_glyphs,
        } => write_response(
            true,
            notate_moves(fen.as_deref(), &moves_uci, wxf, traditional_glyphs),
        ),
        DirectWasmRequest::ArchivePosition { fen } => {
            let provider = SampleArchiveProvider::new_embedded();
            let target_fen = fen
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(STARTING_FEN);
            match provider.query_position(target_fen) {
                Ok(exp) => write_response(true, Ok(serde_json::to_value(exp).unwrap())),
                Err(e) => write_response::<serde_json::Value>(false, Err(e)),
            }
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn jade_wasm_alloc(size: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(size.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// # Safety
/// `ptr` and `cap` must match a buffer previously returned by `jade_wasm_alloc(cap)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_wasm_free(ptr: *mut u8, cap: usize) {
    if !ptr.is_null() && cap > 0 {
        unsafe {
            let _ = Vec::from_raw_parts(ptr, 0, cap);
        }
    }
}

/// # Safety
/// `req_ptr` must point to `req_len` initialized bytes in linear memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_wasm_call(req_ptr: *const u8, req_len: usize) -> i32 {
    if req_ptr.is_null() {
        return write_response::<serde_json::Value>(false, Err("null request pointer".to_string()));
    }
    let slice = unsafe { std::slice::from_raw_parts(req_ptr, req_len) };
    handle_direct_json_request(slice)
}

#[unsafe(no_mangle)]
pub extern "C" fn jade_wasm_response_ptr() -> *const u8 {
    LAST_RESPONSE.with(|cell| cell.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn jade_wasm_response_len() -> usize {
    LAST_RESPONSE.with(|cell| cell.borrow().len())
}

pub fn crate_ready() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_json_abi_roundtrips_board_view_move_hit_test_and_archive() {
        let bv_status = handle_direct_json_request(
            br#"{"op":"board_view","width":450,"height":500,"padding":16,"red_below":true}"#,
        );
        assert_eq!(bv_status, 1);
        let resp_json: serde_json::Value =
            LAST_RESPONSE.with(|c| serde_json::from_slice(&c.borrow()).unwrap());
        assert_eq!(resp_json["ok"], true);
        assert_eq!(
            resp_json["data"]["snapshot"]["pieces"]
                .as_array()
                .unwrap()
                .len(),
            32
        );

        let mv_status = handle_direct_json_request(br#"{"op":"apply_move","token":"h2e2"}"#);
        assert_eq!(mv_status, 1);
        let mv_json: serde_json::Value =
            LAST_RESPONSE.with(|c| serde_json::from_slice(&c.borrow()).unwrap());
        assert_eq!(mv_json["data"]["move_chinese"], "炮二平五");
    }

    #[test]
    fn notate_moves_follows_dialect_and_reports_the_first_bad_ply() {
        let status = handle_direct_json_request(
            br#"{"op":"notate_moves","moves_uci":["h2e2","h9g7","h0g2","b9c7","b7c7"],"wxf":true}"#,
        );
        assert_eq!(status, 1);
        let json: serde_json::Value =
            LAST_RESPONSE.with(|c| serde_json::from_slice(&c.borrow()).unwrap());
        assert_eq!(
            json["data"]["notations"],
            serde_json::json!(["C2=5", "H8+7", "H2+3", "H2+3"])
        );
        assert_eq!(json["data"]["invalid_ply"], 4);

        handle_direct_json_request(br#"{"op":"notate_moves","moves_uci":["h2e2"]}"#);
        let json: serde_json::Value =
            LAST_RESPONSE.with(|c| serde_json::from_slice(&c.borrow()).unwrap());
        assert_eq!(json["data"]["notations"], serde_json::json!(["炮二平五"]));
        assert!(json["data"]["invalid_ply"].is_null());
    }
}
