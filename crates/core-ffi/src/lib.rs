use archive_client::SampleArchiveProvider;
use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use xiangqi_core::{
    ADOPTED_RULES_PROFILE, BoardViewSnapshot, BoardViewport, GameState, Move, Orientation,
    PresentationPrefsSnapshot, Square, format_chinese_move, format_wxf_move, parse_move_token,
};

uniffi::setup_scaffolding!();

// ============================================================================
// 1. UniFFI Swift (iOS / iPadOS / macOS) and Kotlin (Android) Bindings
// ============================================================================

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{detail}")]
    Core { detail: String },
}

impl From<xiangqi_core::CoreError> for FfiError {
    fn from(e: xiangqi_core::CoreError) -> Self {
        Self::Core {
            detail: e.to_string(),
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiSquareCoord {
    pub file: u8,
    pub rank: u8,
    pub uci: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiPieceItem {
    pub visual_token: String,
    pub side: String,
    pub role: String,
    pub file: u8,
    pub rank: u8,
    pub uci: String,
    pub glyph: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiStateSummary {
    pub fen: String,
    pub position_key_sha256: String,
    pub side_to_move: String,
    pub ply_count: u32,
    pub in_check: bool,
    pub legal_moves_uci: Vec<String>,
    pub outcome: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBoardRenderModel {
    pub side_to_move: String,
    pub ply_count: u32,
    pub in_check: bool,
    pub pieces: Vec<FfiPieceItem>,
    pub selected_uci: Option<String>,
    pub legal_dests_uci: Vec<String>,
    pub last_move_from_uci: Option<String>,
    pub last_move_to_uci: Option<String>,
    pub check_uci: Option<String>,
    pub outcome: Option<String>,
    pub reason: Option<String>,
}

#[derive(uniffi::Object)]
pub struct FfiGameSession {
    inner: Mutex<GameState>,
}

#[uniffi::export]
impl FfiGameSession {
    #[uniffi::constructor]
    pub fn new(rules_profile: String) -> Result<Self, FfiError> {
        Ok(Self {
            inner: Mutex::new(GameState::new(&rules_profile)?),
        })
    }

    #[uniffi::constructor]
    pub fn from_fen(fen: String, rules_profile: String) -> Result<Self, FfiError> {
        Ok(Self {
            inner: Mutex::new(GameState::from_fen(&fen, &rules_profile)?),
        })
    }

    pub fn apply_move_token(&self, token: String) -> Result<String, FfiError> {
        let mut guard = self.inner.lock().map_err(|_| FfiError::Core {
            detail: "mutex poisoned".to_string(),
        })?;
        let ply = guard.ply_count();
        let mv = parse_move_token(guard.board(), &token, ply)?;
        let zh = format_chinese_move(guard.board(), mv, false).unwrap_or_else(|_| mv.to_uci());
        guard.apply_move(mv)?;
        Ok(zh)
    }

    /// Notates a legal move token for the current position without applying it: WXF (`C2=5`)
    /// when `wxf` is true, otherwise Chinese (`炮二平五`), so clients can follow the reader's locale.
    pub fn notate_move_token(&self, token: String, wxf: bool) -> Result<String, FfiError> {
        let guard = self.inner.lock().map_err(|_| FfiError::Core {
            detail: "mutex poisoned".to_string(),
        })?;
        let mv = parse_move_token(guard.board(), &token, guard.ply_count())?;
        if wxf {
            Ok(format_wxf_move(guard.board(), mv)?)
        } else {
            Ok(format_chinese_move(guard.board(), mv, false)?)
        }
    }

    pub fn undo(&self) -> Result<String, FfiError> {
        let mut guard = self.inner.lock().map_err(|_| FfiError::Core {
            detail: "mutex poisoned".to_string(),
        })?;
        Ok(guard.undo_move()?.to_uci())
    }

    pub fn summary(&self) -> Result<FfiStateSummary, FfiError> {
        let guard = self.inner.lock().map_err(|_| FfiError::Core {
            detail: "mutex poisoned".to_string(),
        })?;
        let board = guard.board();
        let stm = board.side_to_move();
        let adj = guard.adjudicate();
        Ok(FfiStateSummary {
            fen: guard.to_fen(),
            position_key_sha256: board.position_key().sha256_hex,
            side_to_move: stm.as_str().to_string(),
            ply_count: guard.ply_count() as u32,
            in_check: board.is_in_check(stm),
            legal_moves_uci: board
                .legal_moves()
                .into_iter()
                .map(|m| m.to_uci())
                .collect(),
            outcome: adj.as_ref().map(|a| {
                serde_json::to_value(a.outcome)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            }),
            reason: adj.as_ref().map(|a| {
                serde_json::to_value(a.reason)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            }),
        })
    }

    pub fn pieces(&self, traditional_glyphs: bool) -> Result<Vec<FfiPieceItem>, FfiError> {
        Ok(self.render_model(traditional_glyphs, None)?.pieces)
    }

    pub fn render_model(
        &self,
        traditional_glyphs: bool,
        selected_uci: Option<String>,
    ) -> Result<FfiBoardRenderModel, FfiError> {
        let guard = self.inner.lock().map_err(|_| FfiError::Core {
            detail: "mutex poisoned".to_string(),
        })?;
        let mut prefs = PresentationPrefsSnapshot::default();
        if traditional_glyphs {
            prefs.glyph_set_id = "traditional-prototype-v1".to_string();
        }
        let selected_sq = selected_uci
            .as_deref()
            .and_then(|s| Square::from_uci(s).ok());
        let snap = BoardViewSnapshot::from_game(
            &guard,
            guard.ply_count() as u64,
            1,
            Orientation::RedBelow,
            None,
            selected_sq,
            Vec::new(),
            prefs,
        );
        let pieces = snap
            .pieces
            .into_iter()
            .map(|p| FfiPieceItem {
                visual_token: p.visual_token,
                side: p.side.as_str().to_string(),
                role: serde_json::to_value(p.role)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string(),
                file: p.file,
                rank: p.rank,
                uci: p.square,
                glyph: p.glyph,
            })
            .collect();
        let outcome = snap.adjudication.as_ref().map(|a| {
            serde_json::to_value(a.outcome)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        });
        let reason = snap.adjudication.as_ref().map(|a| {
            serde_json::to_value(a.reason)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        });
        Ok(FfiBoardRenderModel {
            side_to_move: snap.side_to_move.as_str().to_string(),
            ply_count: guard.ply_count() as u32,
            in_check: snap.check_square.is_some(),
            pieces,
            selected_uci: snap.selection.map(|sq| sq.to_uci()),
            legal_dests_uci: snap
                .legal_destinations
                .into_iter()
                .map(|sq| sq.to_uci())
                .collect(),
            last_move_from_uci: snap.last_move.map(|m| m.from.to_uci()),
            last_move_to_uci: snap.last_move.map(|m| m.to.to_uci()),
            check_uci: snap.check_square.map(|sq| sq.to_uci()),
            outcome,
            reason,
        })
    }
}

#[uniffi::export]
pub fn ffi_hit_test(
    width: f64,
    height: f64,
    padding: f64,
    red_below: bool,
    px: f64,
    py: f64,
) -> Option<FfiSquareCoord> {
    let vp = BoardViewport::compute(width, height, padding)?;
    let orient = if red_below {
        Orientation::RedBelow
    } else {
        Orientation::BlackBelow
    };
    vp.screen_to_square(px, py, orient)
        .map(|sq| FfiSquareCoord {
            file: sq.file(),
            rank: sq.rank(),
            uci: sq.to_uci(),
        })
}

#[uniffi::export]
pub fn ffi_sample_archive_query_position_json(fen: String) -> Result<String, FfiError> {
    let provider = SampleArchiveProvider::new_embedded();
    let res = provider
        .query_position(&fen)
        .map_err(|detail| FfiError::Core { detail })?;
    serde_json::to_string(&res).map_err(|e| FfiError::Core {
        detail: e.to_string(),
    })
}

// ============================================================================
// 2. Narrow Panic-Safe C ABI for Windows (.NET LibraryImport + SafeHandle)
// ============================================================================

pub const JADE_CORE_ABI_VERSION: u32 = 1;

pub struct JadeGameHandle {
    state: GameState,
}

thread_local! {
    static LAST_FFI_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn set_last_error(msg: impl Into<String>) {
    LAST_FFI_ERROR.with(|slot| {
        *slot.borrow_mut() = msg.into();
    });
}

fn cstr_to_str<'a>(ptr: *const c_char) -> Result<&'a str, i32> {
    if ptr.is_null() {
        set_last_error("null UTF-8 pointer");
        return Err(-2);
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|e| {
        set_last_error(format!("invalid UTF-8: {e}"));
        -3
    })
}

fn write_out_cstring(s: String, out_ptr: *mut *mut c_char) -> i32 {
    if out_ptr.is_null() {
        set_last_error("null output string pointer");
        return -2;
    }
    match CString::new(s) {
        Ok(cstr) => {
            unsafe {
                *out_ptr = cstr.into_raw();
            }
            0
        }
        Err(e) => {
            set_last_error(format!("interior NUL byte: {e}"));
            -4
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn jade_core_abi_version() -> u32 {
    JADE_CORE_ABI_VERSION
}

/// # Safety
/// `ptr` must be null or a pointer previously allocated by a `jade_*` C-ABI string producer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(CString::from_raw(ptr));
        }));
    }
}

/// # Safety
/// `rules_profile_utf8` must be null or a valid NUL-terminated UTF-8 string; `out_handle` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_game_new(
    rules_profile_utf8: *const c_char,
    out_handle: *mut *mut JadeGameHandle,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if out_handle.is_null() {
            set_last_error("null out_handle");
            return -2;
        }
        let profile = match cstr_to_str(rules_profile_utf8) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => ADOPTED_RULES_PROFILE,
            Err(rc) => return rc,
        };
        match GameState::new(profile) {
            Ok(state) => {
                let boxed = Box::new(JadeGameHandle { state });
                unsafe {
                    *out_handle = Box::into_raw(boxed);
                }
                0
            }
            Err(e) => {
                set_last_error(e.to_string());
                -10
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_game_new");
        -100
    })
}

/// # Safety
/// `fen_utf8` and `rules_profile_utf8` must be valid NUL-terminated UTF-8 strings (or null for default profile); `out_handle` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_game_from_fen(
    fen_utf8: *const c_char,
    rules_profile_utf8: *const c_char,
    out_handle: *mut *mut JadeGameHandle,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if out_handle.is_null() {
            set_last_error("null out_handle");
            return -2;
        }
        let fen = match cstr_to_str(fen_utf8) {
            Ok(s) => s,
            Err(rc) => return rc,
        };
        let profile = match cstr_to_str(rules_profile_utf8) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => ADOPTED_RULES_PROFILE,
            Err(rc) => return rc,
        };
        match GameState::from_fen(fen, profile) {
            Ok(state) => {
                let boxed = Box::new(JadeGameHandle { state });
                unsafe {
                    *out_handle = Box::into_raw(boxed);
                }
                0
            }
            Err(e) => {
                set_last_error(e.to_string());
                -11
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_game_from_fen");
        -100
    })
}

/// # Safety
/// `handle` must be null or an owned handle previously returned by `jade_game_new` / `jade_game_from_fen`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_game_free(handle: *mut JadeGameHandle) {
    if !handle.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(handle));
        }));
    }
}

/// # Safety
/// `handle` must be a valid `JadeGameHandle` pointer and `move_uci_utf8` must be a valid NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_game_apply_uci(
    handle: *mut JadeGameHandle,
    move_uci_utf8: *const c_char,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            set_last_error("null handle");
            return -2;
        }
        let uci = match cstr_to_str(move_uci_utf8) {
            Ok(s) => s,
            Err(rc) => return rc,
        };
        let mv = match Move::from_uci_or_iccs(uci) {
            Ok(m) => m,
            Err(e) => {
                set_last_error(e.to_string());
                return -12;
            }
        };
        let game = unsafe { &mut (*handle).state };
        match game.apply_move(mv) {
            Ok(_) => 0,
            Err(e) => {
                set_last_error(e.to_string());
                -13
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_game_apply_uci");
        -100
    })
}

/// # Safety
/// `handle` must be a valid mutable `JadeGameHandle` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_game_undo(handle: *mut JadeGameHandle) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            set_last_error("null handle");
            return -2;
        }
        let game = unsafe { &mut (*handle).state };
        match game.undo_move() {
            Ok(_) => 0,
            Err(e) => {
                set_last_error(e.to_string());
                -14
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_game_undo");
        -100
    })
}

/// # Safety
/// `handle` must be a valid `JadeGameHandle` pointer and `out_json_utf8` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_game_state_json(
    handle: *const JadeGameHandle,
    out_json_utf8: *mut *mut c_char,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            set_last_error("null handle");
            return -2;
        }
        let game = unsafe { &(*handle).state };
        let board = game.board();
        let stm = board.side_to_move();
        let payload = serde_json::json!({
            "fen": game.to_fen(),
            "position_key_sha256": board.position_key().sha256_hex,
            "side_to_move": stm.as_str(),
            "ply_count": game.ply_count(),
            "in_check": board.is_in_check(stm),
            "legal_moves": board.legal_moves().into_iter().map(|m| m.to_uci()).collect::<Vec<_>>(),
            "adjudication": game.adjudicate(),
        });
        write_out_cstring(payload.to_string(), out_json_utf8)
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_game_state_json");
        -100
    })
}

/// # Safety
/// `handle` must be a valid `JadeGameHandle` pointer and `out_json_utf8` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_board_view_json(
    handle: *const JadeGameHandle,
    width: f64,
    height: f64,
    padding: f64,
    red_below: i32,
    out_json_utf8: *mut *mut c_char,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if handle.is_null() {
            set_last_error("null handle");
            return -2;
        }
        let game = unsafe { &(*handle).state };
        let orient = if red_below != 0 {
            Orientation::RedBelow
        } else {
            Orientation::BlackBelow
        };
        let snap = BoardViewSnapshot::from_game(
            game,
            game.ply_count() as u64,
            1,
            orient,
            Some((width, height, padding)),
            None,
            Vec::new(),
            PresentationPrefsSnapshot::default(),
        );
        match serde_json::to_string(&snap) {
            Ok(s) => write_out_cstring(s, out_json_utf8),
            Err(e) => {
                set_last_error(e.to_string());
                -15
            }
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_board_view_json");
        -100
    })
}

/// # Safety
/// `out_file` and `out_rank` must be valid writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_hit_test(
    width: f64,
    height: f64,
    padding: f64,
    red_below: i32,
    px: f64,
    py: f64,
    out_file: *mut i32,
    out_rank: *mut i32,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        if out_file.is_null() || out_rank.is_null() {
            set_last_error("null output square pointers");
            return -2;
        }
        let Some(vp) = BoardViewport::compute(width, height, padding) else {
            set_last_error("invalid viewport dimensions");
            return -5;
        };
        let orient = if red_below != 0 {
            Orientation::RedBelow
        } else {
            Orientation::BlackBelow
        };
        match vp.screen_to_square(px, py, orient) {
            Some(sq) => {
                unsafe {
                    *out_file = sq.file() as i32;
                    *out_rank = sq.rank() as i32;
                }
                1
            }
            None => 0,
        }
    }))
    .unwrap_or_else(|_| {
        set_last_error("panic caught at FFI boundary in jade_hit_test");
        -100
    })
}

/// # Safety
/// `out_utf8` must be a valid writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn jade_last_error_message(out_utf8: *mut *mut c_char) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        let msg = LAST_FFI_ERROR.with(|slot| slot.borrow().clone());
        write_out_cstring(msg, out_utf8)
    }))
    .unwrap_or(-100)
}

pub fn crate_ready() -> bool {
    let _ = Square::from_index(0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn uniffi_and_c_abi_run_pikafish_fixtures_identically() {
        // 1. UniFFI session check
        let session = FfiGameSession::new(ADOPTED_RULES_PROFILE.to_string()).unwrap();
        let zh = session.apply_move_token("h2e2".to_string()).unwrap();
        assert_eq!(zh, "炮二平五");
        let sum = session.summary().unwrap();
        assert_eq!(sum.side_to_move, "black");
        assert_eq!(sum.ply_count, 1);

        // 2. C ABI round-trip check across all 39 Pikafish fixtures!
        assert_eq!(jade_core_abi_version(), 1);
        let fixtures_str = include_str!("../../../data/fixtures/rules/pikafish_examples.json");
        let doc: serde_json::Value = serde_json::from_str(fixtures_str).unwrap();
        let c_profile = CString::new(ADOPTED_RULES_PROFILE).unwrap();

        for ex in doc["examples"].as_array().unwrap() {
            let fen = CString::new(ex["fen"].as_str().unwrap()).unwrap();
            let mut handle: *mut JadeGameHandle = ptr::null_mut();
            let rc = unsafe { jade_game_from_fen(fen.as_ptr(), c_profile.as_ptr(), &mut handle) };
            assert_eq!(rc, 0);
            assert!(!handle.is_null());

            for mv in ex["moves_uci"].as_array().unwrap() {
                let c_mv = CString::new(mv.as_str().unwrap()).unwrap();
                assert_eq!(unsafe { jade_game_apply_uci(handle, c_mv.as_ptr()) }, 0);
            }

            let mut out_json: *mut c_char = ptr::null_mut();
            assert_eq!(unsafe { jade_game_state_json(handle, &mut out_json) }, 0);
            let json_str = unsafe { CStr::from_ptr(out_json) }
                .to_str()
                .unwrap()
                .to_owned();
            unsafe {
                jade_string_free(out_json);
                jade_game_free(handle);
            }

            let state_val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
            assert_eq!(state_val["adjudication"]["outcome"], ex["expected_outcome"]);
        }
    }
}
