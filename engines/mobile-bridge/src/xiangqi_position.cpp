// SPDX-License-Identifier: Apache-2.0
#include "xiangqi_position.hpp"

#include <cctype>
#include <mutex>
#include <sstream>

namespace jade::xiangqi {
namespace {

struct ZobristTables {
  std::array<std::array<uint64_t, kNumSquares>, 15> piece_sq{};
  uint64_t black_to_move = 0;
};

uint64_t splitmix64(uint64_t& state) {
  uint64_t z = (state += 0x9e3779b97f4a7c15ULL);
  z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ULL;
  z = (z ^ (z >> 27)) * 0x94d049bb133111ebULL;
  return z ^ (z >> 31);
}

const ZobristTables& zobrist() {
  static ZobristTables tables;
  static std::once_flag init_flag;
  std::call_once(init_flag, []() {
    uint64_t seed = 0x4a41444558513031ULL;  // "JADEXQ01"
    for (size_t p = 1; p < tables.piece_sq.size(); ++p) {
      for (size_t sq = 0; sq < kNumSquares; ++sq) {
        tables.piece_sq[p][sq] = splitmix64(seed);
      }
    }
    tables.black_to_move = splitmix64(seed);
  });
  return tables;
}

constexpr bool in_palace(Color c, int file, int rank) {
  if (file < 3 || file > 5) return false;
  return c == Color::Red ? (rank >= 0 && rank <= 2) : (rank >= 7 && rank <= 9);
}

constexpr bool on_own_side_of_river(Color c, int rank) {
  return c == Color::Red ? (rank <= 4) : (rank >= 5);
}

constexpr bool crossed_river(Color c, int rank) {
  return c == Color::Red ? (rank >= 5) : (rank <= 4);
}

Piece char_to_piece(char ch) {
  switch (ch) {
    case 'K': return Piece::RedGeneral;
    case 'A': return Piece::RedAdvisor;
    case 'B':
    case 'E': return Piece::RedElephant;
    case 'N':
    case 'H': return Piece::RedHorse;
    case 'R': return Piece::RedChariot;
    case 'C': return Piece::RedCannon;
    case 'P': return Piece::RedSoldier;
    case 'k': return Piece::BlackGeneral;
    case 'a': return Piece::BlackAdvisor;
    case 'b':
    case 'e': return Piece::BlackElephant;
    case 'n':
    case 'h': return Piece::BlackHorse;
    case 'r': return Piece::BlackChariot;
    case 'c': return Piece::BlackCannon;
    case 'p': return Piece::BlackSoldier;
    default: return Piece::Empty;
  }
}

char piece_to_char(Piece p) {
  switch (p) {
    case Piece::RedGeneral: return 'K';
    case Piece::RedAdvisor: return 'A';
    case Piece::RedElephant: return 'B';
    case Piece::RedHorse: return 'N';
    case Piece::RedChariot: return 'R';
    case Piece::RedCannon: return 'C';
    case Piece::RedSoldier: return 'P';
    case Piece::BlackGeneral: return 'k';
    case Piece::BlackAdvisor: return 'a';
    case Piece::BlackElephant: return 'b';
    case Piece::BlackHorse: return 'n';
    case Piece::BlackChariot: return 'r';
    case Piece::BlackCannon: return 'c';
    case Piece::BlackSoldier: return 'p';
    case Piece::Empty: return '.';
  }
  return '.';
}

}  // namespace

std::string Move::to_uci() const {
  if (is_none()) return "0000";
  int f0 = file_of(from());
  int r0 = rank_of(from());
  int f1 = file_of(to());
  int r1 = rank_of(to());
  std::string out(4, '0');
  out[0] = static_cast<char>('a' + f0);
  out[1] = static_cast<char>('0' + r0);
  out[2] = static_cast<char>('a' + f1);
  out[3] = static_cast<char>('0' + r1);
  return out;
}

std::optional<Move> Move::from_uci(std::string_view uci) {
  if (uci.size() != 4) return std::nullopt;
  int f0 = uci[0] - 'a';
  int r0 = uci[1] - '0';
  int f1 = uci[2] - 'a';
  int r1 = uci[3] - '0';
  if (!on_board(f0, r0) || !on_board(f1, r1)) return std::nullopt;
  int from_sq = make_square(f0, r0);
  int to_sq = make_square(f1, r1);
  if (from_sq == to_sq) return std::nullopt;
  return Move(from_sq, to_sq);
}

Position::Position() { set_startpos(); }

void Position::set_startpos() { set_fen(kStartposFen, nullptr); }

bool Position::set_fen(std::string_view fen, std::string* err) {
  if (fen == "startpos") {
    fen = kStartposFen;
  }

  std::istringstream iss{std::string(fen)};
  std::string board_part;
  std::string side_part;
  if (!(iss >> board_part >> side_part)) {
    if (err) *err = "FEN must include piece placement and side to move";
    return false;
  }

  std::array<Piece, kNumSquares> next_board{};
  next_board.fill(Piece::Empty);
  std::array<int8_t, 2> next_generals{-1, -1};
  std::array<int, 2> general_count{0, 0};

  int rank = kNumRanks - 1;
  int file = 0;
  for (char ch : board_part) {
    if (ch == '/') {
      if (file != kNumFiles || rank <= 0) {
        if (err) *err = "invalid rank separator or file count in FEN";
        return false;
      }
      --rank;
      file = 0;
      continue;
    }
    if (ch >= '1' && ch <= '9') {
      file += (ch - '0');
      if (file > kNumFiles) {
        if (err) *err = "FEN rank exceeds 9 files";
        return false;
      }
      continue;
    }
    Piece p = char_to_piece(ch);
    if (p == Piece::Empty || file >= kNumFiles) {
      if (err) *err = "invalid piece character in FEN";
      return false;
    }
    int sq = make_square(file, rank);
    next_board[sq] = p;
    if (type_of(p) == PieceType::General) {
      Color c = color_of(p);
      if (!in_palace(c, file, rank)) {
        if (err) *err = "general outside palace in FEN";
        return false;
      }
      size_t idx = static_cast<size_t>(c);
      next_generals[idx] = static_cast<int8_t>(sq);
      ++general_count[idx];
    }
    ++file;
  }

  if (rank != 0 || file != kNumFiles) {
    if (err) *err = "FEN must specify all 10 ranks of 9 files";
    return false;
  }
  if (general_count[0] != 1 || general_count[1] != 1) {
    if (err) *err = "FEN must contain exactly one Red and one Black general";
    return false;
  }

  Color next_side = Color::None;
  if (side_part == "w" || side_part == "r" || side_part == "W" || side_part == "R") {
    next_side = Color::Red;
  } else if (side_part == "b" || side_part == "B") {
    next_side = Color::Black;
  } else {
    if (err) *err = "invalid side-to-move token in FEN";
    return false;
  }

  int next_halfmove = 0;
  int next_fullmove = 1;
  std::string tok3;
  std::string tok4;
  if (iss >> tok3) {
    if (tok3 != "-") {
      // Some minimal Xiangqi FENs place halfmove clock directly after side token.
      bool digits = !tok3.empty();
      for (char c : tok3) {
        if (!std::isdigit(static_cast<unsigned char>(c))) digits = false;
      }
      if (digits) {
        next_halfmove = std::stoi(tok3);
        int fm = 1;
        if (iss >> fm && fm >= 1) next_fullmove = fm;
      }
    } else if (iss >> tok4) {
      int hm = 0;
      int fm = 1;
      if (iss >> hm && hm >= 0) next_halfmove = hm;
      if (iss >> fm && fm >= 1) next_fullmove = fm;
    }
  }

  board_ = next_board;
  general_sq_ = next_generals;
  side_ = next_side;
  halfmove_clock_ = next_halfmove;
  fullmove_number_ = next_fullmove;
  recompute_key();

  // Side NOT to move cannot already have its general capturable (including flying general).
  if (in_check(opposite(side_))) {
    if (err) *err = "illegal FEN: non-moving side is already in check or flying general";
    return false;
  }

  key_history_.clear();
  check_history_.clear();
  key_history_.reserve(256);
  check_history_.reserve(256);
  return true;
}

std::string Position::to_fen() const {
  std::string out;
  out.reserve(96);
  for (int r = kNumRanks - 1; r >= 0; --r) {
    int empty_run = 0;
    for (int f = 0; f < kNumFiles; ++f) {
      Piece p = board_[make_square(f, r)];
      if (p == Piece::Empty) {
        ++empty_run;
      } else {
        if (empty_run > 0) {
          out.push_back(static_cast<char>('0' + empty_run));
          empty_run = 0;
        }
        out.push_back(piece_to_char(p));
      }
    }
    if (empty_run > 0) {
      out.push_back(static_cast<char>('0' + empty_run));
    }
    if (r > 0) out.push_back('/');
  }
  out += (side_ == Color::Red ? " w - - " : " b - - ");
  out += std::to_string(halfmove_clock_);
  out.push_back(' ');
  out += std::to_string(fullmove_number_);
  return out;
}

void Position::recompute_key() {
  const auto& z = zobrist();
  uint64_t k = 0;
  for (int sq = 0; sq < kNumSquares; ++sq) {
    Piece p = board_[sq];
    if (p != Piece::Empty) {
      k ^= z.piece_sq[static_cast<size_t>(p)][sq];
    }
  }
  if (side_ == Color::Black) {
    k ^= z.black_to_move;
  }
  key_ = k;
}

bool Position::is_square_attacked_by(int target_sq, Color attacker) const {
  const int tf = file_of(target_sq);
  const int tr = rank_of(target_sq);

  const Piece enemy_chariot = make_piece(attacker, PieceType::Chariot);
  const Piece enemy_cannon = make_piece(attacker, PieceType::Cannon);
  const Piece enemy_general = make_piece(attacker, PieceType::General);
  const Piece enemy_soldier = make_piece(attacker, PieceType::Soldier);
  const Piece enemy_horse = make_piece(attacker, PieceType::Horse);
  const Piece enemy_advisor = make_piece(attacker, PieceType::Advisor);
  const Piece enemy_elephant = make_piece(attacker, PieceType::Elephant);

  // 1. Orthogonal rays: Chariot, Cannon (1 screen), Flying/Adjacent General, Soldier.
  static constexpr int kOrthoDirs[4][2] = {{0, 1}, {0, -1}, {1, 0}, {-1, 0}};
  for (const auto& dir : kOrthoDirs) {
    int screens = 0;
    int step = 0;
    int f = tf + dir[0];
    int r = tr + dir[1];
    while (on_board(f, r)) {
      ++step;
      Piece p = board_[make_square(f, r)];
      if (p != Piece::Empty) {
        if (screens == 0) {
          if (p == enemy_chariot) return true;
          if (p == enemy_general) {
            if (step == 1 && in_palace(attacker, tf, tr)) return true;
            if (dir[0] == 0) return true;  // Flying general along open file.
          }
          if (step == 1 && p == enemy_soldier) {
            if (attacker == Color::Red) {
              if (dir[0] == 0 && dir[1] == -1) return true;  // Red soldier at (tf, tr - 1) attacks up.
              if (dir[1] == 0 && crossed_river(Color::Red, r)) return true;
            } else {
              if (dir[0] == 0 && dir[1] == 1) return true;   // Black soldier at (tf, tr + 1) attacks down.
              if (dir[1] == 0 && crossed_river(Color::Black, r)) return true;
            }
          }
          screens = 1;
        } else {
          if (p == enemy_cannon) return true;
          break;
        }
      }
      f += dir[0];
      r += dir[1];
    }
  }

  // 2. Horse attacks (blocking leg is orthogonally adjacent to the attacking horse).
  static constexpr int kHorseOffsets[8][2] = {
    {-2, -1}, {-2, 1}, {2, -1}, {2, 1},
    {-1, -2}, {-1, 2}, {1, -2}, {1, 2}
  };
  for (const auto& off : kHorseOffsets) {
    int hf = tf + off[0];
    int hr = tr + off[1];
    if (!on_board(hf, hr)) continue;
    if (board_[make_square(hf, hr)] != enemy_horse) continue;
    int leg_f = hf + (tf - hf) / 2;
    int leg_r = hr + (tr - hr) / 2;
    if (board_[make_square(leg_f, leg_r)] == Piece::Empty) {
      return true;
    }
  }

  // 3. Advisor & Elephant attacks (relevant for general square queries / ordering).
  if (in_palace(attacker, tf, tr)) {
    static constexpr int kDiag1[4][2] = {{-1, -1}, {-1, 1}, {1, -1}, {1, 1}};
    for (const auto& d : kDiag1) {
      int af = tf + d[0];
      int ar = tr + d[1];
      if (in_palace(attacker, af, ar) && board_[make_square(af, ar)] == enemy_advisor) {
        return true;
      }
    }
  }
  if (on_own_side_of_river(attacker, tr)) {
    static constexpr int kDiag2[4][2] = {{-2, -2}, {-2, 2}, {2, -2}, {2, 2}};
    for (const auto& d : kDiag2) {
      int ef = tf + d[0];
      int er = tr + d[1];
      if (!on_board(ef, er) || !on_own_side_of_river(attacker, er)) continue;
      if (board_[make_square(ef, er)] == enemy_elephant) {
        int eye_sq = make_square(tf + d[0] / 2, tr + d[1] / 2);
        if (board_[eye_sq] == Piece::Empty) return true;
      }
    }
  }

  return false;
}

bool Position::in_check(Color side) const {
  int gsq = general_sq_[static_cast<size_t>(side)];
  if (gsq < 0) return true;
  return is_square_attacked_by(gsq, opposite(side));
}

void Position::generate_pseudo_legal(MoveList& out, bool captures_only) const {
  out.count = 0;
  const Color us = side_;
  const Color them = opposite(us);

  auto try_push = [&](int from_sq, int to_sq) {
    Piece dst = board_[to_sq];
    if (color_of(dst) == us) return;
    if (captures_only && dst == Piece::Empty) return;
    out.push(Move(from_sq, to_sq));
  };

  for (int sq = 0; sq < kNumSquares; ++sq) {
    Piece p = board_[sq];
    if (color_of(p) != us) continue;
    const int f = file_of(sq);
    const int r = rank_of(sq);

    switch (type_of(p)) {
      case PieceType::General: {
        static constexpr int kOrtho[4][2] = {{0, 1}, {0, -1}, {1, 0}, {-1, 0}};
        for (const auto& d : kOrtho) {
          int nf = f + d[0];
          int nr = r + d[1];
          if (in_palace(us, nf, nr)) {
            try_push(sq, make_square(nf, nr));
          }
        }
        break;
      }
      case PieceType::Advisor: {
        static constexpr int kDiag[4][2] = {{-1, -1}, {-1, 1}, {1, -1}, {1, 1}};
        for (const auto& d : kDiag) {
          int nf = f + d[0];
          int nr = r + d[1];
          if (in_palace(us, nf, nr)) {
            try_push(sq, make_square(nf, nr));
          }
        }
        break;
      }
      case PieceType::Elephant: {
        static constexpr int kElephant[4][2] = {{-2, -2}, {-2, 2}, {2, -2}, {2, 2}};
        for (const auto& d : kElephant) {
          int nf = f + d[0];
          int nr = r + d[1];
          if (!on_board(nf, nr) || !on_own_side_of_river(us, nr)) continue;
          int eye_sq = make_square(f + d[0] / 2, r + d[1] / 2);
          if (board_[eye_sq] == Piece::Empty) {
            try_push(sq, make_square(nf, nr));
          }
        }
        break;
      }
      case PieceType::Horse: {
        static constexpr int kHorseSteps[4][3][2] = {
          {{0, 1}, {-1, 2}, {1, 2}},
          {{0, -1}, {-1, -2}, {1, -2}},
          {{1, 0}, {2, 1}, {2, -1}},
          {{-1, 0}, {-2, 1}, {-2, -1}},
        };
        for (const auto& group : kHorseSteps) {
          int lf = f + group[0][0];
          int lr = r + group[0][1];
          if (!on_board(lf, lr) || board_[make_square(lf, lr)] != Piece::Empty) continue;
          for (int k = 1; k <= 2; ++k) {
            int nf = f + group[k][0];
            int nr = r + group[k][1];
            if (on_board(nf, nr)) {
              try_push(sq, make_square(nf, nr));
            }
          }
        }
        break;
      }
      case PieceType::Chariot: {
        static constexpr int kOrtho[4][2] = {{0, 1}, {0, -1}, {1, 0}, {-1, 0}};
        for (const auto& d : kOrtho) {
          int nf = f + d[0];
          int nr = r + d[1];
          while (on_board(nf, nr)) {
            int to_sq = make_square(nf, nr);
            Piece dst = board_[to_sq];
            if (dst == Piece::Empty) {
              if (!captures_only) out.push(Move(sq, to_sq));
            } else {
              if (color_of(dst) == them) out.push(Move(sq, to_sq));
              break;
            }
            nf += d[0];
            nr += d[1];
          }
        }
        break;
      }
      case PieceType::Cannon: {
        static constexpr int kOrtho[4][2] = {{0, 1}, {0, -1}, {1, 0}, {-1, 0}};
        for (const auto& d : kOrtho) {
          int nf = f + d[0];
          int nr = r + d[1];
          bool jumped_screen = false;
          while (on_board(nf, nr)) {
            int to_sq = make_square(nf, nr);
            Piece dst = board_[to_sq];
            if (!jumped_screen) {
              if (dst == Piece::Empty) {
                if (!captures_only) out.push(Move(sq, to_sq));
              } else {
                jumped_screen = true;
              }
            } else if (dst != Piece::Empty) {
              if (color_of(dst) == them) {
                out.push(Move(sq, to_sq));
              }
              break;
            }
            nf += d[0];
            nr += d[1];
          }
        }
        break;
      }
      case PieceType::Soldier: {
        int fwd_r = r + (us == Color::Red ? 1 : -1);
        if (on_board(f, fwd_r)) {
          try_push(sq, make_square(f, fwd_r));
        }
        if (crossed_river(us, r)) {
          if (f > 0) try_push(sq, make_square(f - 1, r));
          if (f + 1 < kNumFiles) try_push(sq, make_square(f + 1, r));
        }
        break;
      }
      case PieceType::Empty:
        break;
    }
  }
}

void Position::make_move(Move m, UndoInfo& undo) {
  const auto& z = zobrist();
  const int from_sq = m.from();
  const int to_sq = m.to();
  const Piece moving = board_[from_sq];
  const Piece captured = board_[to_sq];
  const Color us = side_;

  undo.captured = captured;
  undo.prev_key = key_;
  undo.prev_halfmove_clock = halfmove_clock_;
  undo.prev_general_sq = general_sq_;
  undo.was_check = in_check(us);

  key_history_.push_back(key_);
  check_history_.push_back(undo.was_check);

  key_ ^= z.piece_sq[static_cast<size_t>(moving)][from_sq];
  if (captured != Piece::Empty) {
    key_ ^= z.piece_sq[static_cast<size_t>(captured)][to_sq];
    if (type_of(captured) == PieceType::General) {
      general_sq_[static_cast<size_t>(color_of(captured))] = -1;
    }
  }
  key_ ^= z.piece_sq[static_cast<size_t>(moving)][to_sq];
  key_ ^= z.black_to_move;

  board_[from_sq] = Piece::Empty;
  board_[to_sq] = moving;
  if (type_of(moving) == PieceType::General) {
    general_sq_[static_cast<size_t>(us)] = static_cast<int8_t>(to_sq);
  }

  if (captured != Piece::Empty) {
    halfmove_clock_ = 0;
  } else {
    ++halfmove_clock_;
  }
  if (us == Color::Black) {
    ++fullmove_number_;
  }
  side_ = opposite(us);
}

void Position::unmake_move(Move m, const UndoInfo& undo) {
  const int from_sq = m.from();
  const int to_sq = m.to();
  const Piece moving = board_[to_sq];

  if (side_ == Color::Red) {
    --fullmove_number_;
  }
  side_ = opposite(side_);
  board_[from_sq] = moving;
  board_[to_sq] = undo.captured;
  general_sq_ = undo.prev_general_sq;
  halfmove_clock_ = undo.prev_halfmove_clock;
  key_ = undo.prev_key;

  if (!key_history_.empty()) key_history_.pop_back();
  if (!check_history_.empty()) check_history_.pop_back();
}

void Position::generate_legal(MoveList& out, bool captures_only) const {
  MoveList pseudo;
  generate_pseudo_legal(pseudo, captures_only);
  out.count = 0;

  Position scratch = *this;
  const Color us = side_;
  for (int i = 0; i < pseudo.count; ++i) {
    UndoInfo undo;
    scratch.make_move(pseudo[i], undo);
    if (!scratch.in_check(us)) {
      out.push(pseudo[i]);
    }
    scratch.unmake_move(pseudo[i], undo);
  }
}

bool Position::is_legal_move(Move m) const {
  if (m.is_none()) return false;
  MoveList legal;
  generate_legal(legal, false);
  for (int i = 0; i < legal.count; ++i) {
    if (legal[i] == m) return true;
  }
  return false;
}

bool Position::apply_uci_move(std::string_view uci, std::string* err) {
  auto parsed = Move::from_uci(uci);
  if (!parsed.has_value()) {
    if (err) *err = "malformed UCI coordinate move: " + std::string(uci);
    return false;
  }
  if (!is_legal_move(*parsed)) {
    if (err) *err = "illegal Xiangqi move in position: " + std::string(uci);
    return false;
  }
  UndoInfo undo;
  make_move(*parsed, undo);
  return true;
}

bool Position::is_repetition_in_history(int /*search_ply*/) const {
  const int hist_size = static_cast<int>(key_history_.size());
  const int min_idx = std::max(0, hist_size - halfmove_clock_);
  for (int idx = hist_size - 2; idx >= min_idx; idx -= 2) {
    if (key_history_[idx] == key_) {
      return true;
    }
  }
  return false;
}

bool Position::is_perpetual_check_repetition(int /*search_ply*/) const {
  // Returns true if the side that JUST moved created a repeated position while
  // giving check continuously across the cycle (forbidden perpetual check).
  if (!in_check(side_)) return false;
  const int hist_size = static_cast<int>(key_history_.size());
  const int min_idx = std::max(0, hist_size - halfmove_clock_);
  for (int idx = hist_size - 2; idx >= min_idx; idx -= 2) {
    if (key_history_[idx] == key_) {
      bool all_checking = true;
      for (int k = idx; k < hist_size; k += 2) {
        if (k < static_cast<int>(check_history_.size()) && !check_history_[k]) {
          all_checking = false;
          break;
        }
      }
      return all_checking;
    }
  }
  return false;
}

std::vector<std::string> Position::sanitize_pv(const std::vector<Move>& raw_pv) const {
  std::vector<std::string> verified;
  verified.reserve(raw_pv.size());
  Position sim = *this;
  for (Move m : raw_pv) {
    if (!sim.is_legal_move(m)) {
      break;
    }
    verified.push_back(m.to_uci());
    UndoInfo undo;
    sim.make_move(m, undo);
  }
  return verified;
}

}  // namespace jade::xiangqi
