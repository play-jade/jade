use crate::types::{
    CoreError, Move, Piece, PieceId, PositionKey, Role, STARTING_FEN, Side, Square,
};
use serde::{Deserialize, Serialize};

mod squares_serde {
    use crate::types::Piece;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        squares: &[Option<Piece>; 90],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        squares.as_slice().serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<[Option<Piece>; 90], D::Error> {
        let vec = Vec::<Option<Piece>>::deserialize(deserializer)?;
        vec.try_into()
            .map_err(|v: Vec<Option<Piece>>| serde::de::Error::invalid_length(v.len(), &"90"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Board {
    #[serde(with = "squares_serde")]
    squares: [Option<Piece>; 90],
    side_to_move: Side,
    next_piece_id: u8,
}

impl Default for Board {
    fn default() -> Self {
        Self::from_fen(STARTING_FEN).expect("valid starting FEN")
    }
}

impl Board {
    pub fn empty(side_to_move: Side) -> Self {
        Self {
            squares: [None; 90],
            side_to_move,
            next_piece_id: 0,
        }
    }

    pub fn from_fen(fen: &str) -> Result<Self, CoreError> {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(CoreError::InvalidFen(fen.to_string()));
        }
        let rows: Vec<&str> = parts[0].split('/').collect();
        if rows.len() != 10 {
            return Err(CoreError::InvalidFen(format!(
                "expected 10 ranks in FEN, got {}",
                rows.len()
            )));
        }
        let side_to_move = match parts[1] {
            "w" | "r" | "W" | "R" => Side::Red,
            "b" | "B" => Side::Black,
            other => {
                return Err(CoreError::InvalidFen(format!(
                    "invalid side to move '{other}'"
                )));
            }
        };

        let mut board = Self::empty(side_to_move);
        for (row_idx, row_str) in rows.iter().enumerate() {
            let rank = 9u8 - (row_idx as u8);
            let mut file = 0u8;
            for ch in row_str.chars() {
                if let Some(digit) = ch.to_digit(10) {
                    if digit == 0 || file + (digit as u8) > 9 {
                        return Err(CoreError::InvalidFen(format!(
                            "invalid empty count '{ch}' in rank {rank}"
                        )));
                    }
                    file += digit as u8;
                } else if let Some((side, role)) = Role::from_fen_char(ch) {
                    if file >= 9 {
                        return Err(CoreError::InvalidFen(format!(
                            "rank {rank} exceeds 9 files"
                        )));
                    }
                    let sq = Square::from_file_rank(file, rank).unwrap();
                    let id = PieceId(board.next_piece_id);
                    board.next_piece_id = board.next_piece_id.saturating_add(1);
                    board.squares[sq.index()] = Some(Piece { id, side, role });
                    file += 1;
                } else {
                    return Err(CoreError::InvalidFen(format!(
                        "unexpected character '{ch}'"
                    )));
                }
            }
            if file != 9 {
                return Err(CoreError::InvalidFen(format!(
                    "rank {rank} has {file} files instead of 9"
                )));
            }
        }
        Ok(board)
    }

    pub fn to_fen(&self, halfmove_clock: u32, fullmove_number: u32) -> String {
        let mut placement = String::with_capacity(96);
        for row_idx in 0..10u8 {
            let rank = 9 - row_idx;
            let mut empty_run = 0u8;
            for file in 0..9u8 {
                let sq = Square::from_file_rank(file, rank).unwrap();
                match self.squares[sq.index()] {
                    None => empty_run += 1,
                    Some(p) => {
                        if empty_run > 0 {
                            placement.push((b'0' + empty_run) as char);
                            empty_run = 0;
                        }
                        placement.push(p.role.fen_char(p.side));
                    }
                }
            }
            if empty_run > 0 {
                placement.push((b'0' + empty_run) as char);
            }
            if row_idx < 9 {
                placement.push('/');
            }
        }
        let stm = match self.side_to_move {
            Side::Red => 'w',
            Side::Black => 'b',
        };
        format!("{placement} {stm} - - {halfmove_clock} {fullmove_number}")
    }

    #[inline]
    pub const fn side_to_move(&self) -> Side {
        self.side_to_move
    }

    #[inline]
    pub fn with_side_to_move(&self, side: Side) -> Self {
        let mut copy = self.clone();
        copy.side_to_move = side;
        copy
    }

    #[inline]
    pub const fn squares(&self) -> &[Option<Piece>; 90] {
        &self.squares
    }

    #[inline]
    pub fn piece_at(&self, sq: Square) -> Option<Piece> {
        self.squares[sq.index()]
    }

    pub fn find_piece_by_id(&self, id: PieceId) -> Option<(Square, Piece)> {
        for idx in 0..90u8 {
            let sq = Square::from_index(idx).unwrap();
            if let Some(p) = self.squares[sq.index()]
                && p.id == id
            {
                return Some((sq, p));
            }
        }
        None
    }

    pub fn general_square(&self, side: Side) -> Option<Square> {
        for idx in 0..90u8 {
            let sq = Square::from_index(idx).unwrap();
            if let Some(p) = self.squares[sq.index()]
                && p.side == side
                && p.role == Role::General
            {
                return Some(sq);
            }
        }
        None
    }

    pub fn position_key(&self) -> PositionKey {
        PositionKey::compute_v1(&self.squares, self.side_to_move)
    }

    pub fn mirror_files(&self) -> Self {
        let mut mirrored = [None; 90];
        for idx in 0..90u8 {
            let sq = Square::from_index(idx).unwrap();
            if let Some(p) = self.squares[sq.index()] {
                mirrored[sq.mirror_file().index()] = Some(p);
            }
        }
        Self {
            squares: mirrored,
            side_to_move: self.side_to_move,
            next_piece_id: self.next_piece_id,
        }
    }

    pub fn has_attacking_material(&self) -> bool {
        self.squares.iter().flatten().any(|p| {
            matches!(
                p.role,
                Role::Chariot | Role::Horse | Role::Cannon | Role::Soldier
            )
        })
    }

    /// Unchecked board update returning any captured piece.
    pub fn make_move_unchecked(&mut self, mv: Move) -> Option<Piece> {
        let moving = self.squares[mv.from.index()].take();
        let captured = self.squares[mv.to.index()].take();
        self.squares[mv.to.index()] = moving;
        self.side_to_move = self.side_to_move.opposite();
        captured
    }

    pub fn unmake_move_unchecked(&mut self, mv: Move, captured: Option<Piece>) {
        self.side_to_move = self.side_to_move.opposite();
        let moving = self.squares[mv.to.index()].take();
        self.squares[mv.from.index()] = moving;
        self.squares[mv.to.index()] = captured;
    }

    /// Returns true if the two Generals face each other on the same file with zero pieces between.
    pub fn generals_facing(&self) -> bool {
        let (Some(red_k), Some(blk_k)) = (
            self.general_square(Side::Red),
            self.general_square(Side::Black),
        ) else {
            return false;
        };
        if red_k.file() != blk_k.file() {
            return false;
        }
        let f = red_k.file();
        let low = red_k.rank().min(blk_k.rank());
        let high = red_k.rank().max(blk_k.rank());
        for r in (low + 1)..high {
            let sq = Square::from_file_rank(f, r).unwrap();
            if self.squares[sq.index()].is_some() {
                return false;
            }
        }
        true
    }

    /// Returns true if `target_sq` is pseudo-legally attacked by any piece of `by_side`.
    pub fn is_square_attacked(&self, target_sq: Square, by_side: Side) -> bool {
        let tf = target_sq.file() as i8;
        let tr = target_sq.rank() as i8;

        // 1. Orthogonal rays: Chariot, General (distance 1 in palace / flying), Cannon (1 screen), Soldier.
        const ORTHO: [(i8, i8); 4] = [(0, 1), (0, -1), (1, 0), (-1, 0)];
        for (df, dr) in ORTHO {
            let mut f = tf + df;
            let mut r = tr + dr;
            let mut screens = 0u8;
            let mut dist = 1u8;
            while (0..9).contains(&f) && (0..10).contains(&r) {
                let sq = Square::from_file_rank(f as u8, r as u8).unwrap();
                if let Some(p) = self.squares[sq.index()] {
                    if screens == 0 {
                        if p.side == by_side {
                            if p.role == Role::Chariot {
                                return true;
                            }
                            if dist == 1
                                && p.role == Role::General
                                && target_sq.is_in_palace(by_side)
                            {
                                return true;
                            }
                            // Flying general attack along file
                            if df == 0
                                && p.role == Role::General
                                && target_sq.is_in_palace(by_side.opposite())
                            {
                                // Only applies when checking general safety; handled via generals_facing() too,
                                // and kept consistent for direct general attack checks.
                                return true;
                            }
                            if dist == 1
                                && p.role == Role::Soldier
                                && Self::soldier_can_step(by_side, sq, target_sq)
                            {
                                return true;
                            }
                        }
                        screens = 1;
                    } else {
                        if p.side == by_side && p.role == Role::Cannon {
                            return true;
                        }
                        break;
                    }
                }
                f += df;
                r += dr;
                dist += 1;
            }
        }

        // 2. Horse attacks: check all 8 candidate horse origins and verify their horse leg!
        const HORSE_FROM: [(i8, i8, i8, i8); 8] = [
            (-1, -2, 0, 1),
            (1, -2, 0, 1),
            (-1, 2, 0, -1),
            (1, 2, 0, -1),
            (-2, -1, 1, 0),
            (-2, 1, 1, 0),
            (2, -1, -1, 0),
            (2, 1, -1, 0),
        ];
        for (ox, oy, leg_dx, leg_dy) in HORSE_FROM {
            let hf = tf + ox;
            let hr = tr + oy;
            if (0..9).contains(&hf) && (0..10).contains(&hr) {
                let h_sq = Square::from_file_rank(hf as u8, hr as u8).unwrap();
                if let Some(p) = self.squares[h_sq.index()]
                    && p.side == by_side
                    && p.role == Role::Horse
                {
                    let leg_sq =
                        Square::from_file_rank((hf + leg_dx) as u8, (hr + leg_dy) as u8).unwrap();
                    if self.squares[leg_sq.index()].is_none() {
                        return true;
                    }
                }
            }
        }

        // 3. Advisor diagonal attacks inside palace
        if target_sq.is_in_palace(by_side) {
            for (df, dr) in [(-1i8, -1i8), (-1, 1), (1, -1), (1, 1)] {
                let af = tf + df;
                let ar = tr + dr;
                if (0..9).contains(&af) && (0..10).contains(&ar) {
                    let a_sq = Square::from_file_rank(af as u8, ar as u8).unwrap();
                    if a_sq.is_in_palace(by_side)
                        && let Some(p) = self.squares[a_sq.index()]
                        && p.side == by_side
                        && p.role == Role::Advisor
                    {
                        return true;
                    }
                }
            }
        }

        // 4. Elephant diagonal-2 attacks on own side of river with clear elephant eye
        let on_attacker_half = match by_side {
            Side::Red => tr <= 4,
            Side::Black => tr >= 5,
        };
        if on_attacker_half {
            for (df, dr) in [(-2i8, -2i8), (-2, 2), (2, -2), (2, 2)] {
                let ef = tf + df;
                let er = tr + dr;
                if (0..9).contains(&ef) && (0..10).contains(&er) {
                    let same_half = match by_side {
                        Side::Red => er <= 4,
                        Side::Black => er >= 5,
                    };
                    if same_half {
                        let e_sq = Square::from_file_rank(ef as u8, er as u8).unwrap();
                        if let Some(p) = self.squares[e_sq.index()]
                            && p.side == by_side
                            && p.role == Role::Elephant
                        {
                            let eye =
                                Square::from_file_rank((tf + df / 2) as u8, (tr + dr / 2) as u8)
                                    .unwrap();
                            if self.squares[eye.index()].is_none() {
                                return true;
                            }
                        }
                    }
                }
            }
        }

        false
    }

    #[inline]
    fn soldier_can_step(side: Side, from: Square, to: Square) -> bool {
        let df = (to.file() as i8) - (from.file() as i8);
        let dr = (to.rank() as i8) - (from.rank() as i8);
        match side {
            Side::Red => {
                if dr == 1 && df == 0 {
                    true
                } else {
                    from.has_crossed_river(Side::Red) && dr == 0 && df.abs() == 1
                }
            }
            Side::Black => {
                if dr == -1 && df == 0 {
                    true
                } else {
                    from.has_crossed_river(Side::Black) && dr == 0 && df.abs() == 1
                }
            }
        }
    }

    pub fn is_in_check(&self, side: Side) -> bool {
        if self.generals_facing() {
            return true;
        }
        let Some(gen_sq) = self.general_square(side) else {
            return false;
        };
        self.is_square_attacked(gen_sq, side.opposite())
    }

    pub fn generate_pseudo_legal_moves_for(&self, side: Side, out: &mut Vec<Move>) {
        out.clear();
        for idx in 0..90u8 {
            let from = Square::from_index(idx).unwrap();
            let Some(piece) = self.squares[from.index()] else {
                continue;
            };
            if piece.side != side {
                continue;
            }
            let f = from.file() as i8;
            let r = from.rank() as i8;

            let mut push_if_not_friendly = |tf: i8, tr: i8| {
                if (0..9).contains(&tf) && (0..10).contains(&tr) {
                    let to = Square::from_file_rank(tf as u8, tr as u8).unwrap();
                    if self.squares[to.index()].is_none_or(|dst| dst.side != side) {
                        out.push(Move::new(from, to));
                    }
                }
            };

            match piece.role {
                Role::General => {
                    for (df, dr) in [(0i8, 1i8), (0, -1), (1, 0), (-1, 0)] {
                        let tf = f + df;
                        let tr = r + dr;
                        if (0..9).contains(&tf) && (0..10).contains(&tr) {
                            let to = Square::from_file_rank(tf as u8, tr as u8).unwrap();
                            if to.is_in_palace(side)
                                && self.squares[to.index()].is_none_or(|d| d.side != side)
                            {
                                out.push(Move::new(from, to));
                            }
                        }
                    }
                }
                Role::Advisor => {
                    for (df, dr) in [(-1i8, -1i8), (-1, 1), (1, -1), (1, 1)] {
                        let tf = f + df;
                        let tr = r + dr;
                        if (0..9).contains(&tf) && (0..10).contains(&tr) {
                            let to = Square::from_file_rank(tf as u8, tr as u8).unwrap();
                            if to.is_in_palace(side)
                                && self.squares[to.index()].is_none_or(|d| d.side != side)
                            {
                                out.push(Move::new(from, to));
                            }
                        }
                    }
                }
                Role::Elephant => {
                    for (df, dr) in [(-2i8, -2i8), (-2, 2), (2, -2), (2, 2)] {
                        let tf = f + df;
                        let tr = r + dr;
                        if (0..9).contains(&tf) && (0..10).contains(&tr) {
                            let ok_half = match side {
                                Side::Red => tr <= 4,
                                Side::Black => tr >= 5,
                            };
                            if ok_half {
                                let eye =
                                    Square::from_file_rank((f + df / 2) as u8, (r + dr / 2) as u8)
                                        .unwrap();
                                if self.squares[eye.index()].is_none() {
                                    push_if_not_friendly(tf, tr);
                                }
                            }
                        }
                    }
                }
                Role::Horse => {
                    const STEPS: [(i8, i8, i8, i8); 8] = [
                        (0, 1, -1, 2),
                        (0, 1, 1, 2),
                        (0, -1, -1, -2),
                        (0, -1, 1, -2),
                        (-1, 0, -2, -1),
                        (-1, 0, -2, 1),
                        (1, 0, 2, -1),
                        (1, 0, 2, 1),
                    ];
                    for (lx, ly, dx, dy) in STEPS {
                        let lf = f + lx;
                        let lr = r + ly;
                        let tf = f + dx;
                        let tr = r + dy;
                        if (0..9).contains(&tf) && (0..10).contains(&tr) {
                            let leg = Square::from_file_rank(lf as u8, lr as u8).unwrap();
                            if self.squares[leg.index()].is_none() {
                                push_if_not_friendly(tf, tr);
                            }
                        }
                    }
                }
                Role::Chariot => {
                    for (df, dr) in [(0i8, 1i8), (0, -1), (1, 0), (-1, 0)] {
                        let mut tf = f + df;
                        let mut tr = r + dr;
                        while (0..9).contains(&tf) && (0..10).contains(&tr) {
                            let to = Square::from_file_rank(tf as u8, tr as u8).unwrap();
                            match self.squares[to.index()] {
                                None => out.push(Move::new(from, to)),
                                Some(dst) => {
                                    if dst.side != side {
                                        out.push(Move::new(from, to));
                                    }
                                    break;
                                }
                            }
                            tf += df;
                            tr += dr;
                        }
                    }
                }
                Role::Cannon => {
                    for (df, dr) in [(0i8, 1i8), (0, -1), (1, 0), (-1, 0)] {
                        let mut tf = f + df;
                        let mut tr = r + dr;
                        let mut jumped = false;
                        while (0..9).contains(&tf) && (0..10).contains(&tr) {
                            let to = Square::from_file_rank(tf as u8, tr as u8).unwrap();
                            match self.squares[to.index()] {
                                None => {
                                    if !jumped {
                                        out.push(Move::new(from, to));
                                    }
                                }
                                Some(dst) => {
                                    if !jumped {
                                        jumped = true;
                                    } else {
                                        if dst.side != side {
                                            out.push(Move::new(from, to));
                                        }
                                        break;
                                    }
                                }
                            }
                            tf += df;
                            tr += dr;
                        }
                    }
                }
                Role::Soldier => {
                    let forward = match side {
                        Side::Red => 1i8,
                        Side::Black => -1i8,
                    };
                    push_if_not_friendly(f, r + forward);
                    if from.has_crossed_river(side) {
                        push_if_not_friendly(f - 1, r);
                        push_if_not_friendly(f + 1, r);
                    }
                }
            }
        }
    }

    pub fn legal_moves_for(&self, side: Side) -> Vec<Move> {
        let mut pseudo = Vec::with_capacity(64);
        self.generate_pseudo_legal_moves_for(side, &mut pseudo);
        let mut scratch = self.clone();
        scratch.side_to_move = side;
        let mut legal = Vec::with_capacity(pseudo.len());
        for mv in pseudo {
            let captured = scratch.make_move_unchecked(mv);
            if !scratch.is_in_check(side) {
                legal.push(mv);
            }
            scratch.unmake_move_unchecked(mv, captured);
        }
        legal
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        self.legal_moves_for(self.side_to_move)
    }

    /// Legal capture moves available to `attacker_side` if given an immediate turn on `self`
    /// (exact `攻击` definition in Pikafish Chapter 3 Section 1 Clause 3).
    pub fn legal_captures_for(&self, attacker_side: Side) -> Vec<(Move, Piece, Piece)> {
        let legal = self.legal_moves_for(attacker_side);
        let mut out = Vec::new();
        for mv in legal {
            if let (Some(att), Some(victim)) =
                (self.squares[mv.from.index()], self.squares[mv.to.index()])
                && att.side == attacker_side
                && victim.side == attacker_side.opposite()
            {
                out.push((mv, att, victim));
            }
        }
        out
    }
}
