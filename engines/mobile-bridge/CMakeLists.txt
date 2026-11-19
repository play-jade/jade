cmake_minimum_required(VERSION 3.25)
project(jade_mobile_bridge VERSION 0.1.0 LANGUAGES C CXX)

if(CMAKE_VERSION VERSION_GREATER_EQUAL "3.28")
  set(CMAKE_CXX_STANDARD 26)
  set(CMAKE_CXX_STANDARD_REQUIRED ON)
  set(CMAKE_CXX_EXTENSIONS OFF)
else()
  add_compile_options($<$<COMPILE_LANGUAGE:CXX>:-std=c++26>)
endif()
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

find_package(Threads REQUIRED)

set(JADE_ENGINE_CORE_SOURCES
  src/xiangqi_position.cpp
  src/xiangqi_search.cpp
)

set(JADE_ENGINE_BRIDGE_SOURCES
  ${JADE_ENGINE_CORE_SOURCES}
  src/mobile_bridge.cpp
  src/jni_bridge.cpp
)

add_library(jade_engine_static STATIC ${JADE_ENGINE_BRIDGE_SOURCES})
target_include_directories(jade_engine_static
  PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>
  PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)
target_link_libraries(jade_engine_static PUBLIC Threads::Threads)
set_target_properties(jade_engine_static PROPERTIES OUTPUT_NAME jade_engine)

add_library(jade_engine_shared SHARED ${JADE_ENGINE_BRIDGE_SOURCES})
target_include_directories(jade_engine_shared
  PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>
  PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)
target_compile_definitions(jade_engine_shared PRIVATE JADE_ENGINE_BUILD_SHARED=1)
target_link_libraries(jade_engine_shared PUBLIC Threads::Threads)
set_target_properties(jade_engine_shared PROPERTIES OUTPUT_NAME jade_engine)

add_executable(jade-uci-engine
  ${JADE_ENGINE_CORE_SOURCES}
  src/uci_main.cpp
)
target_include_directories(jade-uci-engine PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)
target_link_libraries(jade-uci-engine PRIVATE Threads::Threads)

enable_testing()
add_executable(engine_bridge_test tests/engine_bridge_test.cpp)
target_include_directories(engine_bridge_test
  PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/src
)
target_link_libraries(engine_bridge_test PRIVATE jade_engine_static Threads::Threads)
add_test(NAME engine_bridge_test COMMAND engine_bridge_test)
