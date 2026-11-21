// SPDX-License-Identifier: Apache-2.0
#include "xiangqi_search.hpp"

#include <algorithm>
#include <cmath>
#include <cstring>
#include <thread>

namespace jade::xiangqi {
namespace {

constexpr int kMaterialValue[8] = {
  0,      // Empty
  10000,  // General (for MVV-LVA ordering)
  210,    // Advisor
  210,    // Elephant
  435,    // Horse
  960,    // Chariot
  465,    // Cannon
  115     // Soldier
};

// Piece-square tables from Red's perspective (rank 0 = bottom, rank 9 = top).
constexpr int kSoldierPst[kNumSquares] = {
   0,  0,  0,  0,  0,  0,  0,  0,  0,
   0,  0,  0,  0,  0,  0,  0,  0,  0,
   0,  0,  0,  0,  0,  0,  0,  0,  0,
  -2,  0, -2,  0,  8,  0, -2,  0, -2,
   6,  0, 12,  0, 20,  0, 12,  0,  6,
  35, 45, 58, 70, 78, 70, 58, 45, 35,
  48, 64, 82, 98,108, 98, 82, 64, 48,
  55, 76, 96,120,132,120, 96, 76, 55,
  45, 68, 88,114,124,114, 88, 68, 45,
  20, 32, 42, 56, 62, 56, 42, 32, 20,
};

constexpr int kHorsePst[kNumSquares] = {
 -20,-14,-12,-10,-10,-10,-12,-14,-20,
 -12, -4,  8,  6, -6,  6,  8, -4,-12,
  -8,  6, 22, 18, 12, 18, 22,  6, -8,
  -6, 10, 20, 26, 24, 26, 20, 10, -6,
  -4, 14, 26, 34, 32, 34, 26, 14, -4,
  -2, 16, 30, 40, 38, 40, 30, 16, -2,
   0, 20, 34, 46, 44, 46, 34, 20,  0,
   2, 18, 36, 48, 42, 48, 36, 18,  2,
  -6, 12, 24, 34, 20, 34, 24, 12, -6,
 -18, -8, -4,  4, -4,  4, -4, -8,-18,
};

constexpr int kCannonPst[kNumSquares] = {
   0,  0,  4,  8, 12,  8,  4,  0,  0,
   0,  4,  6, 10, 12, 10,  6,  4,  0,
   4,  8, 10, 18, 34, 18, 10,  8,  4,
   2,  4,  8, 14, 22, 14,  8,  4,  2,
   4,  6, 10, 16, 24, 16, 10,  6,  4,
   6,  8, 12, 18, 26, 18, 12,  8,  6,
   6, 10, 14, 20, 28, 20, 14, 10,  6,
   8, 12, 16, 22, 32, 22, 16, 12,  8,
   4,  8, 10, 14, 18, 14, 10,  8,  4,
   2,  4,  6, 10, 14, 10,  6,  4,  2,
};

constexpr int kChariotPst[kNumSquares] = {
  -6,  6,  4, 14,  8, 14,  4,  6, -6,
   4, 10,  8, 16, 10, 16,  8, 10,  4,
   4, 10, 10, 18, 14, 18, 10, 10,  4,
   8, 14, 14, 22, 20, 22, 14, 14,  8,
  14, 20, 20, 26, 26, 26, 20, 20, 14,
  16, 22, 22, 28, 28, 28, 22, 22, 16,
  18, 26, 26, 32, 32, 32, 26, 26, 18,
  18, 26, 26, 34, 34, 34, 26, 26, 18,
  20, 28, 28, 36, 30, 36, 28, 28, 20,
  14, 22, 20, 28, 24, 28, 20, 22, 14,
};

constexpr int kAdvisorElephantGeneralBonus(PieceType pt, int f, int r) {
  if (pt == PieceType::Advisor) {
    if (f == 4 && r == 1) return 22;
    if ((f == 3 || f == 5) && (r == 0 || r == 2)) return 8;
    return 0;
  }
  if (pt == PieceType::Elephant) {
    if (f == 4 && r == 2) return 22;
    if ((f == 2 || f == 6) && r == 4) return 12;
    if ((f == 0 || f == 8) && r == 2) return 4;
    return 6;
  }
  if (pt == PieceType::General) {
    if (f == 4 && r == 0) return 18;
    if (r == 0) return 8;
    if (r == 1) return -4;
    return -14;
  }
  return 0;
}

constexpr int mirror_for_black(int sq) {
  int f = file_of(sq);
  int r = rank_of(sq);
  return make_square(f, (kNumRanks - 1) - r);
}

int piece_square_eval(PieceType pt, int red_sq) {
  switch (pt) {
    case PieceType::General:
      return kAdvisorElephantGeneralBonus(pt, file_of(red_sq), rank_of(red_sq));
    case PieceType::Advisor:
      return kMaterialValue[2] + kAdvisorElephantGeneralBonus(pt, file_of(red_sq), rank_of(red_sq));
    case PieceType::Elephant:
      return kMaterialValue[3] + kAdvisorElephantGeneralBonus(pt, file_of(red_sq), rank_of(red_sq));
    case PieceType::Horse:
      return kMaterialValue[4] + kHorsePst[red_sq];
    case PieceType::Chariot:
      return kMaterialValue[5] + kChariotPst[red_sq];
    case PieceType::Cannon:
      return kMaterialValue[6] + kCannonPst[red_sq];
    case PieceType::Soldier:
      return kMaterialValue[7] + kSoldierPst[red_sq];
    case PieceType::Empty:
      return 0;
  }
  return 0;
}

}  // namespace

NormalizedScore normalize_score(int raw_score, Color root_side) {
  NormalizedScore out{};
  out.raw_score = raw_score;
  if (std::abs(raw_score) >= kMateThreshold) {
    out.is_mate = true;
    int plies = kMateScore - std::abs(raw_score);
    int mate_moves = std::max(1, (plies + 1) / 2);
    out.value = raw_score > 0 ? mate_moves : -mate_moves;
  } else {
    out.is_mate = false;
    out.value = raw_score;
  }
  out.red_value = (root_side == Color::Red) ? out.value : -out.value;
  return out;
}

TranspositionTable::TranspositionTable(int size_mb) { resize_mb(size_mb); }

void TranspositionTable::resize_mb(int size_mb) {
  int clamped_mb = std::clamp(size_mb, 1, 512);
  size_t bytes = static_cast<size_t>(clamped_mb) * 1024ULL * 1024ULL;
  size_t target_entries = std::max<size_t>(1024, bytes / sizeof(Entry));
  size_t pow2 = 1024;
  while ((pow2 << 1) <= target_entries) {
    pow2 <<= 1;
  }
  entries_.assign(pow2, Entry{});
  mask_ = pow2 - 1;
  generation_ = 0;
}

void TranspositionTable::clear() {
  std::fill(entries_.begin(), entries_.end(), Entry{});
  generation_ = 0;
}

int TranspositionTable::score_to_tt(int score, int ply) {
  if (score >= kMateThreshold) return score + ply;
  if (score <= -kMateThreshold) return score - ply;
  return score;
}

int TranspositionTable::score_from_tt(int score, int ply) {
  if (score >= kMateThreshold) return score - ply;
  if (score <= -kMateThreshold) return score + ply;
  return score;
}

bool TranspositionTable::probe(uint64_t key, Entry& out) const {
  if (entries_.empty()) return false;
  const Entry& slot = entries_[static_cast<size_t>(key) & mask_];
  if (slot.bound == Bound::None || slot.key != key) return false;
  out = slot;
  return true;
}

void TranspositionTable::store(
    uint64_t key, Move best_move, int score, int depth, Bound bound, int ply) {
  if (entries_.empty()) return;
  Entry& slot = entries_[static_cast<size_t>(key) & mask_];
  bool should_replace = (slot.bound == Bound::None) ||
                        (slot.key != key) ||
                        (slot.generation != generation_) ||
                        (depth >= slot.depth) ||
                        (bound == Bound::Exact);
  if (!should_replace) return;

  uint16_t move_raw = best_move.is_none() && slot.key == key ? slot.best_move : best_move.raw;
  slot.key = key;
  slot.best_move = move_raw;
  slot.score = static_cast<int16_t>(
      std::clamp(score_to_tt(score, ply), -kScoreInfinite, kScoreInfinite));
  slot.depth = static_cast<int8_t>(std::clamp(depth, 0, 127));
  slot.bound = bound;
  slot.generation = generation_;
}

int TranspositionTable::hashfull_permille() const {
  if (entries_.empty()) return 0;
  size_t sample = std::min<size_t>(1000, entries_.size());
  int used = 0;
  for (size_t i = 0; i < sample; ++i) {
    if (entries_[i].bound != Bound::None && entries_[i].generation == generation_) {
      ++used;
    }
  }
  return static_cast<int>((static_cast<uint64_t>(used) * 1000ULL) / sample);
}

Searcher::Searcher() : tt_(16) { new_game(); }

void Searcher::configure(const EngineOptions& opts) {
  bool hash_changed = (opts.hash_mb != options_.hash_mb);
  options_ = opts;
  options_.hash_mb = std::clamp(options_.hash_mb, 1, 512);
  options_.threads = std::clamp(options_.threads, 1, 64);
  options_.multi_pv = std::clamp(options_.multi_pv, 1, 16);
  options_.move_overhead_ms = std::clamp(options_.move_overhead_ms, 0, 5000);
  if (hash_changed) {
    tt_.resize_mb(options_.hash_mb);
  }
}

void Searcher::new_game() {
  tt_.clear();
  for (auto& k : killers_) {
    k = {Move{}, Move{}};
  }
  for (auto& row : history_) {
    row.fill(0);
  }
}

int Searcher::evaluate(const Position& pos) {
  int red_score = 0;
  int black_score = 0;

  for (int sq = 0; sq < kNumSquares; ++sq) {
    Piece p = pos.piece_at(sq);
    if (p == Piece::Empty) continue;
    PieceType pt = type_of(p);
    if (color_of(p) == Color::Red) {
      red_score += piece_square_eval(pt, sq);
    } else {
      black_score += piece_square_eval(pt, mirror_for_black(sq));
    }
  }

  int diff = (pos.side_to_move() == Color::Red)
                 ? (red_score - black_score)
                 : (black_score - red_score);
  return diff + 12;  // Small side-to-move tempo bonus.
}

bool Searcher::check_abort() {
  if (aborted_) return true;
  if (stop_flag_ != nullptr && stop_flag_->load(std::memory_order_relaxed)) {
    aborted_ = true;
    return true;
  }
  if (extra_abort_fn_ && extra_abort_fn_()) {
    aborted_ = true;
    return true;
  }
  if (max_nodes_ > 0 && nodes_ >= max_nodes_) {
    aborted_ = true;
    return true;
  }
  if (hard_deadline_ms_ > 0) {
    auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                       std::chrono::steady_clock::now() - start_time_)
                       .count();
    if (elapsed >= hard_deadline_ms_) {
      aborted_ = true;
      return true;
    }
  }
  return false;
}

void Searcher::score_moves(
    const Position& pos,
    const MoveList& list,
    Move tt_move,
    int ply,
    std::array<int, 128>& scores) const {
  Move killer0 = (ply < kMaxPly) ? killers_[ply][0] : Move{};
  Move killer1 = (ply < kMaxPly) ? killers_[ply][1] : Move{};

  for (int i = 0; i < list.count; ++i) {
    Move m = list[i];
    if (!tt_move.is_none() && m == tt_move) {
      scores[i] = 1000000;
      continue;
    }
    Piece victim = pos.piece_at(m.to());
    Piece attacker = pos.piece_at(m.from());
    if (victim != Piece::Empty) {
      int v_val = kMaterialValue[static_cast<size_t>(type_of(victim))];
      int a_val = kMaterialValue[static_cast<size_t>(type_of(attacker))];
      scores[i] = 500000 + (v_val * 16) - a_val;
    } else if (!killer0.is_none() && m == killer0) {
      scores[i] = 400000;
    } else if (!killer1.is_none() && m == killer1) {
      scores[i] = 390000;
    } else {
      scores[i] = history_[static_cast<size_t>(attacker)][m.to()];
    }
  }
}

int Searcher::quiescence(Position& pos, int alpha, int beta, int ply) {
  if (ply > seldepth_) seldepth_ = ply;
  if ((++nodes_ & 255ULL) == 0ULL && check_abort()) {
    return 0;
  }
  if (max_nodes_ > 0 && nodes_ >= max_nodes_) {
    aborted_ = true;
    return 0;
  }
  if (ply >= kMaxPly - 1) {
    return evaluate(pos);
  }

  const bool in_chk = pos.in_check();
  if (!in_chk) {
    int stand_pat = evaluate(pos);
    if (stand_pat >= beta) {
      return beta;
    }
    if (stand_pat > alpha) {
      alpha = stand_pat;
    }
  }

  MoveList moves;
  pos.generate_legal(moves, /*captures_only=*/!in_chk);
  if (moves.empty()) {
    if (in_chk) {
      return -kMateScore + ply;
    }
    return alpha;
  }

  std::array<int, 128> scores{};
  score_moves(pos, moves, Move{}, ply, scores);

  for (int i = 0; i < moves.count; ++i) {
    int best_idx = i;
    for (int j = i + 1; j < moves.count; ++j) {
      if (scores[j] > scores[best_idx]) best_idx = j;
    }
    if (best_idx != i) {
      std::swap(moves[i], moves[best_idx]);
      std::swap(scores[i], scores[best_idx]);
    }

    UndoInfo undo;
    pos.make_move(moves[i], undo);
    int score = -quiescence(pos, -beta, -alpha, ply + 1);
    pos.unmake_move(moves[i], undo);

    if (aborted_) return 0;
    if (score >= beta) return beta;
    if (score > alpha) alpha = score;
  }

  return alpha;
}

int Searcher::alpha_beta(Position& pos, int depth, int alpha, int beta, int ply, PvTable& pv) {
  pv.clear_ply(ply);
  if (ply > seldepth_) seldepth_ = ply;

  if ((++nodes_ & 255ULL) == 0ULL && check_abort()) {
    return 0;
  }
  if (max_nodes_ > 0 && nodes_ >= max_nodes_) {
    aborted_ = true;
    return 0;
  }

  if (ply > 0) {
    if (pos.is_perpetual_check_repetition(ply)) {
      // Opponent just delivered forbidden perpetual check; side to move wins.
      return kMateScore - ply - 2;
    }
    if (pos.is_repetition_in_history(ply)) {
      return 0;
    }
    if (pos.halfmove_clock() >= 120) {
      return 0;
    }
    // Mate distance pruning.
    alpha = std::max(alpha, -kMateScore + ply);
    beta = std::min(beta, kMateScore - ply - 1);
    if (alpha >= beta) return alpha;
  }

  if (ply >= kMaxPly - 1) {
    return evaluate(pos);
  }

  const bool in_chk = pos.in_check();
  if (in_chk && ply < kMaxPly / 2) {
    ++depth;  // Check extension.
  }

  if (depth <= 0) {
    --nodes_;  // Avoid double-counting leaf entry in quiescence.
    return quiescence(pos, alpha, beta, ply);
  }

  const bool is_pv_node = (beta - alpha > 1);
  Move tt_move{};
  TranspositionTable::Entry tt_entry{};
  if (tt_.probe(pos.key(), tt_entry)) {
    tt_move = Move(tt_entry.best_move);
    if (!is_pv_node && tt_entry.depth >= depth && ply > 0) {
      int tt_score = TranspositionTable::score_from_tt(tt_entry.score, ply);
      if (tt_entry.bound == TranspositionTable::Bound::Exact) return tt_score;
      if (tt_entry.bound == TranspositionTable::Bound::Lower && tt_score >= beta) return tt_score;
      if (tt_entry.bound == TranspositionTable::Bound::Upper && tt_score <= alpha) return tt_score;
    }
  }

  MoveList legal;
  pos.generate_legal(legal, false);
  if (legal.empty()) {
    // In Xiangqi, both checkmate and stalemate (no legal moves) lose for side to move.
    return -kMateScore + ply;
  }

  std::array<int, 128> scores{};
  score_moves(pos, legal, tt_move, ply, scores);

  const int orig_alpha = alpha;
  Move best_move = legal[0];
  int best_score = -kScoreInfinite;

  for (int i = 0; i < legal.count; ++i) {
    int best_idx = i;
    for (int j = i + 1; j < legal.count; ++j) {
      if (scores[j] > scores[best_idx]) best_idx = j;
    }
    if (best_idx != i) {
      std::swap(legal[i], legal[best_idx]);
      std::swap(scores[i], scores[best_idx]);
    }

    Move m = legal[i];
    bool is_capture = (pos.piece_at(m.to()) != Piece::Empty);
    Piece moving_piece = pos.piece_at(m.from());

    UndoInfo undo;
    pos.make_move(m, undo);

    int score = 0;
    if (i == 0) {
      score = -alpha_beta(pos, depth - 1, -beta, -alpha, ply + 1, pv);
    } else {
      int reduction = 0;
      if (depth >= 3 && i >= 4 && !in_chk && !is_capture && !pos.in_check()) {
        reduction = 1;
      }
      score = -alpha_beta(pos, depth - 1 - reduction, -alpha - 1, -alpha, ply + 1, pv);
      if (!aborted_ && score > alpha && (reduction > 0 || is_pv_node)) {
        score = -alpha_beta(pos, depth - 1, -beta, -alpha, ply + 1, pv);
      }
    }

    pos.unmake_move(m, undo);
    if (aborted_) return 0;

    if (score > best_score) {
      best_score = score;
      best_move = m;
    }
    if (score > alpha) {
      alpha = score;
      // Record PV line for this node.
      pv.moves[ply][0] = m;
      int child_len = (ply + 1 < kMaxPly) ? pv.length[ply + 1] : 0;
      int copy_len = std::min(child_len, kMaxPly - 1 - ply);
      for (int k = 0; k < copy_len; ++k) {
        pv.moves[ply][k + 1] = pv.moves[ply + 1][k];
      }
      pv.length[ply] = 1 + copy_len;

      if (alpha >= beta) {
        if (!is_capture && ply < kMaxPly) {
          if (killers_[ply][0] != m) {
            killers_[ply][1] = killers_[ply][0];
            killers_[ply][0] = m;
          }
          int bonus = depth * depth;
          int& h = history_[static_cast<size_t>(moving_piece)][m.to()];
          h = std::min(300000, h + bonus);
        }
        break;
      }
    }
  }

  TranspositionTable::Bound bound = TranspositionTable::Bound::Exact;
  if (best_score <= orig_alpha) {
    bound = TranspositionTable::Bound::Upper;
  } else if (best_score >= beta) {
    bound = TranspositionTable::Bound::Lower;
  }
  tt_.store(pos.key(), best_move, best_score, depth, bound, ply);
  return best_score;
}

SearchResult Searcher::search(
    const Position& root_pos,
    const SearchLimits& limits,
    const std::atomic<bool>* stop_flag,
    const std::function<bool()>& extra_abort_fn,
    const std::function<void(const SearchUpdate&)>& on_update) {
  start_time_ = std::chrono::steady_clock::now();
  stop_flag_ = stop_flag;
  extra_abort_fn_ = extra_abort_fn;
  nodes_ = 0;
  seldepth_ = 0;
  aborted_ = false;
  max_nodes_ = limits.max_nodes;
  tt_.new_search();

  for (auto& row : history_) {
    for (int& v : row) {
      v /= 2;
    }
  }

  // Compute time budgets.
  hard_deadline_ms_ = 0;
  soft_deadline_ms_ = 0;
  if (!limits.infinite) {
    if (limits.movetime_ms > 0) {
      hard_deadline_ms_ = std::max<int64_t>(1, limits.movetime_ms - options_.move_overhead_ms);
      soft_deadline_ms_ = hard_deadline_ms_;
    } else {
      int64_t rem_ms = (root_pos.side_to_move() == Color::Red) ? limits.wtime_ms : limits.btime_ms;
      int64_t inc_ms = (root_pos.side_to_move() == Color::Red) ? limits.winc_ms : limits.binc_ms;
      if (rem_ms > 0) {
        int mtg = limits.movestogo > 0 ? limits.movestogo : 30;
        int64_t safe_rem = std::max<int64_t>(1, rem_ms - options_.move_overhead_ms);
        soft_deadline_ms_ = std::max<int64_t>(1, safe_rem / mtg + (inc_ms * 3) / 4);
        hard_deadline_ms_ = std::max<int64_t>(soft_deadline_ms_, safe_rem / 3);
      }
    }
  }

  const int max_depth = std::clamp(limits.max_depth > 0 ? limits.max_depth : 64, 1, kMaxPly - 4);
  const int requested_mpv =
      std::clamp(limits.multi_pv > 0 ? limits.multi_pv : options_.multi_pv, 1, 16);

  SearchResult result{};
  result.side_to_move = root_pos.side_to_move();

  MoveList root_legal;
  root_pos.generate_legal(root_legal, false);
  if (root_legal.empty()) {
    result.score = normalize_score(-kMateScore, root_pos.side_to_move());
    if (limits.infinite) {
      while (!check_abort()) {
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
      result.aborted = aborted_;
    }
    auto end_empty = std::chrono::steady_clock::now();
    result.time_ms = std::max<int64_t>(
        0, std::chrono::duration_cast<std::chrono::milliseconds>(end_empty - start_time_).count());
    return result;
  }

  // Always seed fallback with a verified legal move before checking budgets.
  result.best_move = root_legal[0];
  result.pv = {root_legal[0].to_uci()};
  result.score = normalize_score(evaluate(root_pos), root_pos.side_to_move());

  const int mpv_count = std::min(requested_mpv, root_legal.count);
  std::vector<Move> prev_iter_order(root_legal.begin(), root_legal.end());

  // Score root moves initially for depth 1 ordering.
  {
    std::array<int, 128> init_scores{};
    TranspositionTable::Entry root_tt{};
    Move root_tt_move = tt_.probe(root_pos.key(), root_tt) ? Move(root_tt.best_move) : Move{};
    score_moves(root_pos, root_legal, root_tt_move, 0, init_scores);
    std::vector<std::pair<int, Move>> paired;
    paired.reserve(root_legal.count);
    for (int i = 0; i < root_legal.count; ++i) {
      paired.emplace_back(init_scores[i], root_legal[i]);
    }
    std::stable_sort(paired.begin(), paired.end(), [](const auto& a, const auto& b) {
      return a.first > b.first;
    });
    for (size_t i = 0; i < paired.size(); ++i) {
      prev_iter_order[i] = paired[i].second;
    }
    result.best_move = prev_iter_order[0];
    result.pv = {prev_iter_order[0].to_uci()};
  }

  Position work_pos = root_pos;
  PvTable pv_table{};

  for (int depth = 1; depth <= max_depth; ++depth) {
    if (check_abort()) break;

    std::vector<SearchPvLine> depth_lines;
    depth_lines.reserve(mpv_count);
    std::vector<Move> chosen_roots;
    chosen_roots.reserve(mpv_count);

    for (int mpv_idx = 1; mpv_idx <= mpv_count; ++mpv_idx) {
      int alpha = -kScoreInfinite;
      int beta = kScoreInfinite;
      Move best_root{};
      int best_root_score = -kScoreInfinite;
      std::vector<Move> best_raw_pv;

      int searched_idx = 0;
      for (Move rm : prev_iter_order) {
        if (std::find(chosen_roots.begin(), chosen_roots.end(), rm) != chosen_roots.end()) {
          continue;
        }

        UndoInfo undo;
        work_pos.make_move(rm, undo);
        pv_table.clear_ply(1);

        int score = 0;
        if (searched_idx == 0) {
          score = -alpha_beta(work_pos, depth - 1, -beta, -alpha, 1, pv_table);
        } else {
          score = -alpha_beta(work_pos, depth - 1, -alpha - 1, -alpha, 1, pv_table);
          if (!aborted_ && score > alpha) {
            score = -alpha_beta(work_pos, depth - 1, -beta, -alpha, 1, pv_table);
          }
        }
        work_pos.unmake_move(rm, undo);
        ++searched_idx;

        if (aborted_) break;

        if (score > best_root_score || best_root.is_none()) {
          best_root_score = score;
          best_root = rm;
          best_raw_pv.clear();
          best_raw_pv.push_back(rm);
          int child_len = pv_table.length[1];
          for (int k = 0; k < child_len; ++k) {
            best_raw_pv.push_back(pv_table.moves[1][k]);
          }
        }
        if (score > alpha) {
          alpha = score;
        }
      }

      if (aborted_ || best_root.is_none()) {
        break;
      }

      chosen_roots.push_back(best_root);
      SearchPvLine line{};
      line.multipv = mpv_idx;
      line.depth = depth;
      line.seldepth = std::max(depth, seldepth_);
      line.best_move = best_root;
      line.score = normalize_score(best_root_score, root_pos.side_to_move());
      line.pv = root_pos.sanitize_pv(best_raw_pv);
      if (line.pv.empty()) {
        line.pv.push_back(best_root.to_uci());
      }
      depth_lines.push_back(std::move(line));
    }

    if (depth_lines.empty()) {
      break;
    }

    // Sort completed MultiPV lines descending by raw score and re-index multipv 1..N.
    std::stable_sort(depth_lines.begin(), depth_lines.end(), [](const auto& a, const auto& b) {
      return a.score.raw_score > b.score.raw_score;
    });
    for (size_t i = 0; i < depth_lines.size(); ++i) {
      depth_lines[i].multipv = static_cast<int>(i + 1);
    }

    // Commit completed lines only if full MultiPV iteration finished (or depth == 1).
    if (!aborted_ || depth == 1) {
      result.completed_depth = depth;
      result.seldepth = std::max(depth, seldepth_);
      result.best_move = depth_lines[0].best_move;
      result.score = depth_lines[0].score;
      result.pv = depth_lines[0].pv;
      result.lines = depth_lines;

      // Promote best root moves to front of next iteration's move order.
      std::vector<Move> next_order;
      next_order.reserve(prev_iter_order.size());
      for (const auto& l : depth_lines) {
        next_order.push_back(l.best_move);
      }
      for (Move rm : prev_iter_order) {
        if (std::find(next_order.begin(), next_order.end(), rm) == next_order.end()) {
          next_order.push_back(rm);
        }
      }
      prev_iter_order = std::move(next_order);

      auto now = std::chrono::steady_clock::now();
      int64_t elapsed_ms = std::max<int64_t>(
          0, std::chrono::duration_cast<std::chrono::milliseconds>(now - start_time_).count());
      uint64_t nps = elapsed_ms > 0 ? (nodes_ * 1000ULL) / static_cast<uint64_t>(elapsed_ms)
                                    : nodes_ * 1000ULL;
      int hf = tt_.hashfull_permille();

      if (!aborted_ && on_update) {
        for (const auto& l : result.lines) {
          if (check_abort()) break;
          SearchUpdate upd{};
          upd.depth = depth;
          upd.seldepth = l.seldepth;
          upd.nodes = nodes_;
          upd.nps = nps;
          upd.time_ms = elapsed_ms;
          upd.hashfull = hf;
          upd.side_to_move = root_pos.side_to_move();
          upd.line = l;
          on_update(upd);
        }
      }
    }

    if (aborted_) break;

    if (soft_deadline_ms_ > 0) {
      auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                         std::chrono::steady_clock::now() - start_time_)
                         .count();
      if (elapsed >= soft_deadline_ms_) {
        break;
      }
    }
  }

  if (limits.infinite && !aborted_) {
    while (!check_abort()) {
      std::this_thread::sleep_for(std::chrono::milliseconds(1));
    }
  }

  auto end_time = std::chrono::steady_clock::now();
  result.aborted = aborted_;
  result.nodes = nodes_;
  result.time_ms = std::max<int64_t>(
      0, std::chrono::duration_cast<std::chrono::milliseconds>(end_time - start_time_).count());
  result.nps = result.time_ms > 0
                   ? (result.nodes * 1000ULL) / static_cast<uint64_t>(result.time_ms)
                   : result.nodes * 1000ULL;
  result.hashfull = tt_.hashfull_permille();
  if (result.lines.empty() && !result.best_move.is_none()) {
    SearchPvLine fallback{};
    fallback.multipv = 1;
    fallback.depth = std::max(1, result.completed_depth);
    fallback.seldepth = std::max(1, result.seldepth);
    fallback.best_move = result.best_move;
    fallback.score = result.score;
    fallback.pv = result.pv;
    result.lines.push_back(std::move(fallback));
  }
  return result;
}

}  // namespace jade::xiangqi
