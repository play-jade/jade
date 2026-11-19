use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use thiserror::Error;

pub const STARTING_FEN: &str =
    "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1";
pub const ADOPTED_RULES_PROFILE: &str = "program-2023.11-v1";
pub const ACTION_SPACE_SIZE: usize = 8100;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    #[error("invalid square coordinate: {0}")]
    InvalidSquare(String),
    #[error("invalid move format: {0}")]
    InvalidMoveFormat(String),
    #[error("illegal move: {0}")]
    IllegalMove(String),
    #[error("ambiguous notation token at ply {ply}: {token}")]
    AmbiguousNotation { ply: usize, token: String },
    #[error("invalid FEN: {0}")]
    InvalidFen(String),
    #[error("no move to undo")]
    NothingToUndo,
    #[error("unsupported rules profile: {0}")]
    UnsupportedRulesProfile(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Red,
    Black,
}

impl Side {
    #[inline]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Red => Self::Black,
            Self::Black => Self::Red,
        }
    }

    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Black => "black",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    General,
    Advisor,
    Elephant,
    Chariot,
    Horse,
    Cannon,
    Soldier,
}

impl Role {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Advisor,
        Self::Elephant,
        Self::Chariot,
        Self::Horse,
        Self::Cannon,
        Self::Soldier,
    ];

    pub const fn fen_char(self, side: Side) -> char {
        let lower = match self {
            Self::General => 'k',
            Self::Advisor => 'a',
            Self::Elephant => 'b',
            Self::Chariot => 'r',
            Self::Horse => 'n',
            Self::Cannon => 'c',
            Self::Soldier => 'p',
        };
        match side {
            Side::Red => lower.to_ascii_uppercase(),
            Side::Black => lower,
        }
    }

    pub const fn from_fen_char(ch: char) -> Option<(Side, Self)> {
        let side = if ch.is_ascii_uppercase() {
            Side::Red
        } else if ch.is_ascii_lowercase() {
            Side::Black
        } else {
            return None;
        };
        let role = match ch.to_ascii_lowercase() {
            'k' => Self::General,
            'a' => Self::Advisor,
            'b' | 'e' => Self::Elephant,
            'r' => Self::Chariot,
            'n' | 'h' => Self::Horse,
            'c' => Self::Cannon,
            'p' => Self::Soldier,
            _ => return None,
        };
        Some((side, role))
    }

    pub const fn simplified_glyph(self, side: Side) -> &'static str {
        match (side, self) {
            (Side::Red, Self::General) => "帅",
            (Side::Black, Self::General) => "将",
            (Side::Red, Self::Advisor) => "仕",
            (Side::Black, Self::Advisor) => "士",
            (Side::Red, Self::Elephant) => "相",
            (Side::Black, Self::Elephant) => "象",
            (_, Self::Chariot) => "车",
            (_, Self::Horse) => "马",
            (_, Self::Cannon) => "炮",
            (Side::Red, Self::Soldier) => "兵",
            (Side::Black, Self::Soldier) => "卒",
        }
    }

    pub const fn traditional_glyph(self, side: Side) -> &'static str {
        match (side, self) {
            (Side::Red, Self::General) => "帥",
            (Side::Black, Self::General) => "將",
            (Side::Red, Self::Advisor) => "仕",
            (Side::Black, Self::Advisor) => "士",
            (Side::Red, Self::Elephant) => "相",
            (Side::Black, Self::Elephant) => "象",
            (_, Self::Chariot) => "車",
            (_, Self::Horse) => "馬",
            (Side::Red, Self::Cannon) => "炮",
            (Side::Black, Self::Cannon) => "砲",
            (Side::Red, Self::Soldier) => "兵",
            (Side::Black, Self::Soldier) => "卒",
        }
    }
}

/// Internal token assigned per physical piece in a game to track chased targets
/// across repetition cycles without polluting `PositionKey` graph identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PieceId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Piece {
    pub id: PieceId,
    pub side: Side,
    pub role: Role,
}

/// Canonical Xiangqi square (`file` x = 0..8 left-to-right from Red's view,
/// `rank` y = 0..9 bottom-to-top from Red's home rank).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Square(u8);

impl Square {
    #[inline]
    pub const fn from_file_rank(file: u8, rank: u8) -> Option<Self> {
        if file < 9 && rank < 10 {
            Some(Self(rank * 9 + file))
        } else {
            None
        }
    }

    #[inline]
    pub const fn from_index(idx: u8) -> Option<Self> {
        if idx < 90 { Some(Self(idx)) } else { None }
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn file(self) -> u8 {
        self.0 % 9
    }

    #[inline]
    pub const fn rank(self) -> u8 {
        self.0 / 9
    }

    #[inline]
    pub const fn mirror_file(self) -> Self {
        Self(self.rank() * 9 + (8 - self.file()))
    }

    #[inline]
    pub const fn is_in_palace(self, side: Side) -> bool {
        let f = self.file();
        let r = self.rank();
        if f < 3 || f > 5 {
            return false;
        }
        match side {
            Side::Red => r <= 2,
            Side::Black => r >= 7,
        }
    }

    #[inline]
    pub const fn has_crossed_river(self, side: Side) -> bool {
        match side {
            Side::Red => self.rank() >= 5,
            Side::Black => self.rank() <= 4,
        }
    }

    pub fn to_uci(self) -> String {
        format!("{}{}", (b'a' + self.file()) as char, self.rank())
    }

    pub fn to_iccs(self) -> String {
        format!("{}{}", (b'A' + self.file()) as char, self.rank())
    }

    pub fn from_uci(s: &str) -> Result<Self, CoreError> {
        let bytes = s.as_bytes();
        if bytes.len() != 2 {
            return Err(CoreError::InvalidSquare(s.to_string()));
        }
        let f_ch = bytes[0].to_ascii_lowercase();
        let r_ch = bytes[1];
        if !(b'a'..=b'i').contains(&f_ch) || !r_ch.is_ascii_digit() {
            return Err(CoreError::InvalidSquare(s.to_string()));
        }
        Ok(Self::from_file_rank(f_ch - b'a', r_ch - b'0').unwrap())
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", (b'a' + self.file()) as char, self.rank())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Move {
    pub from: Square,
    pub to: Square,
}

impl Move {
    #[inline]
    pub const fn new(from: Square, to: Square) -> Self {
        Self { from, to }
    }

    /// Baseline 8,100-label action encoding (`from_square * 90 + to_square`) per Section 13.
    #[inline]
    pub const fn action_index(self) -> u16 {
        (self.from.0 as u16) * 90 + (self.to.0 as u16)
    }

    #[inline]
    pub const fn from_action_index(idx: u16) -> Option<Self> {
        if idx >= 8100 {
            return None;
        }
        let from = Square((idx / 90) as u8);
        let to = Square((idx % 90) as u8);
        Some(Self { from, to })
    }

    #[inline]
    pub const fn mirror_file(self) -> Self {
        Self {
            from: self.from.mirror_file(),
            to: self.to.mirror_file(),
        }
    }

    pub fn to_uci(self) -> String {
        format!("{}{}", self.from, self.to)
    }

    pub fn to_iccs(self) -> String {
        format!("{}-{}", self.from.to_iccs(), self.to.to_iccs())
    }

    pub fn from_uci_or_iccs(s: &str) -> Result<Self, CoreError> {
        let trimmed = s.trim();
        if trimmed.len() == 4 {
            let from = Square::from_uci(&trimmed[0..2])?;
            let to = Square::from_uci(&trimmed[2..4])?;
            Ok(Self { from, to })
        } else if trimmed.len() == 5 && trimmed.as_bytes()[2] == b'-' {
            let from = Square::from_uci(&trimmed[0..2])?;
            let to = Square::from_uci(&trimmed[3..5])?;
            Ok(Self { from, to })
        } else {
            Err(CoreError::InvalidMoveFormat(s.to_string()))
        }
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.from, self.to)
    }
}

/// Versioned canonical placement + side-to-move key (Section 11).
/// Excludes `PieceId`, move history, and clock counters; stores both canonical bytes
/// and SHA-256 hex digest for exact collision-safe comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PositionKey {
    pub version: u8,
    pub canonical_bytes: Vec<u8>,
    pub sha256_hex: String,
}

impl PositionKey {
    pub fn compute_v1(squares: &[Option<Piece>; 90], side_to_move: Side) -> Self {
        let mut bytes = Vec::with_capacity(92);
        bytes.push(1u8); // encoding version 1
        bytes.push(match side_to_move {
            Side::Red => 0u8,
            Side::Black => 1u8,
        });
        for cell in squares {
            let code = match cell {
                None => 0u8,
                Some(p) => {
                    let base = match p.role {
                        Role::General => 1u8,
                        Role::Advisor => 2u8,
                        Role::Elephant => 3u8,
                        Role::Chariot => 4u8,
                        Role::Horse => 5u8,
                        Role::Cannon => 6u8,
                        Role::Soldier => 7u8,
                    };
                    match p.side {
                        Side::Red => base,
                        Side::Black => base + 8,
                    }
                }
            };
            bytes.push(code);
        }
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let sha256_hex = hex::encode(hasher.finalize());
        Self {
            version: 1,
            canonical_bytes: bytes,
            sha256_hex,
        }
    }
}
