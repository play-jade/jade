// SPDX-License-Identifier: Apache-2.0
#define JADE_ENGINE_BUILD_SHARED 1
#include "jade_engine.h"

#include "json_mini.hpp"
#include "sha256.hpp"
#include "xiangqi_position.hpp"
#include "xiangqi_search.hpp"

#include <algorithm>
#include <atomic>
#include <condition_variable>
#include <cstring>
#include <exception>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

namespace {

using jade::crypto::Sha256;
using jade::xiangqi::Color;
using jade::xiangqi::EngineOptions;
using jade::xiangqi::NormalizedScore;
using jade::xiangqi::Position;
using jade::xiangqi::Searcher;
using jade::xiangqi::SearchLimits;
using jade::xiangqi::SearchPvLine;
using jade::xiangqi::SearchResult;
using jade::xiangqi::SearchUpdate;

inline constexpr std::string_view kEngineBuildTag =
    "jade-mobile-bridge-cpp26:0.1.0:program-2023.11-v1";

std::string compute_config_hash(const EngineOptions& opts) {
  std::string canonical;
  canonical.reserve(256);
  canonical += "engine_id=";
  canonical += opts.engine_id;
  canonical += ";rules_profile=";
  canonical += opts.rules_profile;
  canonical += ";network_hash=";
  canonical += opts.network_hash;
  canonical += ";hash_mb=";
  canonical += std::to_string(opts.hash_mb);
  canonical += ";threads=";
  canonical += std::to_string(opts.threads);
  canonical += ";multi_pv=";
  canonical += std::to_string(opts.multi_pv);
  canonical += ";move_overhead_ms=";
  canonical += std::to_string(opts.move_overhead_ms);
  return Sha256::hex_digest(canonical);
}

void append_score_json(std::string& out, const NormalizedScore& score) {
  out += "{\"kind\":";
  jade::json::append_escaped_string(out, score.is_mate ? "mate" : "cp");
  out += ",\"value\":";
  out += std::to_string(score.value);
  out += ",\"red_value\":";
  out += std::to_string(score.red_value);
  out += ",\"mate\":";
  if (score.is_mate) {
    out += std::to_string(score.value);
  } else {
    out += "null";
  }
  out += ",\"perspective\":\"side_to_move\"}";
}

void append_pv_array(std::string& out, const std::vector<std::string>& pv) {
  out.push_back('[');
  for (size_t i = 0; i < pv.size(); ++i) {
    if (i > 0) out.push_back(',');
    jade::json::append_escaped_string(out, pv[i]);
  }
  out.push_back(']');
}

void append_manifest_fields(
    std::string& out,
    const EngineOptions& opts,
    std::string_view binary_hash,
    std::string_view config_hash) {
  out += ",\"engine_id\":";
  jade::json::append_escaped_string(out, opts.engine_id);
  out += ",\"binary_hash\":";
  jade::json::append_escaped_string(out, binary_hash);
  out += ",\"network_hash\":";
  jade::json::append_escaped_string(out, opts.network_hash);
  out += ",\"rules_profile\":";
  jade::json::append_escaped_string(out, opts.rules_profile);
  out += ",\"config_hash\":";
  jade::json::append_escaped_string(out, config_hash);
  out += ",\"manifest\":{\"engine_id\":";
  jade::json::append_escaped_string(out, opts.engine_id);
  out += ",\"binary_hash\":";
  jade::json::append_escaped_string(out, binary_hash);
  out += ",\"network_hash\":";
  jade::json::append_escaped_string(out, opts.network_hash);
  out += ",\"rules_profile\":";
  jade::json::append_escaped_string(out, opts.rules_profile);
  out += ",\"config_hash\":";
  jade::json::append_escaped_string(out, config_hash);
  out.push_back('}');
}

struct SearchJob {
  uint64_t generation = 0;
  std::string search_id;
  std::string state_id;
  Position position;
  SearchLimits limits;
  JadeEngineCallback on_info = nullptr;
  void* user_data = nullptr;
};

}  // namespace

struct JadeEngineHandle {
  mutable std::mutex error_mutex;
  std::string last_error;

  std::mutex queue_mutex;
  std::condition_variable queue_cv;
  std::condition_variable idle_cv;

  std::atomic<uint64_t> active_generation{0};
  std::atomic<bool> stop_requested{false};
  std::atomic<bool> disposed{false};

  EngineOptions current_options{};
  std::optional<EngineOptions> staged_options;
  std::string binary_hash;
  std::string config_hash;

  std::optional<SearchJob> pending_job;
  bool worker_busy = false;

  Searcher searcher{};
  std::thread worker_thread;
  std::thread::id worker_tid{};

  JadeEngineHandle() {
    binary_hash = Sha256::hex_digest(kEngineBuildTag);
    if (current_options.network_hash.size() != 64) {
      current_options.network_hash = Sha256::hex_digest("builtin-hce-v1");
    } else {
      current_options.network_hash = Sha256::hex_digest("builtin-hce-v1");
    }
    config_hash = compute_config_hash(current_options);
    searcher.configure(current_options);
    worker_thread = std::thread([this]() { worker_loop(); });
  }

  void set_error(std::string msg) {
    std::lock_guard<std::mutex> lock(error_mutex);
    last_error = std::move(msg);
  }

  void clear_error() {
    std::lock_guard<std::mutex> lock(error_mutex);
    last_error.clear();
  }

  void worker_loop() {
    {
      std::lock_guard<std::mutex> lock(queue_mutex);
      worker_tid = std::this_thread::get_id();
    }

    while (true) {
      std::optional<SearchJob> job;
      EngineOptions job_opts;
      std::string job_binary_hash;
      std::string job_config_hash;

      {
        std::unique_lock<std::mutex> lock(queue_mutex);
        queue_cv.wait(lock, [this]() {
          return disposed.load(std::memory_order_acquire) ||
                 staged_options.has_value() ||
                 pending_job.has_value();
        });

        if (disposed.load(std::memory_order_acquire)) {
          worker_busy = false;
          idle_cv.notify_all();
          return;
        }

        if (staged_options.has_value()) {
          current_options = std::move(*staged_options);
          staged_options.reset();
          config_hash = compute_config_hash(current_options);
          searcher.configure(current_options);
          if (!pending_job.has_value()) {
            idle_cv.notify_all();
            continue;
          }
        }

        if (!pending_job.has_value()) {
          continue;
        }

        job = std::move(pending_job);
        pending_job.reset();
        worker_busy = true;

        // Clear stop flag only if this job is still the latest active generation.
        if (active_generation.load(std::memory_order_acquire) == job->generation) {
          stop_requested.store(false, std::memory_order_release);
        } else {
          stop_requested.store(true, std::memory_order_release);
        }

        job_opts = current_options;
        job_binary_hash = binary_hash;
        job_config_hash = config_hash;
      }

      run_job(*job, job_opts, job_binary_hash, job_config_hash);

      {
        std::lock_guard<std::mutex> lock(queue_mutex);
        worker_busy = false;
        if (!pending_job.has_value() && !staged_options.has_value()) {
          idle_cv.notify_all();
        }
      }
    }
  }

  void run_job(
      const SearchJob& job,
      const EngineOptions& opts,
      std::string_view bin_hash,
      std::string_view cfg_hash) {
    const uint64_t gen = job.generation;

    auto is_stale = [this, gen]() -> bool {
      return disposed.load(std::memory_order_relaxed) ||
             (active_generation.load(std::memory_order_acquire) != gen);
    };

    if (disposed.load(std::memory_order_acquire)) {
      return;
    }

    // If superseded before even starting, emit a tagged superseded frame and skip work.
    if (active_generation.load(std::memory_order_acquire) != gen) {
      if (job.on_info != nullptr && !disposed.load(std::memory_order_acquire)) {
        std::string payload;
        payload.reserve(512);
        payload += "{\"type\":\"complete\",\"status\":\"superseded\",\"cancelled\":true,"
                   "\"superseded\":true,\"stale\":true,\"search_id\":";
        jade::json::append_escaped_string(payload, job.search_id);
        payload += ",\"state_id\":";
        jade::json::append_escaped_string(payload, job.state_id);
        payload += ",\"depth\":0,\"seldepth\":0,\"nodes\":0,\"nps\":0,\"time_ms\":0,\"side_to_move\":";
        jade::json::append_escaped_string(
            payload, job.position.side_to_move() == Color::Red ? "red" : "black");
        payload += ",\"score\":{\"kind\":\"cp\",\"value\":0,\"red_value\":0,\"mate\":null,"
                   "\"perspective\":\"side_to_move\"},\"best_move\":null,\"pv\":[],\"lines\":[]";
        append_manifest_fields(payload, opts, bin_hash, cfg_hash);
        payload.push_back('}');
        job.on_info(payload.c_str(), payload.size(), job.user_data);
      }
      return;
    }

    SearchResult result = searcher.search(
        job.position,
        job.limits,
        &stop_requested,
        is_stale,
        [&](const SearchUpdate& upd) {
          // Never emit stale streaming `info` frames after cancellation or supersession.
          if (stop_requested.load(std::memory_order_acquire) || is_stale()) {
            return;
          }
          if (job.on_info == nullptr) return;

          std::string payload;
          payload.reserve(640);
          payload += "{\"type\":\"info\",\"stale\":false,\"superseded\":false,\"search_id\":";
          jade::json::append_escaped_string(payload, job.search_id);
          payload += ",\"state_id\":";
          jade::json::append_escaped_string(payload, job.state_id);
          payload += ",\"depth\":";
          payload += std::to_string(upd.depth);
          payload += ",\"seldepth\":";
          payload += std::to_string(upd.seldepth);
          payload += ",\"multipv\":";
          payload += std::to_string(upd.line.multipv);
          payload += ",\"nodes\":";
          payload += std::to_string(upd.nodes);
          payload += ",\"nps\":";
          payload += std::to_string(upd.nps);
          payload += ",\"time_ms\":";
          payload += std::to_string(upd.time_ms);
          payload += ",\"hashfull\":";
          payload += std::to_string(upd.hashfull);
          payload += ",\"side_to_move\":";
          jade::json::append_escaped_string(
              payload, upd.side_to_move == Color::Red ? "red" : "black");
          payload += ",\"score\":";
          append_score_json(payload, upd.line.score);
          payload += ",\"best_move\":";
          if (upd.line.best_move.is_none()) {
            payload += "null";
          } else {
            jade::json::append_escaped_string(payload, upd.line.best_move.to_uci());
          }
          payload += ",\"pv\":";
          append_pv_array(payload, upd.line.pv);
          append_manifest_fields(payload, opts, bin_hash, cfg_hash);
          payload.push_back('}');

          if (!stop_requested.load(std::memory_order_acquire) && !is_stale()) {
            job.on_info(payload.c_str(), payload.size(), job.user_data);
          }
        });

    if (disposed.load(std::memory_order_acquire) || job.on_info == nullptr) {
      return;
    }

    // Determine whether this job finished normally, was explicitly cancelled, or was superseded.
    bool superseded = false;
    bool cancelled = false;
    {
      std::lock_guard<std::mutex> lock(queue_mutex);
      if (pending_job.has_value()) {
        superseded = true;
        cancelled = true;
      } else if (active_generation.load(std::memory_order_relaxed) != gen ||
                 (stop_requested.load(std::memory_order_relaxed) && job.limits.infinite) ||
                 (stop_requested.load(std::memory_order_relaxed) && result.aborted)) {
        cancelled = true;
      }
    }

    std::string_view status_str =
        superseded ? "superseded" : (cancelled ? "cancelled" : "completed");

    std::string payload;
    payload.reserve(768);
    payload += "{\"type\":\"complete\",\"status\":";
    jade::json::append_escaped_string(payload, status_str);
    payload += ",\"cancelled\":";
    payload += (cancelled ? "true" : "false");
    payload += ",\"superseded\":";
    payload += (superseded ? "true" : "false");
    payload += ",\"stale\":";
    payload += (superseded ? "true" : "false");
    payload += ",\"search_id\":";
    jade::json::append_escaped_string(payload, job.search_id);
    payload += ",\"state_id\":";
    jade::json::append_escaped_string(payload, job.state_id);
    payload += ",\"depth\":";
    payload += std::to_string(result.completed_depth);
    payload += ",\"seldepth\":";
    payload += std::to_string(result.seldepth);
    payload += ",\"nodes\":";
    payload += std::to_string(result.nodes);
    payload += ",\"nps\":";
    payload += std::to_string(result.nps);
    payload += ",\"time_ms\":";
    payload += std::to_string(result.time_ms);
    payload += ",\"hashfull\":";
    payload += std::to_string(result.hashfull);
    payload += ",\"side_to_move\":";
    jade::json::append_escaped_string(
        payload, result.side_to_move == Color::Red ? "red" : "black");
    payload += ",\"score\":";
    append_score_json(payload, result.score);
    payload += ",\"best_move\":";
    if (result.best_move.is_none()) {
      payload += "null";
    } else {
      jade::json::append_escaped_string(payload, result.best_move.to_uci());
    }
    payload += ",\"pv\":";
    append_pv_array(payload, result.pv);
    payload += ",\"lines\":[";
    for (size_t i = 0; i < result.lines.size(); ++i) {
      if (i > 0) payload.push_back(',');
      const SearchPvLine& l = result.lines[i];
      payload += "{\"multipv\":";
      payload += std::to_string(l.multipv);
      payload += ",\"depth\":";
      payload += std::to_string(l.depth);
      payload += ",\"seldepth\":";
      payload += std::to_string(l.seldepth);
      payload += ",\"score\":";
      append_score_json(payload, l.score);
      payload += ",\"best_move\":";
      if (l.best_move.is_none()) {
        payload += "null";
      } else {
        jade::json::append_escaped_string(payload, l.best_move.to_uci());
      }
      payload += ",\"pv\":";
      append_pv_array(payload, l.pv);
      payload.push_back('}');
    }
    payload.push_back(']');
    append_manifest_fields(payload, opts, bin_hash, cfg_hash);
    payload.push_back('}');

    job.on_info(payload.c_str(), payload.size(), job.user_data);
  }
};

namespace {

bool parse_configure_options(
    const jade::json::Value& root,
    EngineOptions& opts,
    std::string& err) {
  if (!root.is_object()) {
    err = "configure options payload must be a JSON object";
    return false;
  }

  auto read_int = [&](std::initializer_list<std::string_view> keys, int& target) -> bool {
    for (std::string_view k : keys) {
      if (const auto* v = root.find(k)) {
        if (!v->is_number()) {
          err = "expected numeric option for key: " + std::string(k);
          return false;
        }
        target = static_cast<int>(v->as_int(target));
        return true;
      }
    }
    return true;
  };

  if (!read_int({"hash_mb", "Hash", "hash"}, opts.hash_mb)) return false;
  if (!read_int({"threads", "Threads"}, opts.threads)) return false;
  if (!read_int({"multi_pv", "MultiPV", "multipv"}, opts.multi_pv)) return false;
  if (!read_int({"move_overhead_ms", "Move Overhead"}, opts.move_overhead_ms)) return false;

  if (const auto* rp = root.find("rules_profile")) {
    if (!rp->is_string() || rp->as_string_view().empty()) {
      err = "rules_profile must be a non-empty string";
      return false;
    }
    opts.rules_profile = std::string(rp->as_string_view());
  }
  if (const auto* eid = root.find("engine_id")) {
    if (!eid->is_string() || eid->as_string_view().empty()) {
      err = "engine_id must be a non-empty string";
      return false;
    }
    opts.engine_id = std::string(eid->as_string_view());
  }
  if (const auto* nh = root.find("network_hash")) {
    if (!nh->is_string() || nh->as_string_view().empty()) {
      err = "network_hash must be a non-empty string";
      return false;
    }
    std::string_view sv = nh->as_string_view();
    opts.network_hash = (sv.size() == 64) ? std::string(sv) : Sha256::hex_digest(sv);
  }

  opts.hash_mb = std::clamp(opts.hash_mb, 1, 512);
  opts.threads = std::clamp(opts.threads, 1, 64);
  opts.multi_pv = std::clamp(opts.multi_pv, 1, 16);
  opts.move_overhead_ms = std::clamp(opts.move_overhead_ms, 0, 5000);
  return true;
}

int32_t parse_search_job(
    const jade::json::Value& root,
    SearchJob& job,
    std::string& err) {
  if (!root.is_object()) {
    err = "search request must be a JSON object";
    return JADE_ENGINE_ERR_INVALID_JSON;
  }

  const auto* sid = root.find("search_id");
  if (!sid || !sid->is_string() || sid->as_string_view().empty()) {
    err = "search request requires non-empty string field 'search_id'";
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  const auto* stid = root.find("state_id");
  if (!stid || !stid->is_string() || stid->as_string_view().empty()) {
    err = "search request requires non-empty string field 'state_id'";
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  job.search_id = std::string(sid->as_string_view());
  job.state_id = std::string(stid->as_string_view());

  std::string_view fen_sv = "startpos";
  if (const auto* f = root.find("fen")) {
    if (!f->is_string() || f->as_string_view().empty()) {
      err = "'fen' must be a non-empty string";
      return JADE_ENGINE_ERR_INVALID_POSITION;
    }
    fen_sv = f->as_string_view();
  }
  if (!job.position.set_fen(fen_sv, &err)) {
    return JADE_ENGINE_ERR_INVALID_POSITION;
  }

  if (const auto* moves_val = root.find("moves")) {
    const auto* arr = moves_val->as_array();
    if (!arr) {
      err = "'moves' must be a JSON array of UCI strings";
      return JADE_ENGINE_ERR_INVALID_POSITION;
    }
    for (const auto& elem : *arr) {
      if (!elem.is_string()) {
        err = "every element of 'moves' must be a string";
        return JADE_ENGINE_ERR_INVALID_POSITION;
      }
      if (!job.position.apply_uci_move(elem.as_string_view(), &err)) {
        return JADE_ENGINE_ERR_INVALID_POSITION;
      }
    }
  }

  const jade::json::Value* lim_obj = root.find("limits");
  if (lim_obj && !lim_obj->is_object() && !lim_obj->is_null()) {
    err = "'limits' must be a JSON object";
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }

  auto lookup_field = [&](std::string_view k) -> const jade::json::Value* {
    if (lim_obj && lim_obj->is_object()) {
      if (const auto* v = lim_obj->find(k)) return v;
    }
    return root.find(k);
  };

  bool any_limit = false;
  if (const auto* inf = lookup_field("infinite")) {
    if (inf->is_bool() && inf->as_bool()) {
      job.limits.infinite = true;
      any_limit = true;
    }
  }
  if (const auto* d = lookup_field("depth")) {
    if (d->is_number()) {
      job.limits.max_depth = static_cast<int>(d->as_int(0));
      any_limit = true;
    }
  }
  if (const auto* mt = lookup_field("movetime_ms")) {
    if (mt->is_number()) {
      job.limits.movetime_ms = mt->as_int(0);
      any_limit = true;
    }
  } else if (const auto* mt_alt = lookup_field("movetime")) {
    if (mt_alt->is_number()) {
      job.limits.movetime_ms = mt_alt->as_int(0);
      any_limit = true;
    }
  }
  if (const auto* nd = lookup_field("nodes")) {
    if (nd->is_number() && nd->as_int(0) > 0) {
      job.limits.max_nodes = static_cast<uint64_t>(nd->as_int(0));
      any_limit = true;
    }
  }
  if (const auto* mpv = lookup_field("multi_pv")) {
    if (mpv->is_number()) {
      job.limits.multi_pv = static_cast<int>(mpv->as_int(0));
    }
  }
  if (!any_limit) {
    job.limits.max_depth = 6;
  }

  return JADE_ENGINE_OK;
}

}  // namespace

extern "C" {

int32_t jade_engine_create(JadeEngineHandle** out_handle) {
  if (out_handle == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  *out_handle = nullptr;
  try {
    *out_handle = new JadeEngineHandle();
    return JADE_ENGINE_OK;
  } catch (...) {
    *out_handle = nullptr;
    return JADE_ENGINE_ERR_INTERNAL;
  }
}

int32_t jade_engine_configure(
    JadeEngineHandle* handle,
    const char* options_json_utf8,
    size_t len) {
  if (handle == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  try {
    if (handle->disposed.load(std::memory_order_acquire)) {
      handle->set_error("engine handle has been disposed");
      return JADE_ENGINE_ERR_DISPOSED;
    }
    if (options_json_utf8 == nullptr) {
      handle->set_error("options_json_utf8 must not be null");
      return JADE_ENGINE_ERR_INVALID_ARGUMENT;
    }

    std::string_view json_sv(options_json_utf8, len);
    jade::json::Value root;
    std::string err;
    if (!jade::json::parse(json_sv, root, err)) {
      handle->set_error("invalid options JSON: " + err);
      return JADE_ENGINE_ERR_INVALID_JSON;
    }

    std::unique_lock<std::mutex> lock(handle->queue_mutex);
    EngineOptions next_opts = handle->staged_options.value_or(handle->current_options);
    if (!parse_configure_options(root, next_opts, err)) {
      lock.unlock();
      handle->set_error(err);
      return JADE_ENGINE_ERR_INVALID_ARGUMENT;
    }

    // Cancel any active or queued search before reconfiguring options / TT size.
    ++handle->active_generation;
    handle->stop_requested.store(true, std::memory_order_release);
    handle->pending_job.reset();
    handle->staged_options = std::move(next_opts);
    handle->queue_cv.notify_one();

    if (std::this_thread::get_id() != handle->worker_tid) {
      handle->idle_cv.wait(lock, [handle]() {
        return !handle->worker_busy && !handle->staged_options.has_value();
      });
    }

    lock.unlock();
    handle->clear_error();
    return JADE_ENGINE_OK;
  } catch (const std::exception& e) {
    handle->set_error(std::string("configure exception: ") + e.what());
    return JADE_ENGINE_ERR_INTERNAL;
  } catch (...) {
    handle->set_error("unknown exception in jade_engine_configure");
    return JADE_ENGINE_ERR_INTERNAL;
  }
}

int32_t jade_engine_search(
    JadeEngineHandle* handle,
    const char* request_json_utf8,
    size_t len,
    JadeEngineCallback on_info,
    void* user_data) {
  if (handle == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  try {
    if (handle->disposed.load(std::memory_order_acquire)) {
      handle->set_error("engine handle has been disposed");
      return JADE_ENGINE_ERR_DISPOSED;
    }
    if (request_json_utf8 == nullptr || on_info == nullptr) {
      handle->set_error("request_json_utf8 and on_info callback must not be null");
      return JADE_ENGINE_ERR_INVALID_ARGUMENT;
    }

    std::string_view json_sv(request_json_utf8, len);
    jade::json::Value root;
    std::string err;
    if (!jade::json::parse(json_sv, root, err)) {
      handle->set_error("invalid search request JSON: " + err);
      return JADE_ENGINE_ERR_INVALID_JSON;
    }

    SearchJob job{};
    int32_t parse_status = parse_search_job(root, job, err);
    if (parse_status != JADE_ENGINE_OK) {
      handle->set_error(err);
      return parse_status;
    }
    job.on_info = on_info;
    job.user_data = user_data;

    {
      std::lock_guard<std::mutex> lock(handle->queue_mutex);
      uint64_t next_gen = handle->active_generation.fetch_add(1, std::memory_order_acq_rel) + 1;
      job.generation = next_gen;
      handle->stop_requested.store(true, std::memory_order_release);
      handle->pending_job = std::move(job);
      handle->queue_cv.notify_one();
    }

    handle->clear_error();
    return JADE_ENGINE_OK;
  } catch (const std::exception& e) {
    handle->set_error(std::string("search exception: ") + e.what());
    return JADE_ENGINE_ERR_INTERNAL;
  } catch (...) {
    handle->set_error("unknown exception in jade_engine_search");
    return JADE_ENGINE_ERR_INTERNAL;
  }
}

int32_t jade_engine_cancel(JadeEngineHandle* handle) {
  if (handle == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  try {
    if (handle->disposed.load(std::memory_order_acquire)) {
      handle->set_error("engine handle has been disposed");
      return JADE_ENGINE_ERR_DISPOSED;
    }

    std::unique_lock<std::mutex> lock(handle->queue_mutex);
    handle->active_generation.fetch_add(1, std::memory_order_acq_rel);
    handle->stop_requested.store(true, std::memory_order_release);
    handle->pending_job.reset();
    handle->queue_cv.notify_all();

    if (std::this_thread::get_id() != handle->worker_tid) {
      handle->idle_cv.wait(lock, [handle]() {
        return !handle->worker_busy && !handle->pending_job.has_value();
      });
    }

    lock.unlock();
    handle->clear_error();
    return JADE_ENGINE_OK;
  } catch (const std::exception& e) {
    handle->set_error(std::string("cancel exception: ") + e.what());
    return JADE_ENGINE_ERR_INTERNAL;
  } catch (...) {
    handle->set_error("unknown exception in jade_engine_cancel");
    return JADE_ENGINE_ERR_INTERNAL;
  }
}

void jade_engine_dispose(JadeEngineHandle* handle) {
  if (handle == nullptr) return;
  try {
    std::thread::id worker_tid;
    {
      std::lock_guard<std::mutex> lock(handle->queue_mutex);
      handle->disposed.store(true, std::memory_order_release);
      handle->active_generation.fetch_add(1, std::memory_order_acq_rel);
      handle->stop_requested.store(true, std::memory_order_release);
      handle->pending_job.reset();
      worker_tid = handle->worker_tid;
      handle->queue_cv.notify_all();
    }
    if (handle->worker_thread.joinable() && std::this_thread::get_id() != worker_tid) {
      handle->worker_thread.join();
    }
    delete handle;
  } catch (...) {
    // Never allow an exception to cross the C-ABI dispose boundary.
  }
}

size_t jade_engine_last_error(
    const JadeEngineHandle* handle,
    char* buf,
    size_t buf_cap) {
  if (handle == nullptr) {
    if (buf != nullptr && buf_cap > 0) buf[0] = '\0';
    return 0;
  }
  try {
    std::lock_guard<std::mutex> lock(handle->error_mutex);
    const size_t msg_len = handle->last_error.size();
    if (buf != nullptr && buf_cap > 0) {
      const size_t copy_len = std::min(msg_len, buf_cap - 1);
      if (copy_len > 0) {
        std::memcpy(buf, handle->last_error.data(), copy_len);
      }
      buf[copy_len] = '\0';
    }
    return msg_len;
  } catch (...) {
    if (buf != nullptr && buf_cap > 0) buf[0] = '\0';
    return 0;
  }
}

}  // extern "C"
