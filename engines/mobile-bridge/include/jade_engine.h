/* SPDX-License-Identifier: Apache-2.0 */
#ifndef JADE_ENGINE_H_
#define JADE_ENGINE_H_

#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32) || defined(__CYGWIN__)
  #if defined(JADE_ENGINE_BUILD_SHARED)
    #define JADE_ENGINE_API __declspec(dllexport)
  #elif defined(JADE_ENGINE_USE_SHARED)
    #define JADE_ENGINE_API __declspec(dllimport)
  #else
    #define JADE_ENGINE_API
  #endif
#elif defined(__GNUC__) || defined(__clang__)
  #define JADE_ENGINE_API __attribute__((visibility("default")))
#else
  #define JADE_ENGINE_API
#endif

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Opaque per-instance engine handle.
 *
 * Each handle owns an isolated Xiangqi search engine instance (transposition
 * table, killer/history tables, configuration, last-error buffer) and a single
 * bounded background worker thread so searches never block UI callers.
 */
typedef struct JadeEngineHandle JadeEngineHandle;

/**
 * Status codes returned by the narrow C-ABI surface.
 *
 * Zero indicates success; negative values indicate caller or runtime errors
 * whose human-readable detail can be retrieved via `jade_engine_last_error`.
 */
typedef enum JadeEngineStatus {
  JADE_ENGINE_OK = 0,
  JADE_ENGINE_ERR_INVALID_ARGUMENT = -1,
  JADE_ENGINE_ERR_INVALID_JSON = -2,
  JADE_ENGINE_ERR_INVALID_POSITION = -3,
  JADE_ENGINE_ERR_DISPOSED = -4,
  JADE_ENGINE_ERR_INTERNAL = -5
} JadeEngineStatus;

/**
 * Asynchronous callback invoked on the engine's bounded worker thread for
 * streaming `info` frames and terminal `complete` frames.
 *
 * `payload_json_utf8` points to a NUL-terminated UTF-8 JSON buffer of length
 * `payload_len` bytes (excluding the trailing NUL). The pointer is valid only
 * for the duration of the callback invocation.
 *
 * Every payload repeats `search_id` and `state_id` from the originating
 * `jade_engine_search` request, along with normalized evaluation manifest
 * metadata (`engine_id`, `binary_hash`, `network_hash`, `rules_profile`,
 * `config_hash`), normalized `score`, and verified legal `pv` moves.
 */
typedef void (*JadeEngineCallback)(
  const char* payload_json_utf8,
  size_t payload_len,
  void* user_data
);

/**
 * Allocate and initialize a new isolated engine instance and its bounded
 * background worker thread.
 */
JADE_ENGINE_API int32_t jade_engine_create(JadeEngineHandle** out_handle);

/**
 * Apply JSON configuration options (`hash_mb`, `threads`, `multi_pv`,
 * `move_overhead_ms`, `rules_profile`, `engine_id`, `network_hash`) to the
 * engine instance and recompute its deterministic `config_hash`.
 */
JADE_ENGINE_API int32_t jade_engine_configure(
  JadeEngineHandle* handle,
  const char* options_json_utf8,
  size_t len
);

/**
 * Validate a search request (`search_id`, `state_id`, `fen`, `moves`, `limits`)
 * synchronously and enqueue it onto the handle's bounded background worker.
 *
 * Never blocks the caller/UI thread for search execution. If an earlier search
 * is active on `handle`, it is atomically superseded within 256 nodes (< 5 ms)
 * so stale `info` updates are suppressed and any superseded completion frame
 * is explicitly tagged (`"superseded": true`, `"stale": true`).
 */
JADE_ENGINE_API int32_t jade_engine_search(
  JadeEngineHandle* handle,
  const char* request_json_utf8,
  size_t len,
  JadeEngineCallback on_info,
  void* user_data
);

/**
 * Signal atomic cancellation of any active or queued search on `handle`.
 *
 * When invoked from outside the worker callback thread, waits for the bounded
 * worker to reach an idle state (checked every 256 nodes, completing in < 5 ms).
 * Safe to invoke re-entrantly from inside `JadeEngineCallback` without deadlock.
 */
JADE_ENGINE_API int32_t jade_engine_cancel(JadeEngineHandle* handle);

/**
 * Cancel any active search, join the bounded worker thread, and release all
 * memory owned by `handle`. Passing `NULL` is a safe no-op.
 */
JADE_ENGINE_API void jade_engine_dispose(JadeEngineHandle* handle);

/**
 * Copy the last UTF-8 error message recorded on `handle` into `buf` (up to
 * `buf_cap - 1` bytes plus a trailing NUL when `buf != NULL && buf_cap > 0`).
 *
 * Returns the full byte length (excluding NUL) of the stored error string.
 */
JADE_ENGINE_API size_t jade_engine_last_error(
  const JadeEngineHandle* handle,
  char* buf,
  size_t buf_cap
);

#ifdef __cplusplus
}  /* extern "C" */
#endif

#endif  /* JADE_ENGINE_H_ */
