// SPDX-License-Identifier: Apache-2.0
#include "sha256.hpp"
#include "xiangqi_position.hpp"
#include "xiangqi_search.hpp"

#include <algorithm>
#include <atomic>
#include <cctype>
#include <chrono>
#include <iostream>
#include <mutex>
#include <sstream>
#include <string>
#include <thread>
#include <vector>

namespace {

using jade::xiangqi::EngineOptions;
using jade::xiangqi::Move;
using jade::xiangqi::Position;
using jade::xiangqi::Searcher;
using jade::xiangqi::SearchLimits;
using jade::xiangqi::SearchResult;
using jade::xiangqi::SearchUpdate;

std::string to_lower_ascii(std::string s) {
  for (char& c : s) {
    c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
  }
  return s;
}

std::string trim_ascii(std::string_view sv) {
  size_t b = 0;
  while (b < sv.size() && std::isspace(static_cast<unsigned char>(sv[b]))) ++b;
  size_t e = sv.size();
  while (e > b && std::isspace(static_cast<unsigned char>(sv[e - 1]))) --e;
  return std::string(sv.substr(b, e - b));
}

class UciHostLoop {
 public:
  UciHostLoop() { pos_.set_startpos(); }

  ~UciHostLoop() { stop_and_join(/*force_immediate=*/true); }

  void run() {
    std::string line;
    while (std::getline(std::cin, line)) {
      std::string trimmed = trim_ascii(line);
      if (trimmed.empty()) continue;

      std::istringstream iss(trimmed);
      std::string cmd;
      iss >> cmd;

      if (cmd == "uci") {
        std::lock_guard<std::mutex> out_lock(io_mutex_);
        std::cout << "id name Jade Reference Xiangqi 0.1.0\n"
                  << "id author Play Jade Contributors\n"
                  << "option name Hash type spin default 16 min 1 max 512\n"
                  << "option name Threads type spin default 1 min 1 max 64\n"
                  << "option name MultiPV type spin default 1 min 1 max 16\n"
                  << "option name Move Overhead type spin default 10 min 0 max 5000\n"
                  << "option name EvalFile type string default <builtin-hce-v1>\n"
                  << "option name UCI_Variant type combo default xiangqi var xiangqi\n"
                  << "uciok\n"
                  << std::flush;
      } else if (cmd == "isready") {
        std::lock_guard<std::mutex> out_lock(io_mutex_);
        std::cout << "readyok\n" << std::flush;
      } else if (cmd == "ucinewgame") {
        stop_and_join(/*force_immediate=*/true);
        searcher_.new_game();
        pos_.set_startpos();
      } else if (cmd == "setoption") {
        stop_and_join(/*force_immediate=*/true);
        handle_setoption(iss);
      } else if (cmd == "position") {
        stop_and_join(/*force_immediate=*/true);
        handle_position(iss);
      } else if (cmd == "go") {
        stop_and_join(/*force_immediate=*/true);
        handle_go(iss);
      } else if (cmd == "stop") {
        stop_and_join(/*force_immediate=*/true);
      } else if (cmd == "d" || cmd == "display") {
        std::lock_guard<std::mutex> out_lock(io_mutex_);
        std::cout << "Fen: " << pos_.to_fen() << "\n"
                  << "Key: " << std::hex << pos_.key() << std::dec << "\n"
                  << std::flush;
      } else if (cmd == "quit") {
        stop_and_join(/*force_immediate=*/!active_short_bounded_.load());
        return;
      }
    }

    // EOF on stdin: let bounded short searches complete cleanly; stop infinite searches.
    stop_and_join(/*force_immediate=*/!active_short_bounded_.load());
  }

 private:
  void stop_and_join(bool force_immediate) {
    if (!force_immediate) {
      // Allow short piped commands (e.g. `go depth 4\nquit\n`) up to 200 ms to complete naturally.
      auto wait_start = std::chrono::steady_clock::now();
      while (searching_.load(std::memory_order_acquire)) {
        auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                           std::chrono::steady_clock::now() - wait_start)
                           .count();
        if (elapsed >= 200) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
    }
    stop_flag_.store(true, std::memory_order_release);
    if (worker_.joinable()) {
      worker_.join();
    }
    searching_.store(false, std::memory_order_release);
    active_short_bounded_.store(false, std::memory_order_release);
  }

  void handle_setoption(std::istringstream& iss) {
    std::string tok;
    std::string name;
    std::string value;
    bool reading_name = false;
    bool reading_value = false;

    while (iss >> tok) {
      if (tok == "name" && !reading_value) {
        reading_name = true;
        continue;
      }
      if (tok == "value") {
        reading_name = false;
        reading_value = true;
        continue;
      }
      if (reading_name) {
        if (!name.empty()) name.push_back(' ');
        name += tok;
      } else if (reading_value) {
        if (!value.empty()) value.push_back(' ');
        value += tok;
      }
    }

    EngineOptions opts = searcher_.options();
    std::string key = to_lower_ascii(name);
    try {
      if (key == "hash" && !value.empty()) {
        opts.hash_mb = std::stoi(value);
      } else if (key == "threads" && !value.empty()) {
        opts.threads = std::stoi(value);
      } else if (key == "multipv" && !value.empty()) {
        opts.multi_pv = std::stoi(value);
      } else if (key == "move overhead" && !value.empty()) {
        opts.move_overhead_ms = std::stoi(value);
      } else if (key == "evalfile" && !value.empty()) {
        opts.network_hash = jade::crypto::Sha256::hex_digest(value);
      }
    } catch (...) {
      std::lock_guard<std::mutex> out_lock(io_mutex_);
      std::cout << "info string ignored invalid option value for " << name << "\n" << std::flush;
      return;
    }
    searcher_.configure(opts);
  }

  void handle_position(std::istringstream& iss) {
    std::string mode;
    if (!(iss >> mode)) return;

    Position next_pos;
    std::string tok;
    bool has_moves_kw = false;

    if (mode == "startpos") {
      next_pos.set_startpos();
      if (iss >> tok && tok == "moves") {
        has_moves_kw = true;
      }
    } else if (mode == "fen") {
      std::string fen;
      while (iss >> tok) {
        if (tok == "moves") {
          has_moves_kw = true;
          break;
        }
        if (!fen.empty()) fen.push_back(' ');
        fen += tok;
      }
      std::string err;
      if (!next_pos.set_fen(fen, &err)) {
        std::lock_guard<std::mutex> out_lock(io_mutex_);
        std::cout << "info string error setting FEN: " << err << "\n" << std::flush;
        return;
      }
    } else {
      return;
    }

    if (has_moves_kw) {
      while (iss >> tok) {
        std::string err;
        if (!next_pos.apply_uci_move(tok, &err)) {
          std::lock_guard<std::mutex> out_lock(io_mutex_);
          std::cout << "info string error applying move: " << err << "\n" << std::flush;
          return;
        }
      }
    }
    pos_ = std::move(next_pos);
  }

  void handle_go(std::istringstream& iss) {
    SearchLimits limits{};
    bool any_limit_set = false;
    std::string tok;
    while (iss >> tok) {
      if (tok == "infinite") {
        limits.infinite = true;
        any_limit_set = true;
      } else if (tok == "depth") {
        if (iss >> limits.max_depth) any_limit_set = true;
      } else if (tok == "movetime") {
        if (iss >> limits.movetime_ms) any_limit_set = true;
      } else if (tok == "nodes") {
        if (iss >> limits.max_nodes) any_limit_set = true;
      } else if (tok == "wtime") {
        if (iss >> limits.wtime_ms) any_limit_set = true;
      } else if (tok == "btime") {
        if (iss >> limits.btime_ms) any_limit_set = true;
      } else if (tok == "winc") {
        iss >> limits.winc_ms;
      } else if (tok == "binc") {
        iss >> limits.binc_ms;
      } else if (tok == "movestogo") {
        iss >> limits.movestogo;
      }
    }
    if (!any_limit_set) {
      limits.max_depth = 6;
    }

    bool is_short_bounded = !limits.infinite &&
                            ((limits.max_depth > 0 && limits.max_depth <= 8) ||
                             (limits.movetime_ms > 0 && limits.movetime_ms <= 250) ||
                             (limits.max_nodes > 0 && limits.max_nodes <= 100000));

    stop_flag_.store(false, std::memory_order_release);
    searching_.store(true, std::memory_order_release);
    active_short_bounded_.store(is_short_bounded, std::memory_order_release);

    Position search_pos = pos_;
    worker_ = std::thread([this, search_pos, limits]() {
      SearchResult res = searcher_.search(
          search_pos,
          limits,
          &stop_flag_,
          nullptr,
          [this](const SearchUpdate& upd) {
            std::lock_guard<std::mutex> out_lock(io_mutex_);
            std::cout << "info depth " << upd.depth
                      << " seldepth " << upd.seldepth
                      << " multipv " << upd.line.multipv
                      << " score " << (upd.line.score.is_mate ? "mate " : "cp ")
                      << upd.line.score.value
                      << " nodes " << upd.nodes
                      << " nps " << upd.nps
                      << " hashfull " << upd.hashfull
                      << " time " << upd.time_ms
                      << " pv";
            for (const auto& mv : upd.line.pv) {
              std::cout << ' ' << mv;
            }
            std::cout << '\n' << std::flush;
          });

      {
        std::lock_guard<std::mutex> out_lock(io_mutex_);
        std::string bm = res.best_move.is_none() ? "(none)" : res.best_move.to_uci();
        std::cout << "bestmove " << bm << '\n' << std::flush;
      }
      searching_.store(false, std::memory_order_release);
    });
  }

  Position pos_{};
  Searcher searcher_{};
  std::mutex io_mutex_{};
  std::atomic<bool> stop_flag_{false};
  std::atomic<bool> searching_{false};
  std::atomic<bool> active_short_bounded_{false};
  std::thread worker_{};
};

int run_bench() {
  Searcher searcher;
  Position pos;
  pos.set_startpos();
  SearchLimits limits{};
  limits.max_depth = 5;
  std::atomic<bool> stop{false};
  SearchResult r1 = searcher.search(pos, limits, &stop, nullptr, nullptr);
  std::cout << "bench startpos depth 5 bestmove " << r1.best_move.to_uci()
            << " nodes " << r1.nodes << " time_ms " << r1.time_ms
            << " nps " << r1.nps << "\n";
  return r1.best_move.is_none() ? 1 : 0;
}

}  // namespace

int main(int argc, char** argv) {
  for (int i = 1; i < argc; ++i) {
    std::string_view arg(argv[i]);
    if (arg == "--version" || arg == "-v") {
      std::cout << "Jade Reference Xiangqi 0.1.0 (Apache-2.0, rules program-2023.11-v1)\n";
      return 0;
    }
    if (arg == "--bench") {
      return run_bench();
    }
  }

  UciHostLoop host;
  host.run();
  return 0;
}
