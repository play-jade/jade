// SPDX-License-Identifier: Apache-2.0
#ifndef JADE_ENGINE_XIANGQI_POSITION_HPP_
#define JADE_ENGINE_XIANGQI_POSITION_HPP_

#include <array>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace jade::xiangqi {

inline constexpr int kNumFiles = 9;
inline constexpr int kNumRanks = 10;
inline constexpr int kNumSquares = kNumFiles * kNumRanks;  // 90

inline constexpr std::string_view kStartposFen =
    "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w - - 0 1";

enum class Color : uint8_t {
  Red = 0,
  Black = 1,
  None = 2,
};

constexpr Color opposite(Color c) {
  return c == Color::Red ? Color::Black : (c == Color::Black ? Color::Red : Color::None);
}

enum class PieceType : uint8_t {
  Empty = 0,
  General = 1,
  Advisor = 2,
  Elephant = 3,
  Horse = 4,
  Chariot = 5,
  Cannon = 6,
  Soldier = 7,
};

enum class Piece : uint8_t {
  Empty = 0,
  RedGeneral = 1,
  RedAdvisor = 2,
  RedElephant = 3,
  RedHorse = 4,
  RedChariot = 5,
  RedCannon = 6,
  RedSoldier = 7,
  BlackGeneral = 8,
  BlackAdvisor = 9,
  BlackElephant = 10,
  BlackHorse = 11,
  BlackChariot = 12,
  BlackCannon = 13,
  BlackSoldier = 14,
};

constexpr int make_square(int file, int rank) { return rank * kNumFiles + file; }
constexpr int file_of(int sq) { return sq % kNumFiles; }
constexpr int rank_of(int sq) { return sq / kNumFiles; }
constexpr bool on_board(int file, int rank) {
  return file >= 0 && file < kNumFiles && rank >= 0 && rank < kNumRanks;
}

constexpr Piece make_piece(Color c, PieceType pt) {
  if (pt == PieceType::Empty || c == Color::None) return Piece::Empty;
  return static_cast<Piece>((c == Color::Red ? 0 : 7) + static_cast<uint8_t>(pt));
}

constexpr Color color_of(Piece p) {
  uint8_t v = static_cast<uint8_t>(p);
  if (v == 0) return Color::None;
  return v <= 7 ? Color::Red : Color::Black;
}

constexpr PieceType type_of(Piece p) {
  uint8_t v = static_cast<uint8_t>(p);
  if (v == 0) return PieceType::Empty;
  return static_cast<PieceType>(v <= 7 ? v : v - 7);
}

struct Move {
  uint16_t raw = 0;

  constexpr Move() = default;
  constexpr explicit Move(uint16_t r) : raw(r) {}
  constexpr Move(int from_sq, int to_sq)
      : raw(static_cast<uint16_t>((from_sq & 0x7f) | ((to_sq & 0x7f) << 7))) {}

  constexpr bool is_none() const { return raw == 0; }
  constexpr int from() const { return raw & 0x7f; }
  constexpr int to() const { return (raw >> 7) & 0x7f; }

  constexpr bool operator==(const Move& other) const { return raw == other.raw; }
  constexpr bool operator!=(const Move& other) const { return raw != other.raw; }

  std::string to_uci() const;
  static std::optional<Move> from_uci(std::string_view uci);
};

struct MoveList {
  std::array<Move, 128> moves{};
  int count = 0;

  void push(Move m) {
    if (count < static_cast<int>(moves.size())) {
      moves[count++] = m;
    }
  }

  Move* begin() { return moves.data(); }
  Move* end() { return moves.data() + count; }
  const Move* begin() const { return moves.data(); }
  const Move* end() const { return moves.data() + count; }
  int size() const { return count; }
  bool empty() const { return count == 0; }
  Move operator[](int idx) const { return moves[idx]; }
  Move& operator[](int idx) { return moves[idx]; }
};

struct UndoInfo {
  Piece captured = Piece::Empty;
  uint64_t prev_key = 0;
  int prev_halfmove_clock = 0;
  std::array<int8_t, 2> prev_general_sq{-1, -1};
  bool was_check = false;
};

class Position {
 public:
  Position();

  bool set_fen(std::string_view fen, std::string* err = nullptr);
  void set_startpos();
  std::string to_fen() const;

  Color side_to_move() const { return side_; }
  uint64_t key() const { return key_; }
  Piece piece_at(int sq) const { return board_[sq]; }
  int halfmove_clock() const { return halfmove_clock_; }
  int fullmove_number() const { return fullmove_number_; }
  int general_square(Color c) const { return general_sq_[static_cast<size_t>(c)]; }

  bool is_square_attacked_by(int target_sq, Color attacker) const;
  bool in_check(Color side) const;
  bool in_check() const { return in_check(side_); }

  void generate_pseudo_legal(MoveList& out, bool captures_only = false) const;
  void generate_legal(MoveList& out, bool captures_only = false) const;

  void make_move(Move m, UndoInfo& undo);
  void unmake_move(Move m, const UndoInfo& undo);

  bool apply_uci_move(std::string_view uci, std::string* err = nullptr);
  bool is_legal_move(Move m) const;

  bool is_repetition_in_history(int search_ply) const;
  bool is_perpetual_check_repetition(int search_ply) const;

  std::vector<std::string> sanitize_pv(const std::vector<Move>& raw_pv) const;

 private:
  void recompute_key();

  std::array<Piece, kNumSquares> board_{};
  std::array<int8_t, 2> general_sq_{-1, -1};
  Color side_ = Color::Red;
  uint64_t key_ = 0;
  int halfmove_clock_ = 0;
  int fullmove_number_ = 1;
  std::vector<uint64_t> key_history_;
  std::vector<bool> check_history_;
};

}  // namespace jade::xiangqi

#endif  // JADE_ENGINE_XIANGQI_POSITION_HPP_
