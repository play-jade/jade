pub mod adjudication;
pub mod board;
pub mod board_view;
pub mod notation;
pub mod types;

pub use adjudication::{
    Adjudication, DecisionDetails, GameOutcome, GameState, ReasonCode, ViolationLevel,
    chased_piece_ids_on_board,
};
pub use board::Board;
pub use board_view::{
    BoardAnnotation, BoardViewSnapshot, BoardViewport, Orientation, PieceViewRecord,
    PresentationPrefsSnapshot,
};
pub use notation::{
    NotationDialect, ReplayDiagnostic, ReplayResult, format_chinese_move, format_move,
    format_wxf_move, parse_move_token, replay_tokens,
};
pub use types::{
    ACTION_SPACE_SIZE, ADOPTED_RULES_PROFILE, CoreError, Move, Piece, PieceId, PositionKey, Role,
    STARTING_FEN, Side, Square,
};

pub fn crate_ready() -> bool {
    true
}
