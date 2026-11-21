use crate::board::Board;
use crate::types::{
    ADOPTED_RULES_PROFILE, CoreError, Move, Piece, PieceId, PositionKey, Role, STARTING_FEN, Side,
    Square,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationLevel {
    /// `允许循环` (0) — never penalized with a loss.
    Allowed = 0,
    /// `长捉` (1) — perpetual chase of at least one opponent piece that escapes on every opponent turn.
    PerpetualChase = 1,
    /// `长将` (2) — perpetual check on every move by this side in the repetition cycle.
    PerpetualCheck = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameOutcome {
    RedWin,
    BlackWin,
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    Checkmate,
    NoLegalMovesLoss,
    PerpetualCheckLoss,
    PerpetualChaseLoss,
    MutualPerpetualCheckDraw,
    MutualPerpetualChaseDraw,
    AllowedRepetitionDraw,
    InsufficientMaterialDraw,
    NaturalLimit120Draw,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionDetails {
    pub red_violation: Option<ViolationLevel>,
    pub black_violation: Option<ViolationLevel>,
    pub red_chased_targets: Vec<Square>,
    pub black_chased_targets: Vec<Square>,
    pub effective_non_capture_plies: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adjudication {
    pub outcome: GameOutcome,
    pub reason: ReasonCode,
    pub move_interval: Option<(usize, usize)>,
    pub profile_version: String,
    pub details: DecisionDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryRecord {
    mv: Move,
    captured: Option<Piece>,
    board_after: Board,
    pos_key_after: PositionKey,
    gave_check: bool,
    effective_non_capture_plies_after: u32,
    red_checks_in_window_after: u32,
    black_checks_in_window_after: u32,
    was_excess_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    rules_profile: String,
    initial_board: Board,
    initial_pos_key: PositionKey,
    current_board: Board,
    history: Vec<HistoryRecord>,
    effective_non_capture_plies: u32,
    red_checks_in_window: u32,
    black_checks_in_window: u32,
    last_move_was_excess_check: bool,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new(ADOPTED_RULES_PROFILE).expect("default profile")
    }
}

impl GameState {
    pub fn new(rules_profile: &str) -> Result<Self, CoreError> {
        Self::from_fen(STARTING_FEN, rules_profile)
    }

    pub fn from_fen(fen: &str, rules_profile: &str) -> Result<Self, CoreError> {
        if rules_profile != ADOPTED_RULES_PROFILE {
            return Err(CoreError::UnsupportedRulesProfile(
                rules_profile.to_string(),
            ));
        }
        let board = Board::from_fen(fen)?;
        let pos_key = board.position_key();
        Ok(Self {
            rules_profile: rules_profile.to_string(),
            initial_board: board.clone(),
            initial_pos_key: pos_key,
            current_board: board,
            history: Vec::new(),
            effective_non_capture_plies: 0,
            red_checks_in_window: 0,
            black_checks_in_window: 0,
            last_move_was_excess_check: false,
        })
    }

    #[inline]
    pub fn rules_profile(&self) -> &str {
        &self.rules_profile
    }

    #[inline]
    pub const fn board(&self) -> &Board {
        &self.current_board
    }

    #[inline]
    pub const fn initial_board(&self) -> &Board {
        &self.initial_board
    }

    #[inline]
    pub const fn side_to_move(&self) -> Side {
        self.current_board.side_to_move()
    }

    #[inline]
    pub fn ply_count(&self) -> usize {
        self.history.len()
    }

    #[inline]
    pub const fn effective_non_capture_plies(&self) -> u32 {
        self.effective_non_capture_plies
    }

    pub fn last_move(&self) -> Option<Move> {
        self.history.last().map(|h| h.mv)
    }

    pub fn moves_played(&self) -> Vec<Move> {
        self.history.iter().map(|h| h.mv).collect()
    }

    pub fn to_fen(&self) -> String {
        let fullmove = (self.history.len() as u32 / 2) + 1;
        self.current_board
            .to_fen(self.effective_non_capture_plies, fullmove)
    }

    pub fn apply_move(&mut self, mv: Move) -> Result<Option<Adjudication>, CoreError> {
        let mover = self.current_board.side_to_move();
        let legal = self.current_board.legal_moves_for(mover);
        if !legal.contains(&mv) {
            return Err(CoreError::IllegalMove(mv.to_uci()));
        }

        let captured = self.current_board.make_move_unchecked(mv);
        let defender = mover.opposite();
        let gave_check = self.current_board.is_in_check(defender);

        let mut was_excess_check = false;
        if captured.is_some() {
            self.effective_non_capture_plies = 0;
            self.red_checks_in_window = 0;
            self.black_checks_in_window = 0;
        } else {
            if gave_check {
                let count = match mover {
                    Side::Red => {
                        self.red_checks_in_window += 1;
                        self.red_checks_in_window
                    }
                    Side::Black => {
                        self.black_checks_in_window += 1;
                        self.black_checks_in_window
                    }
                };
                if count > 10 {
                    was_excess_check = true;
                }
            }
            // Pikafish Chapter 1 Section 5 Clause 2:
            // Excess checks (>10 per side within a non-capture stretch) do not count toward the
            // 120-step limit, nor does the opponent's reply forced by an excess check.
            if !was_excess_check && !self.last_move_was_excess_check {
                self.effective_non_capture_plies += 1;
            }
        }
        self.last_move_was_excess_check = was_excess_check;

        let pos_key_after = self.current_board.position_key();
        self.history.push(HistoryRecord {
            mv,
            captured,
            board_after: self.current_board.clone(),
            pos_key_after,
            gave_check,
            effective_non_capture_plies_after: self.effective_non_capture_plies,
            red_checks_in_window_after: self.red_checks_in_window,
            black_checks_in_window_after: self.black_checks_in_window,
            was_excess_check,
        });

        Ok(self.adjudicate())
    }

    pub fn undo_move(&mut self) -> Result<Move, CoreError> {
        let Some(rec) = self.history.pop() else {
            return Err(CoreError::NothingToUndo);
        };
        self.current_board
            .unmake_move_unchecked(rec.mv, rec.captured);
        if let Some(prev) = self.history.last() {
            self.effective_non_capture_plies = prev.effective_non_capture_plies_after;
            self.red_checks_in_window = prev.red_checks_in_window_after;
            self.black_checks_in_window = prev.black_checks_in_window_after;
            self.last_move_was_excess_check = prev.was_excess_check;
        } else {
            self.effective_non_capture_plies = 0;
            self.red_checks_in_window = 0;
            self.black_checks_in_window = 0;
            self.last_move_was_excess_check = false;
        }
        Ok(rec.mv)
    }

    /// Evaluates terminal adjudication under `program-2023.11-v1` (Pikafish rev. 2023-11-22),
    /// honoring Chapter 1 Section 4 & Section 5 Clause 3 precedence:
    /// 1. Checkmate (`将死`) / No-legal-move loss (`困毙`)
    /// 2. Threefold position repetition (`三局面循环`)
    /// 3. Insufficient attacking material (`双方均只剩下将、士、象`)
    /// 4. 120-step check-adjusted natural limit (`自然限着`)
    pub fn adjudicate(&self) -> Option<Adjudication> {
        let stm = self.current_board.side_to_move();
        let legal_replies = self.current_board.legal_moves_for(stm);

        // 1. Checkmate or No-legal-move loss (`困毙`)
        if legal_replies.is_empty() {
            let winner = match stm.opposite() {
                Side::Red => GameOutcome::RedWin,
                Side::Black => GameOutcome::BlackWin,
            };
            let in_check = self.current_board.is_in_check(stm);
            let reason = if in_check {
                ReasonCode::Checkmate
            } else {
                ReasonCode::NoLegalMovesLoss
            };
            return Some(Adjudication {
                outcome: winner,
                reason,
                move_interval: self
                    .history
                    .len()
                    .checked_sub(1)
                    .map(|p| (p, self.history.len())),
                profile_version: self.rules_profile.clone(),
                details: DecisionDetails {
                    red_violation: None,
                    black_violation: None,
                    red_chased_targets: Vec::new(),
                    black_chased_targets: Vec::new(),
                    effective_non_capture_plies: self.effective_non_capture_plies,
                },
            });
        }

        // 2. Threefold position repetition (`三局面循环`)
        if let Some(rep) = self.evaluate_threefold_repetition() {
            return Some(rep);
        }

        // 3. Insufficient attacking material
        if !self.current_board.has_attacking_material() {
            return Some(Adjudication {
                outcome: GameOutcome::Draw,
                reason: ReasonCode::InsufficientMaterialDraw,
                move_interval: None,
                profile_version: self.rules_profile.clone(),
                details: DecisionDetails {
                    red_violation: None,
                    black_violation: None,
                    red_chased_targets: Vec::new(),
                    black_chased_targets: Vec::new(),
                    effective_non_capture_plies: self.effective_non_capture_plies,
                },
            });
        }

        // 4. 120-step natural limit (`自然限着`)
        if self.effective_non_capture_plies >= 120 {
            return Some(Adjudication {
                outcome: GameOutcome::Draw,
                reason: ReasonCode::NaturalLimit120Draw,
                move_interval: None,
                profile_version: self.rules_profile.clone(),
                details: DecisionDetails {
                    red_violation: None,
                    black_violation: None,
                    red_chased_targets: Vec::new(),
                    black_chased_targets: Vec::new(),
                    effective_non_capture_plies: self.effective_non_capture_plies,
                },
            });
        }

        None
    }

    fn state_at_index(&self, idx: usize) -> (&Board, &PositionKey) {
        if idx == 0 {
            (&self.initial_board, &self.initial_pos_key)
        } else {
            let rec = &self.history[idx - 1];
            (&rec.board_after, &rec.pos_key_after)
        }
    }

    fn evaluate_threefold_repetition(&self) -> Option<Adjudication> {
        let last_idx = self.history.len();
        if last_idx < 4 {
            return None;
        }
        let (_, target_key) = self.state_at_index(last_idx);
        let mut occurrences = Vec::with_capacity(3);
        for idx in 0..=last_idx {
            let (_, k) = self.state_at_index(idx);
            if k.sha256_hex == target_key.sha256_hex
                && k.canonical_bytes == target_key.canonical_bytes
            {
                occurrences.push(idx);
            }
        }
        if occurrences.len() < 3 {
            return None;
        }

        // Closed interval [idx_first ..= idx_last] from 1st occurrence to 3rd occurrence.
        let idx_first = occurrences[occurrences.len() - 3];
        let idx_last = last_idx;
        let cycle_len = idx_last - idx_first;

        // Collect boards S_0 ..= S_cycle_len and moves M_0 .. M_{cycle_len - 1}.
        let mut cycle_boards: Vec<&Board> = Vec::with_capacity(cycle_len + 1);
        for idx in idx_first..=idx_last {
            cycle_boards.push(self.state_at_index(idx).0);
        }

        let mut any_check_in_cycle = false;
        let mut red_all_checks = true;
        let mut black_all_checks = true;
        let mut red_move_count = 0usize;
        let mut black_move_count = 0usize;

        for (step, cycle_board) in cycle_boards.iter().enumerate().take(cycle_len) {
            let rec = &self.history[idx_first + step];
            let mover = cycle_board.side_to_move();
            if rec.gave_check {
                any_check_in_cycle = true;
            }
            match mover {
                Side::Red => {
                    red_move_count += 1;
                    if !rec.gave_check {
                        red_all_checks = false;
                    }
                }
                Side::Black => {
                    black_move_count += 1;
                    if !rec.gave_check {
                        black_all_checks = false;
                    }
                }
            }
        }

        let red_perpetual_check = red_move_count > 0 && red_all_checks;
        let black_perpetual_check = black_move_count > 0 && black_all_checks;

        let (red_chased_targets, black_chased_targets) = if any_check_in_cycle {
            // Pikafish Chapter 3 Section 1 Clause 5:
            // "在循环局面序列中不存在任何一步将军走法时..."
            // Whenever ANY move in the repetition sequence is a check, neither side has any chase!
            (Vec::new(), Vec::new())
        } else {
            let red_targets = find_perpetually_chased_targets(
                &cycle_boards,
                &self.history[idx_first..idx_last],
                Side::Red,
            );
            let black_targets = find_perpetually_chased_targets(
                &cycle_boards,
                &self.history[idx_first..idx_last],
                Side::Black,
            );
            (red_targets, black_targets)
        };

        let red_violation = if red_perpetual_check {
            ViolationLevel::PerpetualCheck
        } else if !red_chased_targets.is_empty() {
            ViolationLevel::PerpetualChase
        } else {
            ViolationLevel::Allowed
        };

        let black_violation = if black_perpetual_check {
            ViolationLevel::PerpetualCheck
        } else if !black_chased_targets.is_empty() {
            ViolationLevel::PerpetualChase
        } else {
            ViolationLevel::Allowed
        };

        let (outcome, reason) = match red_violation.cmp(&black_violation) {
            std::cmp::Ordering::Greater => {
                let r = match red_violation {
                    ViolationLevel::PerpetualCheck => ReasonCode::PerpetualCheckLoss,
                    ViolationLevel::PerpetualChase => ReasonCode::PerpetualChaseLoss,
                    ViolationLevel::Allowed => unreachable!(),
                };
                (GameOutcome::BlackWin, r)
            }
            std::cmp::Ordering::Less => {
                let r = match black_violation {
                    ViolationLevel::PerpetualCheck => ReasonCode::PerpetualCheckLoss,
                    ViolationLevel::PerpetualChase => ReasonCode::PerpetualChaseLoss,
                    ViolationLevel::Allowed => unreachable!(),
                };
                (GameOutcome::RedWin, r)
            }
            std::cmp::Ordering::Equal => {
                let r = match red_violation {
                    ViolationLevel::PerpetualCheck => ReasonCode::MutualPerpetualCheckDraw,
                    ViolationLevel::PerpetualChase => ReasonCode::MutualPerpetualChaseDraw,
                    ViolationLevel::Allowed => ReasonCode::AllowedRepetitionDraw,
                };
                (GameOutcome::Draw, r)
            }
        };

        Some(Adjudication {
            outcome,
            reason,
            move_interval: Some((idx_first, idx_last)),
            profile_version: self.rules_profile.clone(),
            details: DecisionDetails {
                red_violation: Some(red_violation),
                black_violation: Some(black_violation),
                red_chased_targets,
                black_chased_targets,
                effective_non_capture_plies: self.effective_non_capture_plies,
            },
        })
    }
}

/// Computes the set of `PieceId`s belonging to `chaser_side.opposite()` that are chased (`捉`)
/// by `chaser_side` on `board` according to Pikafish Chapter 3 Section 1 Clause 5.
pub fn chased_piece_ids_on_board(board: &Board, chaser_side: Side) -> HashSet<PieceId> {
    let defender_side = chaser_side.opposite();
    let defender_captures = board.legal_captures_for(defender_side);
    let mut chased = HashSet::new();

    for (mv, att, victim) in board.legal_captures_for(chaser_side) {
        // Check is handled separately; Generals are not chase targets.
        if victim.role == Role::General {
            continue;
        }
        // Exclusion 5.b: Attacker is General or Soldier.
        if matches!(att.role, Role::General | Role::Soldier) {
            continue;
        }
        // Exclusion 5.c: Victim is an uncrossed Soldier.
        if victim.role == Role::Soldier && !mv.to.has_crossed_river(defender_side) {
            continue;
        }
        // Exclusion 5.d: Same role AND attacker is currently in victim's legal attack on `board`.
        if att.role == victim.role {
            let mutual = defender_captures
                .iter()
                .any(|(dmv, _, _)| dmv.from == mv.to && dmv.to == mv.from);
            if mutual {
                continue;
            }
        }
        // Exclusion 5.a: Simulate capture; check if `att` on `mv.to` is under legal attack by `defender_side`,
        // EXCEPT when (Horse|Cannon -> Chariot) or (Advisor|Elephant -> Chariot|Horse|Cannon).
        let exempt_from_recapture = (matches!(att.role, Role::Horse | Role::Cannon)
            && victim.role == Role::Chariot)
            || (matches!(att.role, Role::Advisor | Role::Elephant)
                && matches!(victim.role, Role::Chariot | Role::Horse | Role::Cannon));

        if !exempt_from_recapture {
            let mut sim = board.clone();
            sim = sim.with_side_to_move(chaser_side);
            sim.make_move_unchecked(mv);
            // `sim` now has `side_to_move == defender_side`.
            let recapturable = sim
                .legal_captures_for(defender_side)
                .iter()
                .any(|(rmv, _, _)| rmv.to == mv.to);
            if recapturable {
                continue;
            }
        }

        chased.insert(victim.id);
    }

    chased
}

fn find_perpetually_chased_targets(
    cycle_boards: &[&Board],
    cycle_records: &[HistoryRecord],
    chaser_side: Side,
) -> Vec<Square> {
    let defender_side = chaser_side.opposite();
    let k = cycle_records.len();
    if k == 0 {
        return Vec::new();
    }

    // Precompute chased PieceIds on every board S_0 ..= S_k.
    // Note: in S_0 and S_k, placements are identical; if identical pieces did not swap,
    // PieceIds match directly. To handle even swapped identical pieces cleanly across head-to-tail
    // wrap-around, we also check square-based chased sets!
    let chased_sets: Vec<HashSet<PieceId>> = cycle_boards
        .iter()
        .map(|b| chased_piece_ids_on_board(b, chaser_side))
        .collect();

    let mut targets = Vec::new();
    for idx in 0..90u8 {
        let initial_sq = Square::from_index(idx).unwrap();
        let Some(piece) = cycle_boards[0].piece_at(initial_sq) else {
            continue;
        };
        if piece.side != defender_side || piece.role == Role::General {
            continue;
        }
        let pid = piece.id;

        // Head-to-tail requirement (Clause 10 & Section 2 Clause 3):
        // - After EVERY move by `chaser_side` (step `i` where `S_i.side_to_move == chaser_side`),
        //   `pid` MUST be chased in `S_{i+1}`.
        // - On EVERY move by `defender_side` (step `i` where `S_i.side_to_move == defender_side`),
        //   `pid` MUST escape chase (`逃捉`): chased in `S_i` (with wrap `S_0` matching `S_k` via `pid`'s
        //   square in `S_0` if `i == 0`) AND NOT chased in `S_{i+1}`!
        let mut all_chaser_moves_chase = true;
        let mut all_defender_moves_escape = true;
        let mut chaser_moves = 0usize;
        let mut defender_moves = 0usize;

        for step in 0..k {
            let mover = cycle_boards[step].side_to_move();
            if mover == chaser_side {
                chaser_moves += 1;
                if !chased_sets[step + 1].contains(&pid) {
                    all_chaser_moves_chase = false;
                    break;
                }
            } else {
                defender_moves += 1;
                // Before defender's move at step == 0, head-to-tail wrap means `S_0` was formed by `M_{k-1}` (`S_k`).
                // Check `chased_sets[step]` on `pid` (which at `step == 0` is `S_0`'s `pid`, and if identical pieces
                // swapped between `S_0` and `S_k`, `chased_piece_ids_on_board` depends only on placement so `pid` at
                // `initial_sq` in `S_0` has the exact same chased status as whatever identical piece landed on `initial_sq` in `S_k`!).
                let was_chased_before = chased_sets[step].contains(&pid);
                let is_chased_after = chased_sets[step + 1].contains(&pid);
                if !was_chased_before || is_chased_after {
                    all_defender_moves_escape = false;
                    break;
                }
            }
        }

        if chaser_moves > 0
            && defender_moves > 0
            && all_chaser_moves_chase
            && all_defender_moves_escape
        {
            targets.push(initial_sq);
        }
    }

    targets
}
