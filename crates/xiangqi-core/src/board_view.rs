use crate::adjudication::{Adjudication, GameState};
use crate::types::{Move, PieceId, Role, Side, Square};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    RedBelow,
    BlackBelow,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoardViewport {
    pub width: f64,
    pub height: f64,
    pub padding: f64,
    pub cell_size: f64,
    pub origin_x: f64,
    pub origin_y: f64,
}

impl BoardViewport {
    /// Section 4 exact viewport geometry:
    /// `s = min((W - 2*p) / 9, (H - 2*p) / 10)`
    /// `ox = (W - 8*s) / 2`, `oy = (H - 9*s) / 2`
    /// Returns `None` if `p < 0` or `s <= 0`.
    pub fn compute(width: f64, height: f64, padding: f64) -> Option<Self> {
        if !width.is_finite() || !height.is_finite() || !padding.is_finite() || padding < 0.0 {
            return None;
        }
        let s = ((width - 2.0 * padding) / 9.0).min((height - 2.0 * padding) / 10.0);
        if s <= 0.0 {
            return None;
        }
        let origin_x = (width - 8.0 * s) / 2.0;
        let origin_y = (height - 9.0 * s) / 2.0;
        Some(Self {
            width,
            height,
            padding,
            cell_size: s,
            origin_x,
            origin_y,
        })
    }

    #[inline]
    pub fn square_to_screen(&self, sq: Square, orientation: Orientation) -> (f64, f64) {
        let x = sq.file() as f64;
        let y = sq.rank() as f64;
        let s = self.cell_size;
        match orientation {
            Orientation::RedBelow => (self.origin_x + s * x, self.origin_y + s * (9.0 - y)),
            Orientation::BlackBelow => (self.origin_x + s * (8.0 - x), self.origin_y + s * y),
        }
    }

    /// Section 4 exact pointer hit-testing with deterministic half-open cells:
    /// Accept ONLY `-0.5 <= u < 8.5` and `-0.5 <= v < 9.5`; never clamp outside points.
    pub fn screen_to_square(&self, px: f64, py: f64, orientation: Orientation) -> Option<Square> {
        if !px.is_finite() || !py.is_finite() {
            return None;
        }
        let u = (px - self.origin_x) / self.cell_size;
        let v = (py - self.origin_y) / self.cell_size;
        if !(-0.5..8.5).contains(&u) || !(-0.5..9.5).contains(&v) {
            return None;
        }
        let c = (u + 0.5).floor() as u8;
        let r = (v + 0.5).floor() as u8;
        match orientation {
            Orientation::RedBelow => Square::from_file_rank(c, 9 - r),
            Orientation::BlackBelow => Square::from_file_rank(8 - c, r),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PieceViewRecord {
    pub visual_token: String,
    pub piece_id: PieceId,
    pub side: Side,
    pub role: Role,
    pub file: u8,
    pub rank: u8,
    pub square: String,
    pub glyph: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardAnnotation {
    pub kind: String, // "arrow" | "highlight"
    pub from: Square,
    pub to: Option<Square>,
    pub color_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationPrefsSnapshot {
    pub material_id: String,
    pub glyph_set_id: String,
    pub dark_surface: bool,
    pub reduced_motion: bool,
}

impl Default for PresentationPrefsSnapshot {
    fn default() -> Self {
        Self {
            material_id: "pale-jade".to_string(),
            glyph_set_id: "simplified-prototype-v1".to_string(),
            dark_surface: false,
            reduced_motion: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardViewSnapshot {
    pub contract_version: u8,
    pub state_id: String,
    pub source_revision: u64,
    pub presentation_revision: u64,
    pub side_to_move: Side,
    pub orientation: Orientation,
    pub viewport: Option<BoardViewport>,
    pub pieces: Vec<PieceViewRecord>,
    pub selection: Option<Square>,
    pub legal_destinations: Vec<Square>,
    pub last_move: Option<Move>,
    pub check_square: Option<Square>,
    pub annotations: Vec<BoardAnnotation>,
    pub preferences: PresentationPrefsSnapshot,
    pub adjudication: Option<Adjudication>,
}

impl BoardViewSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn from_game(
        game: &GameState,
        source_revision: u64,
        presentation_revision: u64,
        orientation: Orientation,
        viewport_dims: Option<(f64, f64, f64)>,
        selection: Option<Square>,
        annotations: Vec<BoardAnnotation>,
        preferences: PresentationPrefsSnapshot,
    ) -> Self {
        let board = game.board();
        let pos_key = board.position_key();
        let traditional = preferences.glyph_set_id.starts_with("traditional");

        let mut pieces = Vec::with_capacity(32);
        for idx in 0..90u8 {
            let sq = Square::from_index(idx).unwrap();
            if let Some(p) = board.piece_at(sq) {
                let glyph = if traditional {
                    p.role.traditional_glyph(p.side)
                } else {
                    p.role.simplified_glyph(p.side)
                };
                pieces.push(PieceViewRecord {
                    visual_token: format!("piece-{}", p.id.0),
                    piece_id: p.id,
                    side: p.side,
                    role: p.role,
                    file: sq.file(),
                    rank: sq.rank(),
                    square: sq.to_uci(),
                    glyph: glyph.to_string(),
                });
            }
        }

        let legal_destinations = if let Some(sel) = selection {
            board
                .legal_moves()
                .into_iter()
                .filter(|mv| mv.from == sel)
                .map(|mv| mv.to)
                .collect()
        } else {
            Vec::new()
        };

        let stm = board.side_to_move();
        let check_square = if board.is_in_check(stm) {
            board.general_square(stm)
        } else {
            None
        };

        let viewport = viewport_dims.and_then(|(w, h, p)| BoardViewport::compute(w, h, p));

        Self {
            contract_version: 1,
            state_id: pos_key.sha256_hex,
            source_revision,
            presentation_revision,
            side_to_move: stm,
            orientation,
            viewport,
            pieces,
            selection,
            legal_destinations,
            last_move: game.last_move(),
            check_square,
            annotations,
            preferences,
            adjudication: game.adjudicate(),
        }
    }
}
