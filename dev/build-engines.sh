#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Builds the C++26 Xiangqi reference UCI engine (`jade-uci-engine`), narrow
# C-ABI mobile libraries (`libjade_engine.dylib`/`.so` and `libjade_engine.a`),
# and runs the C-ABI / concurrency / cancellation / UCI smoke tests.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BRIDGE_DIR="${REPO_ROOT}/engines/mobile-bridge"
OUT_DIR="${JADE_ENGINE_BUILD_DIR:-${BRIDGE_DIR}/build}"
BUILD_ONLY=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --out-dir)
      OUT_DIR="$2"
      shift 2
      ;;
    --build-only)
      BUILD_ONLY=1
      shift
      ;;
    *)
      echo "Usage: $0 [--out-dir <path>] [--build-only]" >&2
      exit 2
      ;;
  esac
done

if [[ -n "${CXX:-}" ]]; then
  CXX_BIN="${CXX}"
elif [[ -x "/usr/bin/clang++" ]]; then
  CXX_BIN="/usr/bin/clang++"
elif command -v clang++ >/dev/null 2>&1; then
  CXX_BIN="$(command -v clang++)"
else
  CXX_BIN="$(command -v g++)"
fi

AR_BIN="${AR:-$(command -v ar)}"
CXXFLAGS=(-std=c++26 -O2 -Wall -Wextra -Werror -fPIC -pthread)
INCLUDES=(-I"${BRIDGE_DIR}/include" -I"${BRIDGE_DIR}/src")

OS_NAME="$(uname -s)"
if [[ "${OS_NAME}" == "Darwin" ]]; then
  SHARED_EXT="dylib"
  SHARED_FLAGS=(-dynamiclib -Wl,-install_name,@rpath/libjade_engine.dylib)
else
  SHARED_EXT="so"
  SHARED_FLAGS=(-shared)
fi

mkdir -p "${OUT_DIR}"

echo "[build-engines] Compiler: ${CXX_BIN} (-std=c++26 -O2)"
echo "[build-engines] Output directory: ${OUT_DIR}"

# 1. Compile shared object units
"${CXX_BIN}" "${CXXFLAGS[@]}" "${INCLUDES[@]}" \
  -c "${BRIDGE_DIR}/src/xiangqi_position.cpp" -o "${OUT_DIR}/xiangqi_position.o"
"${CXX_BIN}" "${CXXFLAGS[@]}" "${INCLUDES[@]}" \
  -c "${BRIDGE_DIR}/src/xiangqi_search.cpp" -o "${OUT_DIR}/xiangqi_search.o"
"${CXX_BIN}" "${CXXFLAGS[@]}" "${INCLUDES[@]}" \
  -c "${BRIDGE_DIR}/src/mobile_bridge.cpp" -o "${OUT_DIR}/mobile_bridge.o"
"${CXX_BIN}" "${CXXFLAGS[@]}" "${INCLUDES[@]}" \
  -c "${BRIDGE_DIR}/src/jni_bridge.cpp" -o "${OUT_DIR}/jni_bridge.o"

# 2. Archive static library (`libjade_engine.a`)
rm -f "${OUT_DIR}/libjade_engine.a"
"${AR_BIN}" rcs "${OUT_DIR}/libjade_engine.a" \
  "${OUT_DIR}/xiangqi_position.o" \
  "${OUT_DIR}/xiangqi_search.o" \
  "${OUT_DIR}/mobile_bridge.o" \
  "${OUT_DIR}/jni_bridge.o"

# 3. Link shared library (`libjade_engine.dylib` / `libjade_engine.so`)
"${CXX_BIN}" "${CXXFLAGS[@]}" "${SHARED_FLAGS[@]}" \
  "${OUT_DIR}/xiangqi_position.o" \
  "${OUT_DIR}/xiangqi_search.o" \
  "${OUT_DIR}/mobile_bridge.o" \
  "${OUT_DIR}/jni_bridge.o" \
  -o "${OUT_DIR}/libjade_engine.${SHARED_EXT}"

# 4. Link standalone UCI executable (`jade-uci-engine` + `jade-ReferenceUCI` alias)
"${CXX_BIN}" "${CXXFLAGS[@]}" "${INCLUDES[@]}" \
  "${BRIDGE_DIR}/src/uci_main.cpp" \
  "${OUT_DIR}/xiangqi_position.o" \
  "${OUT_DIR}/xiangqi_search.o" \
  -o "${OUT_DIR}/jade-uci-engine"
ln -sf "jade-uci-engine" "${OUT_DIR}/jade-ReferenceUCI"

# 5. Link C-ABI & concurrency test suite (`engine_bridge_test`)
"${CXX_BIN}" "${CXXFLAGS[@]}" "${INCLUDES[@]}" \
  "${BRIDGE_DIR}/tests/engine_bridge_test.cpp" \
  "${OUT_DIR}/libjade_engine.a" \
  -o "${OUT_DIR}/engine_bridge_test"

echo "[build-engines] Built artifacts:"
ls -lh "${OUT_DIR}/jade-uci-engine" \
       "${OUT_DIR}/libjade_engine.${SHARED_EXT}" \
       "${OUT_DIR}/libjade_engine.a" \
       "${OUT_DIR}/engine_bridge_test"

if [[ "${BUILD_ONLY}" -eq 1 ]]; then
  exit 0
fi

# 6. Run C++26 rules, tactical, C-ABI, cancellation, and supersession tests
echo "[build-engines] Running engine_bridge_test..."
"${OUT_DIR}/engine_bridge_test"

# 7. Run UCI protocol smoke checks (bounded MultiPV search + live `go infinite` -> `stop`)
echo "[build-engines] Running UCI smoke check (bounded depth 4 MultiPV=2)..."
UCI_BOUNDED_OUT="$(
  printf "uci\nisready\nsetoption name MultiPV value 2\nposition startpos moves h2e2 h9g7\ngo depth 4\nquit\n" \
    | "${OUT_DIR}/jade-uci-engine"
)"
grep -q "^uciok$" <<<"${UCI_BOUNDED_OUT}"
grep -q "^readyok$" <<<"${UCI_BOUNDED_OUT}"
grep -q "^info depth 4 " <<<"${UCI_BOUNDED_OUT}"
grep -q "^bestmove [a-i][0-9][a-i][0-9]$" <<<"${UCI_BOUNDED_OUT}"

echo "[build-engines] Running UCI smoke check (live go infinite -> stop)..."
UCI_INF_OUT="$(
  {
    printf "uci\nisready\nposition startpos\ngo infinite\n"
    sleep 0.05
    printf "stop\nquit\n"
  } | "${OUT_DIR}/jade-uci-engine"
)"
grep -q "^bestmove [a-i][0-9][a-i][0-9]$" <<<"${UCI_INF_OUT}"

echo "[build-engines] All builds and verification checks succeeded."
