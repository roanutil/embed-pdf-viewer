# Mirrors packages/engine/runtime/build/CMakeLists.txt, trimmed to macOS and to
# the two sources arm D needs.
cmake_minimum_required(VERSION 3.15)
project(cc_ops CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED PDFIUM_LIB_DIR)
  message(FATAL_ERROR "PDFIUM_LIB_DIR is required")
endif()
if(NOT DEFINED PDFIUM_INCLUDE_DIR)
  message(FATAL_ERROR "PDFIUM_INCLUDE_DIR is required")
endif()

# arm c's Rust is built `--release` (opt-level = 3, lto = true,
# codegen-units = 1, see Cargo.toml's [profile.release]). Arm d must be
# compiled at the same optimization level: the wasm phase of this spike once
# linked arm d at -O0 against arm c's -O3, which manufactured a 25-33% win for
# "Rust" that was really a win for "the only one of these that got optimized"
# (see RESULTS.md). Default to Release here rather than trusting cmake-js's
# default, and set -O3 explicitly rather than trusting CMAKE_CXX_FLAGS_RELEASE's
# default, so the optimization level is visible in this file and cannot drift.
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

add_library(${PROJECT_NAME} SHARED
  "${CMAKE_CURRENT_LIST_DIR}/../cpp/ops.cc"
  "${CMAKE_CURRENT_LIST_DIR}/../cpp/ops_napi.cc"
)

target_compile_options(${PROJECT_NAME} PRIVATE -O3 -DNDEBUG)

# Cross-TU inlining, the closest native match to Cargo's codegen-units = 1:
# ops.cc's loop and ops_napi.cc's thin wrapper are two translation units, same
# as arm c's `ops` crate (one codegen unit) linked into `napi-arms`. Silently
# skipped (not an error) if this toolchain's compiler/linker don't support it.
include(CheckIPOSupported)
check_ipo_supported(RESULT ipo_supported OUTPUT ipo_error)
if(ipo_supported)
  set_target_properties(${PROJECT_NAME} PROPERTIES INTERPROCEDURAL_OPTIMIZATION TRUE)
else()
  # This used to fail silently. Arm c's Cargo.toml sets `lto = true`
  # unconditionally, so a toolchain that can't do IPO here would build arm c
  # with LTO and arm d without it and say nothing -- the same "one arm got
  # optimized and the other did not" class of bug that already produced a
  # fake 25-33% gap for "Rust" earlier in this spike (see RESULTS.md).
  message(WARNING
    "IPO/LTO not supported on this toolchain (${ipo_error}); arm d will "
    "build WITHOUT cross-TU inlining while arm c keeps lto = true "
    "unconditionally. Any arm c vs arm d gap measured under this build is "
    "confounded with that asymmetry and must not be reported as language "
    "difference.")
endif()

# cmake-js supplies its own napi.h-less node headers via CMAKE_JS_INC;
# node-addon-api's C++ wrapper (napi.h, napi-inl.h) ships as its own npm
# package and is resolved the same way its own README documents.
execute_process(
  COMMAND node -p "require('node-addon-api').include.replace(/\"/g, '')"
  WORKING_DIRECTORY "${CMAKE_CURRENT_LIST_DIR}/.."
  OUTPUT_VARIABLE NODE_ADDON_API_DIR
  OUTPUT_STRIP_TRAILING_WHITESPACE
)

target_include_directories(${PROJECT_NAME} PRIVATE
  ${CMAKE_JS_INC}
  "${NODE_ADDON_API_DIR}"
  "${PDFIUM_INCLUDE_DIR}"
  "${CMAKE_CURRENT_LIST_DIR}/../cpp"
)

target_link_libraries(${PROJECT_NAME} PRIVATE
  ${CMAKE_JS_LIB}
  "${PDFIUM_LIB_DIR}/libembedpdf.dylib"
  "-framework AppKit"
  "-framework CoreFoundation"
  "-framework CoreGraphics"
  "-framework Foundation"
)

set_target_properties(${PROJECT_NAME} PROPERTIES
  PREFIX ""
  SUFFIX ".node"
  OUTPUT_NAME "cc-ops"
  BUILD_RPATH "@loader_path"
  INSTALL_RPATH "@loader_path"
)
