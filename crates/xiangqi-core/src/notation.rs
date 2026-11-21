use crate::adjudication::{Adjudication, GameState};
use crate::board::Board;
use crate::types::{ADOPTED_RULES_PROFILE, CoreError, Move, Piece, Role, Side, Square};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotationDialect {
    Uci,
    Iccs,
    Wxf,
    ChineseSimplified,
    ChineseTraditional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayDiagnostic {
    pub ply: usize,
    pub token: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayResult {
    pub parser_version: u8,
    pub dialect: NotationDialect,
    pub initial_fen: String,
    pub valid_moves: Vec<Move>,
    pub final_fen: String,
    pub complete: bool,
    pub terminal_adjudication: Option<Adjudication>,
    pub diagnostic: Option<ReplayDiagnostic>,
}

const RED_NUMERALS: [char; 9] = ['一', '二', '三', '四', '五', '六', '七', '八', '九'];
const FULLWIDTH_DIGITS: [char; 9] = ['１', '２', '３', '４', '５', '６', '７', '８', '９'];

#[inline]
fn mover_file_number(side: Side, canonical_file: u8) -> u8 {
    match side {
        Side::Red => 9 - canonical_file,
        Side::Black => canonical_file + 1,
    }
}

#[inline]
fn format_numeral(side: Side, n: u8, fullwidth_black: bool) -> char {
    let idx = (n.saturating_sub(1) as usize).min(8);
    match side {
        Side::Red => RED_NUMERALS[idx],
        Side::Black => {
            if fullwidth_black {
                FULLWIDTH_DIGITS[idx]
            } else {
                (b'1' + idx as u8) as char
            }
        }
    }
}

fn parse_numeral_1_to_9(ch: char) -> Option<u8> {
    if ('1'..='9').contains(&ch) {
        return Some((ch as u8) - b'0');
    }
    if let Some(pos) = FULLWIDTH_DIGITS.iter().position(|&c| c == ch) {
        return Some((pos as u8) + 1);
    }
    if let Some(pos) = RED_NUMERALS.iter().position(|&c| c == ch) {
        return Some((pos as u8) + 1);
    }
    None
}

/// Pieces of `(side, role)` on `file` ordered from front-most (closest to enemy) to rear-most.
fn ordered_same_role_on_file(board: &Board, side: Side, role: Role, file: u8) -> Vec<Square> {
    let mut list = Vec::new();
    for rank in 0..10u8 {
        let sq = Square::from_file_rank(file, rank).unwrap();
        if let Some(p) = board.piece_at(sq)
            && p.side == side
            && p.role == role
        {
            list.push(sq);
        }
    }
    match side {
        Side::Red => list.sort_by_key(|a| std::cmp::Reverse(a.rank())),
        Side::Black => list.sort_by_key(|a| a.rank()),
    }
    list
}

fn count_multi_soldier_files(board: &Board, side: Side) -> usize {
    (0..9u8)
        .filter(|&f| ordered_same_role_on_file(board, side, Role::Soldier, f).len() >= 2)
        .count()
}

pub fn format_move(board: &Board, mv: Move, dialect: NotationDialect) -> Result<String, CoreError> {
    match dialect {
        NotationDialect::Uci => Ok(mv.to_uci()),
        NotationDialect::Iccs => Ok(mv.to_iccs()),
        NotationDialect::Wxf => format_wxf_move(board, mv),
        NotationDialect::ChineseSimplified => format_chinese_move(board, mv, false),
        NotationDialect::ChineseTraditional => format_chinese_move(board, mv, true),
    }
}

pub fn format_chinese_move(
    board: &Board,
    mv: Move,
    traditional: bool,
) -> Result<String, CoreError> {
    let Some(piece) = board.piece_at(mv.from) else {
        return Err(CoreError::IllegalMove(mv.to_uci()));
    };
    let side = piece.side;
    let role_glyph = if traditional {
        piece.role.traditional_glyph(side)
    } else {
        piece.role.simplified_glyph(side)
    };

    let same_file = ordered_same_role_on_file(board, side, piece.role, mv.from.file());
    let src_file_num = mover_file_number(side, mv.from.file());

    let (first_two_chars, _) =
        if same_file.len() >= 2 && !matches!(piece.role, Role::Advisor | Role::Elephant) {
            let pos = same_file.iter().position(|&sq| sq == mv.from).unwrap_or(0);
            if piece.role == Role::Soldier {
                let multi_files = count_multi_soldier_files(board, side);
                if multi_files >= 2 {
                    // Pikafish Ch 2 Sec 6 Clause 3.b: two files each with >= 2 pawns -> 前/中/后 + file numeral
                    let rank_label = match (same_file.len(), pos) {
                        (_, 0) => '前',
                        (n, p) if p + 1 == n => '后',
                        _ => '中',
                    };
                    let f_ch = format_numeral(side, src_file_num, false);
                    (format!("{rank_label}{f_ch}"), true)
                } else {
                    let prefix = match (same_file.len(), pos) {
                        (_, 0) => '前',
                        (n, p) if p + 1 == n => '后',
                        (3, 1) => '中',
                        (_, 1) => '二',
                        (_, 2) => '三',
                        _ => '四',
                    };
                    (format!("{prefix}{role_glyph}"), true)
                }
            } else {
                let prefix = if pos == 0 {
                    '前'
                } else if pos + 1 == same_file.len() {
                    '后'
                } else {
                    '中'
                };
                (format!("{prefix}{role_glyph}"), true)
            }
        } else {
            let f_ch = format_numeral(side, src_file_num, false);
            (format!("{role_glyph}{f_ch}"), false)
        };

    let dr = (mv.to.rank() as i8) - (mv.from.rank() as i8);
    let forward_dr = match side {
        Side::Red => dr,
        Side::Black => -dr,
    };

    let (dir_ch, fourth_num) = if dr == 0 {
        ('平', mover_file_number(side, mv.to.file()))
    } else {
        let dir = if forward_dr > 0 { '进' } else { '退' };
        let arg = if matches!(piece.role, Role::Horse | Role::Advisor | Role::Elephant) {
            mover_file_number(side, mv.to.file())
        } else {
            dr.unsigned_abs()
        };
        (dir, arg)
    };

    let dir_out = if traditional && dir_ch == '进' {
        '進'
    } else {
        dir_ch
    };
    let fourth_ch = format_numeral(side, fourth_num, false);

    Ok(format!("{first_two_chars}{dir_out}{fourth_ch}"))
}

pub fn format_wxf_move(board: &Board, mv: Move) -> Result<String, CoreError> {
    let Some(piece) = board.piece_at(mv.from) else {
        return Err(CoreError::IllegalMove(mv.to_uci()));
    };
    let side = piece.side;
    let role_letter = match piece.role {
        Role::General => 'K',
        Role::Advisor => 'A',
        Role::Elephant => 'E',
        Role::Chariot => 'R',
        Role::Horse => 'H',
        Role::Cannon => 'C',
        Role::Soldier => 'P',
    };
    let same_file = ordered_same_role_on_file(board, side, piece.role, mv.from.file());
    let src_file_num = mover_file_number(side, mv.from.file());
    let prefix = if same_file.len() >= 2 && !matches!(piece.role, Role::Advisor | Role::Elephant) {
        let pos = same_file.iter().position(|&sq| sq == mv.from).unwrap_or(0);
        let sign = if pos == 0 { '+' } else { '-' };
        format!("{sign}{role_letter}")
    } else {
        format!("{role_letter}{src_file_num}")
    };

    let dr = (mv.to.rank() as i8) - (mv.from.rank() as i8);
    let forward_dr = match side {
        Side::Red => dr,
        Side::Black => -dr,
    };
    let (op, arg) = if dr == 0 {
        ('=', mover_file_number(side, mv.to.file()))
    } else {
        let op = if forward_dr > 0 { '+' } else { '-' };
        let arg = if matches!(piece.role, Role::Horse | Role::Advisor | Role::Elephant) {
            mover_file_number(side, mv.to.file())
        } else {
            dr.unsigned_abs()
        };
        (op, arg)
    };
    Ok(format!("{prefix}{op}{arg}"))
}

fn normalize_chinese_token(token: &str) -> String {
    token
        .trim()
        .chars()
        .map(|ch| match ch {
            '帥' => '帅',
            '將' => '将',
            '車' | '砗' => '车',
            '馬' | '傌' => '马',
            '砲' | '包' => '炮',
            '進' => '进',
            '後' => '后',
            c => {
                if let Some(pos) = FULLWIDTH_DIGITS.iter().position(|&fw| fw == c) {
                    (b'1' + pos as u8) as char
                } else {
                    c
                }
            }
        })
        .collect()
}

fn parse_role_char_chinese(ch: char) -> Option<Role> {
    match ch {
        '帅' | '将' => Some(Role::General),
        '仕' | '士' => Some(Role::Advisor),
        '相' | '象' => Some(Role::Elephant),
        '车' => Some(Role::Chariot),
        '马' => Some(Role::Horse),
        '炮' => Some(Role::Cannon),
        '兵' | '卒' => Some(Role::Soldier),
        _ => None,
    }
}

/// Parses a single move token against `board` in UCI, ICCS, WXF, or Chinese 4-char notation.
pub fn parse_move_token(board: &Board, token: &str, ply: usize) -> Result<Move, CoreError> {
    let trimmed = token.trim();
    let legal = board.legal_moves();

    // 1. Direct UCI (`h2e2`) or ICCS (`H2-E2`)
    if let Ok(mv) = Move::from_uci_or_iccs(trimmed) {
        if legal.contains(&mv) {
            return Ok(mv);
        }
        return Err(CoreError::IllegalMove(trimmed.to_string()));
    }

    // 2. WXF (`C2=5`, `H8+7`, `+R=4`, also accept `B` for Elephant and `N` for Horse)
    if trimmed.is_ascii() && trimmed.len() == 4 {
        let upper = trimmed.to_ascii_uppercase();
        let canonical_wxf: String = upper
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if i <= 1 {
                    match c {
                        'B' => 'E',
                        'N' => 'H',
                        other => other,
                    }
                } else {
                    c
                }
            })
            .collect();
        let matches: Vec<Move> = legal
            .iter()
            .copied()
            .filter(|&mv| {
                format_wxf_move(board, mv).ok().as_deref() == Some(canonical_wxf.as_str())
            })
            .collect();
        return match matches.len() {
            1 => Ok(matches[0]),
            0 => Err(CoreError::IllegalMove(trimmed.to_string())),
            _ => Err(CoreError::AmbiguousNotation {
                ply,
                token: trimmed.to_string(),
            }),
        };
    }

    // 3. Chinese 4-character notation (supporting full-width digits, traditional/variant glyphs,
    //    and relaxed Red/Black numeral interchange in historical records).
    let norm = normalize_chinese_token(trimmed);
    let chars: Vec<char> = norm.chars().collect();
    if chars.len() != 4 {
        return Err(CoreError::InvalidMoveFormat(trimmed.to_string()));
    }

    let side = board.side_to_move();
    let Some(fourth_val) = parse_numeral_1_to_9(chars[3]) else {
        return Err(CoreError::InvalidMoveFormat(trimmed.to_string()));
    };
    let dir_ch = chars[2];
    if !matches!(dir_ch, '平' | '进' | '退') {
        return Err(CoreError::InvalidMoveFormat(trimmed.to_string()));
    }

    let matches: Vec<Move> = legal
        .iter()
        .copied()
        .filter(|&mv| {
            let Some(piece) = board.piece_at(mv.from) else {
                return false;
            };
            chinese_move_matches(board, mv, piece, &chars, dir_ch, fourth_val)
        })
        .collect();

    match matches.len() {
        1 => Ok(matches[0]),
        0 => {
            let _ = side;
            Err(CoreError::IllegalMove(trimmed.to_string()))
        }
        _ => Err(CoreError::AmbiguousNotation {
            ply,
            token: trimmed.to_string(),
        }),
    }
}

fn chinese_move_matches(
    board: &Board,
    mv: Move,
    piece: Piece,
    chars: &[char],
    dir_ch: char,
    fourth_val: u8,
) -> bool {
    let side = piece.side;
    let dr = (mv.to.rank() as i8) - (mv.from.rank() as i8);
    let forward_dr = match side {
        Side::Red => dr,
        Side::Black => -dr,
    };

    // Check direction and 4th character
    match dir_ch {
        '平' => {
            if dr != 0 || mover_file_number(side, mv.to.file()) != fourth_val {
                return false;
            }
        }
        '进' => {
            if forward_dr <= 0 {
                return false;
            }
            let expected = if matches!(piece.role, Role::Horse | Role::Advisor | Role::Elephant) {
                mover_file_number(side, mv.to.file())
            } else {
                dr.unsigned_abs()
            };
            if expected != fourth_val {
                return false;
            }
        }
        '退' => {
            if forward_dr >= 0 {
                return false;
            }
            let expected = if matches!(piece.role, Role::Horse | Role::Advisor | Role::Elephant) {
                mover_file_number(side, mv.to.file())
            } else {
                dr.unsigned_abs()
            };
            if expected != fourth_val {
                return false;
            }
        }
        _ => return false,
    }

    // Check first two characters:
    // Case A: `<Role><FileNum>` (e.g. `炮二`, `马8`)
    if let Some(role) = parse_role_char_chinese(chars[0]) {
        if role != piece.role {
            return false;
        }
        let Some(f_num) = parse_numeral_1_to_9(chars[1]) else {
            return false;
        };
        return mover_file_number(side, mv.from.file()) == f_num;
    }

    // Case B: `<OrderPrefix><Role>` (`前马`, `后炮`, `中兵`, `二兵`, `三兵`, `四兵`)
    //     or `<OrderPrefix><FileNum>` (`前三平四` for two-file multi-pawn)
    let same_file = ordered_same_role_on_file(board, side, piece.role, mv.from.file());
    if same_file.len() < 2 {
        return false;
    }
    let Some(pos) = same_file.iter().position(|&sq| sq == mv.from) else {
        return false;
    };

    let order_ok = match (chars[0], same_file.len(), pos) {
        ('前', _, 0) => true,
        ('后', n, p) if p + 1 == n => true,
        ('中', 3, 1) | ('中', 5, 2) => true,
        ('二', n, 1) if n >= 4 => true,
        ('三', n, 2) if n >= 4 => true,
        ('四', 5, 3) => true,
        _ => false,
    };
    if !order_ok {
        return false;
    }

    if let Some(role) = parse_role_char_chinese(chars[1]) {
        role == piece.role
    } else if let Some(f_num) = parse_numeral_1_to_9(chars[1]) {
        piece.role == Role::Soldier && mover_file_number(side, mv.from.file()) == f_num
    } else {
        false
    }
}

/// Normalizes a sequence of move tokens against the board, preserving any valid prefix and
/// reporting an exact ply diagnostic without presenting a truncated parse as complete (Section 11).
pub fn replay_tokens(
    initial_fen: &str,
    dialect: NotationDialect,
    tokens: &[String],
) -> Result<ReplayResult, CoreError> {
    let mut state = GameState::from_fen(initial_fen, ADOPTED_RULES_PROFILE)?;
    let mut valid_moves = Vec::with_capacity(tokens.len());

    for (ply, tok) in tokens.iter().enumerate() {
        match parse_move_token(state.board(), tok, ply) {
            Ok(mv) => {
                state.apply_move(mv)?;
                valid_moves.push(mv);
            }
            Err(err) => {
                return Ok(ReplayResult {
                    parser_version: 1,
                    dialect,
                    initial_fen: initial_fen.to_string(),
                    valid_moves,
                    final_fen: state.to_fen(),
                    complete: false,
                    terminal_adjudication: None,
                    diagnostic: Some(ReplayDiagnostic {
                        ply,
                        token: tok.clone(),
                        message: err.to_string(),
                    }),
                });
            }
        }
    }

    Ok(ReplayResult {
        parser_version: 1,
        dialect,
        initial_fen: initial_fen.to_string(),
        valid_moves,
        final_fen: state.to_fen(),
        complete: true,
        terminal_adjudication: state.adjudicate(),
        diagnostic: None,
    })
}
