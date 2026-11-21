// SPDX-License-Identifier: Apache-2.0
#include "jade_engine.h"

#include "json_mini.hpp"
#include "xiangqi_position.hpp"
#include "xiangqi_search.hpp"

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <iostream>
#include <mutex>
#include <set>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {

int g_failures = 0;

void check_impl(bool cond, const char* expr, const char* file, int line, std::string_view detail = {}) {
  if (!cond) {
    ++g_failures;
    std::cerr << "[FAIL] " << file << ":" << line << " assertion failed: " << expr;
    if (!detail.empty()) {
      std::cerr << " (" << detail << ")";
    }
    std::cerr << "\n";
  }
}

#define CHECK(cond) check_impl((cond), #cond, __FILE__, __LINE__)
#define CHECK_MSG(cond, msg) check_impl((cond), #cond, __FILE__, __LINE__, (msg))

struct CallbackCollector {
  std::mutex mu;
  std::condition_variable cv;
  std::vector<jade::json::Value> events;
  int info_count = 0;
  int complete_count = 0;

  static void on_event(const char* payload_utf8, size_t len, void* user_data) {
    auto* self = static_cast<CallbackCollector*>(user_data);
    jade::json::Value val;
    std::string err;
    bool ok = jade::json::parse(std::string_view(payload_utf8, len), val, err);
    std::lock_guard<std::mutex> lock(self->mu);
    if (ok) {
      if (const auto* t = val.find("type")) {
        if (t->as_string_view() == "info") ++self->info_count;
        if (t->as_string_view() == "complete") ++self->complete_count;
      }
      self->events.push_back(std::move(val));
    }
    self->cv.notify_all();
  }

  bool wait_for_info(int min_count, int timeout_ms = 2000) {
    std::unique_lock<std::mutex> lock(mu);
    return cv.wait_for(lock, std::chrono::milliseconds(timeout_ms), [&]() {
      return info_count >= min_count;
    });
  }

  bool wait_for_complete(int min_count = 1, int timeout_ms = 5000) {
    std::unique_lock<std::mutex> lock(mu);
    return cv.wait_for(lock, std::chrono::milliseconds(timeout_ms), [&]() {
      return complete_count >= min_count;
    });
  }
};

void test_xiangqi_move_generation_rules() {
  using namespace jade::xiangqi;

  // 1. Startpos has exactly 44 legal moves.
  Position pos;
  pos.set_startpos();
  MoveList legal;
  pos.generate_legal(legal, false);
  CHECK_MSG(legal.count == 44, "expected 44 legal moves in Xiangqi startpos, got " + std::to_string(legal.count));

  // 2. Horse leg blocking: Red horse at e4 surrounded on 3 orthogonal legs (e5, d4, f4), open on e3.
  Position horse_pos;
  std::string err;
  CHECK(horse_pos.set_fen("3k5/9/9/9/4P4/3PNP3/9/9/9/4K4 w - - 0 1", &err));
  MoveList horse_moves;
  horse_pos.generate_legal(horse_moves, false);
  bool d2_ok = false;
  bool f2_ok = false;
  bool d6_bad = false;
  bool c5_bad = false;
  for (int i = 0; i < horse_moves.count; ++i) {
    std::string u = horse_moves[i].to_uci();
    if (u == "e4d2") d2_ok = true;
    if (u == "e4f2") f2_ok = true;
    if (u == "e4d6" || u == "e4f6") d6_bad = true;
    if (u == "e4c5" || u == "e4c3" || u == "e4g5" || u == "e4g3") c5_bad = true;
  }
  CHECK(d2_ok && f2_ok);
  CHECK(!d6_bad && !c5_bad);

  // 3. Elephant eye blocking & river barrier: Red elephant at c4 with eye b3 blocked by Red soldier;
  // cannot cross river to a6/e6, cannot jump to a2, can only jump to e2.
  Position ele_pos;
  CHECK(ele_pos.set_fen("4k4/9/9/9/9/2B6/1P7/9/9/3K5 w - - 0 1", &err));
  MoveList ele_moves;
  ele_pos.generate_legal(ele_moves, false);
  std::set<std::string> c4_targets;
  for (int i = 0; i < ele_moves.count; ++i) {
    std::string u = ele_moves[i].to_uci();
    if (u.substr(0, 2) == "c4") c4_targets.insert(u);
  }
  CHECK(c4_targets.size() == 1 && c4_targets.count("c4e2") == 1);

  // 4. Cannon screen capture vs double screen non-capture.
  // Red cannon on e2, screen on e5 (P), enemy horse on e7 (n), enemy chariot behind on e8 (r).
  Position can_pos;
  CHECK(can_pos.set_fen("3k5/4r4/4n4/9/4P4/9/9/4C4/9/4K4 w - - 0 1", &err));
  MoveList can_moves;
  can_pos.generate_legal(can_moves, false);
  bool captures_e7 = false;
  bool captures_e8 = false;
  bool slides_e5 = false;
  for (int i = 0; i < can_moves.count; ++i) {
    std::string u = can_moves[i].to_uci();
    if (u == "e2e7") captures_e7 = true;
    if (u == "e2e8") captures_e8 = true;
    if (u == "e2e5") slides_e5 = true;
  }
  CHECK(captures_e7);
  CHECK(!captures_e8);
  CHECK(!slides_e5);

  // 5. Flying general pin: Red horse at e4 is the sole piece between Red K (e0) and Black k (e9).
  // Moving the horse anywhere exposes flying general, so only Red General moves are legal.
  Position fg_pos;
  CHECK(fg_pos.set_fen("4k4/9/9/9/9/4N4/9/9/9/4K4 w - - 0 1", &err));
  MoveList fg_moves;
  fg_pos.generate_legal(fg_moves, false);
  for (int i = 0; i < fg_moves.count; ++i) {
    CHECK_MSG(fg_moves[i].to_uci().substr(0, 2) != "e4", "pinned horse on e4 must have 0 legal moves");
  }
  CHECK(fg_moves.count == 3);  // e0d0, e0f0, e0e1
}

void test_mate_in_one_and_tactics() {
  JadeEngineHandle* handle = nullptr;
  CHECK(jade_engine_create(&handle) == JADE_ENGINE_OK);
  CHECK(handle != nullptr);

  // Unique mate-in-1: Red plays a8a9# (d8 escape covered by Red soldier at c8).
  const std::string req = R"({
    "search_id": "mate-test-1",
    "state_id": "state-mate-1",
    "fen": "3ka4/R1P1a4/9/9/9/9/9/9/9/4K4 w - - 0 1",
    "limits": { "depth": 3 }
  })";

  CallbackCollector col;
  CHECK(jade_engine_search(handle, req.data(), req.size(), &CallbackCollector::on_event, &col) == JADE_ENGINE_OK);
  CHECK(col.wait_for_complete(1, 3000));

  std::lock_guard<std::mutex> lock(col.mu);
  CHECK(!col.events.empty());
  const auto& final_ev = col.events.back();
  CHECK(final_ev.find("type")->as_string_view() == "complete");
  CHECK(final_ev.find("status")->as_string_view() == "completed");
  CHECK(final_ev.find("search_id")->as_string_view() == "mate-test-1");
  CHECK(final_ev.find("state_id")->as_string_view() == "state-mate-1");
  CHECK(final_ev.find("best_move")->as_string_view() == "a8a9");

  const auto* score = final_ev.find("score");
  CHECK(score != nullptr && score->is_object());
  CHECK(score->find("kind")->as_string_view() == "mate");
  CHECK(score->find("value")->as_int() == 1);
  CHECK(score->find("red_value")->as_int() == 1);

  // Verify manifest hashes are 64-character lowercase hex SHA-256 strings.
  CHECK(final_ev.find("binary_hash")->as_string_view().size() == 64);
  CHECK(final_ev.find("config_hash")->as_string_view().size() == 64);
  CHECK(final_ev.find("network_hash")->as_string_view().size() == 64);
  CHECK(final_ev.find("rules_profile")->as_string_view() == "program-2023.11-v1");

  jade_engine_dispose(handle);

  // Tactical capture test: undefended Black chariot on e4 capturable by Red chariot on e1.
  JadeEngineHandle* tac_handle = nullptr;
  CHECK(jade_engine_create(&tac_handle) == JADE_ENGINE_OK);
  const std::string tac_req = R"({
    "search_id": "tactic-capture-1",
    "state_id": "state-tac-1",
    "fen": "3akab2/9/9/9/9/4r4/9/9/4R4/3AKAB2 w - - 0 1",
    "limits": { "depth": 4 }
  })";
  CallbackCollector tac_col;
  CHECK(jade_engine_search(tac_handle, tac_req.data(), tac_req.size(), &CallbackCollector::on_event, &tac_col) == JADE_ENGINE_OK);
  CHECK(tac_col.wait_for_complete(1, 3000));
  {
    std::lock_guard<std::mutex> tlock(tac_col.mu);
    const auto& tev = tac_col.events.back();
    CHECK(tev.find("best_move")->as_string_view() == "e1e4");
    CHECK(tev.find("score")->find("value")->as_int() >= 800);
  }
  jade_engine_dispose(tac_handle);
}

void test_streaming_pv_legality_and_multipv() {
  JadeEngineHandle* handle = nullptr;
  CHECK(jade_engine_create(&handle) == JADE_ENGINE_OK);

  const std::string cfg = R"({"hash_mb": 8, "multi_pv": 3})";
  CHECK(jade_engine_configure(handle, cfg.data(), cfg.size()) == JADE_ENGINE_OK);

  const std::string req = R"({
    "search_id": "mpv-search-42",
    "state_id": "board-rev-7",
    "fen": "startpos",
    "moves": ["h2e2", "h9g7"],
    "limits": { "depth": 3 }
  })";

  CallbackCollector col;
  auto t0 = std::chrono::steady_clock::now();
  CHECK(jade_engine_search(handle, req.data(), req.size(), &CallbackCollector::on_event, &col) == JADE_ENGINE_OK);
  auto enqueue_us = std::chrono::duration_cast<std::chrono::microseconds>(
                        std::chrono::steady_clock::now() - t0)
                        .count();
  CHECK_MSG(enqueue_us < 10000, "jade_engine_search must not block caller thread");

  CHECK(col.wait_for_complete(1, 5000));

  jade::xiangqi::Position base_pos;
  base_pos.set_startpos();
  CHECK(base_pos.apply_uci_move("h2e2"));
  CHECK(base_pos.apply_uci_move("h9g7"));

  std::lock_guard<std::mutex> lock(col.mu);
  CHECK(col.info_count >= 3);
  for (const auto& ev : col.events) {
    CHECK(ev.find("search_id")->as_string_view() == "mpv-search-42");
    CHECK(ev.find("state_id")->as_string_view() == "board-rev-7");

    // Verify every PV move in every info/complete callback is strictly legal on the board.
    const auto* pv_arr = ev.find("pv")->as_array();
    CHECK(pv_arr != nullptr && !pv_arr->empty());
    jade::xiangqi::Position sim = base_pos;
    for (const auto& mv_val : *pv_arr) {
      CHECK_MSG(sim.apply_uci_move(mv_val.as_string_view()),
                "illegal continuation move in PV: " + std::string(mv_val.as_string_view()));
    }
  }

  const auto& done = col.events.back();
  const auto* lines = done.find("lines")->as_array();
  CHECK(lines != nullptr && lines->size() == 3);
  std::set<std::string> distinct_roots;
  for (const auto& l : *lines) {
    distinct_roots.insert(std::string(l.find("best_move")->as_string_view()));
  }
  CHECK_MSG(distinct_roots.size() == 3, "MultiPV=3 must produce 3 distinct legal root moves");

  jade_engine_dispose(handle);
}

void test_infinite_search_fast_cancellation_and_reentrant_cancel() {
  JadeEngineHandle* handle = nullptr;
  CHECK(jade_engine_create(&handle) == JADE_ENGINE_OK);

  const std::string req = R"({
    "search_id": "inf-cancel-1",
    "state_id": "state-inf-1",
    "fen": "startpos",
    "limits": { "infinite": true }
  })";

  CallbackCollector col;
  CHECK(jade_engine_search(handle, req.data(), req.size(), &CallbackCollector::on_event, &col) == JADE_ENGINE_OK);
  CHECK_MSG(col.wait_for_info(1, 2000), "expected streaming info before cancel");

  auto cancel_start = std::chrono::steady_clock::now();
  CHECK(jade_engine_cancel(handle) == JADE_ENGINE_OK);
  auto cancel_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                       std::chrono::steady_clock::now() - cancel_start)
                       .count();
  CHECK_MSG(cancel_ms < 20, "cancellation took " + std::to_string(cancel_ms) + " ms (expected < 20 ms)");

  CHECK(col.wait_for_complete(1, 1000));
  {
    std::lock_guard<std::mutex> lock(col.mu);
    const auto& last = col.events.back();
    CHECK(last.find("type")->as_string_view() == "complete");
    CHECK(last.find("cancelled")->as_bool() == true);
    CHECK(!last.find("best_move")->is_null());

    jade::xiangqi::Position start_pos;
    start_pos.set_startpos();
    CHECK(start_pos.apply_uci_move(last.find("best_move")->as_string_view()));
  }

  // Test re-entrant cancellation directly from inside JadeEngineCallback without deadlock.
  struct ReentrantCtx {
    JadeEngineHandle* h = nullptr;
    std::atomic<bool> completed{false};
    std::condition_variable cv;
    std::mutex mu;
  } rctx;
  rctx.h = handle;

  const std::string req2 = R"({
    "search_id": "inf-reentrant-2",
    "state_id": "state-inf-2",
    "fen": "startpos",
    "limits": { "infinite": true }
  })";

  auto reentrant_cb = [](const char* payload_utf8, size_t len, void* ud) {
    auto* ctx = static_cast<ReentrantCtx*>(ud);
    jade::json::Value v;
    std::string err;
    if (!jade::json::parse(std::string_view(payload_utf8, len), v, err)) return;
    auto type_sv = v.find("type")->as_string_view();
    if (type_sv == "info" && v.find("depth")->as_int() >= 2) {
      jade_engine_cancel(ctx->h);
    } else if (type_sv == "complete") {
      std::lock_guard<std::mutex> lk(ctx->mu);
      ctx->completed.store(true);
      ctx->cv.notify_all();
    }
  };

  CHECK(jade_engine_search(handle, req2.data(), req2.size(), reentrant_cb, &rctx) == JADE_ENGINE_OK);
  {
    std::unique_lock<std::mutex> lk(rctx.mu);
    bool finished = rctx.cv.wait_for(lk, std::chrono::milliseconds(2000), [&]() {
      return rctx.completed.load();
    });
    CHECK_MSG(finished, "re-entrant jade_engine_cancel inside callback must not deadlock");
  }

  jade_engine_dispose(handle);
}

void test_superseding_search_suppresses_stale_updates() {
  JadeEngineHandle* handle = nullptr;
  CHECK(jade_engine_create(&handle) == JADE_ENGINE_OK);

  CallbackCollector col;
  const std::string req_old = R"({
    "search_id": "stale-search-old",
    "state_id": "state-old",
    "fen": "startpos",
    "limits": { "infinite": true }
  })";
  CHECK(jade_engine_search(handle, req_old.data(), req_old.size(), &CallbackCollector::on_event, &col) == JADE_ENGINE_OK);
  CHECK(col.wait_for_info(1, 2000));

  // Immediately supersede with a bounded search on a new board state!
  const std::string req_new = R"({
    "search_id": "fresh-search-new",
    "state_id": "state-new",
    "fen": "startpos",
    "moves": ["b0c2"],
    "limits": { "depth": 4 }
  })";
  CHECK(jade_engine_search(handle, req_new.data(), req_new.size(), &CallbackCollector::on_event, &col) == JADE_ENGINE_OK);

  // Wait until `fresh-search-new` completes.
  {
    std::unique_lock<std::mutex> lock(col.mu);
    bool new_done = col.cv.wait_for(lock, std::chrono::milliseconds(4000), [&]() {
      for (const auto& ev : col.events) {
        if (ev.find("type")->as_string_view() == "complete" &&
            ev.find("search_id")->as_string_view() == "fresh-search-new") {
          return true;
        }
      }
      return false;
    });
    CHECK_MSG(new_done, "superseding search fresh-search-new must complete");

    bool saw_superseded_tag = false;
    bool saw_first_new_event = false;
    for (const auto& ev : col.events) {
      std::string_view sid = ev.find("search_id")->as_string_view();
      std::string_view ty = ev.find("type")->as_string_view();
      if (sid == "fresh-search-new") {
        saw_first_new_event = true;
        CHECK(ev.find("state_id")->as_string_view() == "state-new");
        CHECK(ev.find("stale")->as_bool() == false);
      } else if (sid == "stale-search-old") {
        CHECK(ev.find("state_id")->as_string_view() == "state-old");
        if (ty == "complete") {
          CHECK(ev.find("superseded")->as_bool() == true);
          CHECK(ev.find("stale")->as_bool() == true);
          saw_superseded_tag = true;
        }
        if (saw_first_new_event) {
          CHECK_MSG(false, "no old-search events may arrive after fresh-search-new begins");
        }
      }
    }
    CHECK(saw_superseded_tag);
  }

  jade_engine_dispose(handle);
}

void test_multi_instance_isolation_and_error_surface() {
  JadeEngineHandle* h1 = nullptr;
  JadeEngineHandle* h2 = nullptr;
  CHECK(jade_engine_create(&h1) == JADE_ENGINE_OK);
  CHECK(jade_engine_create(&h2) == JADE_ENGINE_OK);

  const std::string cfg1 = R"({"hash_mb": 4, "engine_id": "instance-alpha"})";
  const std::string cfg2 = R"({"hash_mb": 8, "engine_id": "instance-beta"})";
  CHECK(jade_engine_configure(h1, cfg1.data(), cfg1.size()) == JADE_ENGINE_OK);
  CHECK(jade_engine_configure(h2, cfg2.data(), cfg2.size()) == JADE_ENGINE_OK);

  CallbackCollector c1;
  CallbackCollector c2;
  const std::string r1 = R"({
    "search_id": "iso-1",
    "state_id": "s-1",
    "fen": "3ka4/R1P1a4/9/9/9/9/9/9/9/4K4 w - - 0 1",
    "limits": { "depth": 3 }
  })";
  const std::string r2 = R"({
    "search_id": "iso-2",
    "state_id": "s-2",
    "fen": "startpos",
    "moves": ["h2e2"],
    "limits": { "depth": 4 }
  })";

  CHECK(jade_engine_search(h1, r1.data(), r1.size(), &CallbackCollector::on_event, &c1) == JADE_ENGINE_OK);
  CHECK(jade_engine_search(h2, r2.data(), r2.size(), &CallbackCollector::on_event, &c2) == JADE_ENGINE_OK);

  CHECK(c1.wait_for_complete(1, 4000));
  CHECK(c2.wait_for_complete(1, 4000));

  {
    std::lock_guard<std::mutex> l1(c1.mu);
    std::lock_guard<std::mutex> l2(c2.mu);
    const auto& d1 = c1.events.back();
    const auto& d2 = c2.events.back();
    CHECK(d1.find("engine_id")->as_string_view() == "instance-alpha");
    CHECK(d2.find("engine_id")->as_string_view() == "instance-beta");
    CHECK(d1.find("config_hash")->as_string_view() != d2.find("config_hash")->as_string_view());
    CHECK(d1.find("best_move")->as_string_view() == "a8a9");
    CHECK(d2.find("side_to_move")->as_string_view() == "black");
  }

  // Test error surface (invalid JSON, illegal move) and jade_engine_last_error.
  const std::string bad_json = "{not valid json";
  CHECK(jade_engine_configure(h1, bad_json.data(), bad_json.size()) == JADE_ENGINE_ERR_INVALID_JSON);
  char err_buf[256]{};
  size_t err_len = jade_engine_last_error(h1, err_buf, sizeof(err_buf));
  CHECK(err_len > 0 && std::string_view(err_buf).find("invalid options JSON") != std::string_view::npos);

  const std::string illegal_move_req = R"({
    "search_id": "bad-move",
    "state_id": "s-bad",
    "fen": "startpos",
    "moves": ["a0a9"],
    "limits": { "depth": 2 }
  })";
  CHECK(jade_engine_search(h1, illegal_move_req.data(), illegal_move_req.size(),
                           &CallbackCollector::on_event, &c1) == JADE_ENGINE_ERR_INVALID_POSITION);
  err_len = jade_engine_last_error(h1, err_buf, sizeof(err_buf));
  CHECK(err_len > 0 && std::string_view(err_buf).find("illegal Xiangqi move") != std::string_view::npos);

  jade_engine_dispose(h1);
  jade_engine_dispose(h2);
  jade_engine_dispose(nullptr);  // Safe no-op.
}

}  // namespace

int main() {
  std::cout << "[engine_bridge_test] Running Xiangqi move generation & rules tests...\n";
  test_xiangqi_move_generation_rules();

  std::cout << "[engine_bridge_test] Running mate-in-1 & tactical capture tests...\n";
  test_mate_in_one_and_tactics();

  std::cout << "[engine_bridge_test] Running streaming PV legality & MultiPV tests...\n";
  test_streaming_pv_legality_and_multipv();

  std::cout << "[engine_bridge_test] Running infinite search <20ms cancel & reentrant cancel tests...\n";
  test_infinite_search_fast_cancellation_and_reentrant_cancel();

  std::cout << "[engine_bridge_test] Running superseding search stale-suppression tests...\n";
  test_superseding_search_suppresses_stale_updates();

  std::cout << "[engine_bridge_test] Running multi-instance isolation & FFI error tests...\n";
  test_multi_instance_isolation_and_error_surface();

  if (g_failures > 0) {
    std::cerr << "[engine_bridge_test] FAILED with " << g_failures << " error(s).\n";
    return EXIT_FAILURE;
  }
  std::cout << "[engine_bridge_test] ALL TESTS PASSED.\n";
  return EXIT_SUCCESS;
}
