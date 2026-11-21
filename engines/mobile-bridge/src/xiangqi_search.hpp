// SPDX-License-Identifier: Apache-2.0
#ifndef JADE_ENGINE_XIANGQI_SEARCH_HPP_
#define JADE_ENGINE_XIANGQI_SEARCH_HPP_

#include "xiangqi_position.hpp"

#include <array>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <functional>
#include <string>
#include <vector>

namespace jade::xiangqi {

inline constexpr int kScoreInfinite = 32000;
inline constexpr int kMateScore = 30000;
inline constexpr int kMateThreshold = 29000;
inline constexpr int kMaxPly = 96;

struct EngineOptions {
  int hash_mb = 16;
  int threads = 1;
  int multi_pv = 1;
  int move_overhead_ms = 10;
  std::string rules_profile = "program-2023.11-v1";
  std::string engine_id = "jade-reference-cpp-0.1.0";
  std::string network_hash =
      "0000000000000000000000000000000000000000000000000000000000000000";
};

struct SearchLimits {
  int max_depth = 64;
  int64_t movetime_ms = 0;
  int64_t wtime_ms = 0;
  int64_t btime_ms = 0;
  int64_t winc_ms = 0;
  int64_t binc_ms = 0;
  int movestogo = 0;
  uint64_t max_nodes = 0;
  bool infinite = false;
  int multi_pv = 0;  // 0 => use EngineOptions::multi_pv
};

struct NormalizedScore {
  bool is_mate = false;
  int value = 0;      // cp or mate-in-N from side_to_move perspective
  int red_value = 0;  // cp or mate-in-N from Red perspective
  int raw_score = 0;  // internal centipawn/mate score from side_to_move
};

NormalizedScore normalize_score(int raw_score, Color root_side);

struct SearchPvLine {
  int multipv = 1;
  int depth = 0;
  int seldepth = 0;
  NormalizedScore score{};
  Move best_move{};
  std::vector<std::string> pv;
};

struct SearchUpdate {
  int depth = 0;
  int seldepth = 0;
  uint64_t nodes = 0;
  uint64_t nps = 0;
  int64_t time_ms = 0;
  int hashfull = 0;
  Color side_to_move = Color::Red;
  SearchPvLine line{};
};

struct SearchResult {
  bool aborted = false;
  int completed_depth = 0;
  int seldepth = 0;
  uint64_t nodes = 0;
  uint64_t nps = 0;
  int64_t time_ms = 0;
  int hashfull = 0;
  Color side_to_move = Color::Red;
  Move best_move{};
  NormalizedScore score{};
  std::vector<std::string> pv;
  std::vector<SearchPvLine> lines;
};

class TranspositionTable {
 public:
  enum class Bound : uint8_t {
    None = 0,
    Exact = 1,
    Lower = 2,
    Upper = 3,
  };

  struct Entry {
    uint64_t key = 0;
    uint16_t best_move = 0;
    int16_t score = 0;
    int8_t depth = -1;
    Bound bound = Bound::None;
    uint8_t generation = 0;
  };

  explicit TranspositionTable(int size_mb = 16);

  void resize_mb(int size_mb);
  void clear();
  void new_search() { ++generation_; }

  bool probe(uint64_t key, Entry& out) const;
  void store(uint64_t key, Move best_move, int score, int depth, Bound bound, int ply);
  int hashfull_permille() const;

  static int score_to_tt(int score, int ply);
  static int score_from_tt(int score, int ply);

 private:
  std::vector<Entry> entries_;
  size_t mask_ = 0;
  uint8_t generation_ = 0;
};

class Searcher {
 public:
  Searcher();

  void configure(const EngineOptions& opts);
  const EngineOptions& options() const { return options_; }
  void new_game();

  SearchResult search(
      const Position& root_pos,
      const SearchLimits& limits,
      const std::atomic<bool>* stop_flag,
      const std::function<bool()>& extra_abort_fn,
      const std::function<void(const SearchUpdate&)>& on_update);

  static int evaluate(const Position& pos);

 private:
  struct PvTable {
    std::array<std::array<Move, kMaxPly>, kMaxPly> moves{};
    std::array<int, kMaxPly> length{};

    void clear_ply(int ply) {
      if (ply < kMaxPly) length[ply] = 0;
    }
  };

  bool check_abort();
  int alpha_beta(Position& pos, int depth, int alpha, int beta, int ply, PvTable& pv);
  int quiescence(Position& pos, int alpha, int beta, int ply);
  void score_moves(
      const Position& pos,
      const MoveList& list,
      Move tt_move,
      int ply,
      std::array<int, 128>& scores) const;

  EngineOptions options_{};
  TranspositionTable tt_;
  std::array<std::array<Move, 2>, kMaxPly> killers_{};
  std::array<std::array<int, kNumSquares>, 15> history_{};

  // Per-search transient state.
  const std::atomic<bool>* stop_flag_ = nullptr;
  std::function<bool()> extra_abort_fn_{};
  std::chrono::steady_clock::time_point start_time_{};
  int64_t hard_deadline_ms_ = 0;
  int64_t soft_deadline_ms_ = 0;
  uint64_t max_nodes_ = 0;
  uint64_t nodes_ = 0;
  int seldepth_ = 0;
  bool aborted_ = false;
};

}  // namespace jade::xiangqi

#endif  // JADE_ENGINE_XIANGQI_SEARCH_HPP_
