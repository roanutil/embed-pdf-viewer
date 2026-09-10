# Builds the C++ native benchmark addon for ARM64 macOS.
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

# Match the Rust release profile's optimization level. These settings do not
# make the Rust and C++ compiler pipelines identical.
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

add_library(${PROJECT_NAME} SHARED
  "${CMAKE_CURRENT_LIST_DIR}/../cpp/ops.cc"
  "${CMAKE_CURRENT_LIST_DIR}/../cpp/ops_napi.cc"
)

target_compile_options(${PROJECT_NAME} PRIVATE -O3 -DNDEBUG)

# Request optimization across C++ source files when the toolchain supports it.
# CMake selects the implementation; this need not match Cargo's full LTO.
include(CheckIPOSupported)
check_ipo_supported(RESULT ipo_supported OUTPUT ipo_error)
if(ipo_supported)
  set_target_properties(${PROJECT_NAME} PROPERTIES INTERPROCEDURAL_OPTIMIZATION TRUE)
else()
  # Keep an unsupported optimization setting visible in benchmark logs.
  message(WARNING
    "Optimization across C++ source files is unavailable (${ipo_error}). "
    "Variant d will build without it, while Rust variant c enables lto = true. "
    "Timing differences between these builds cannot be attributed to language alone.")
endif()

# cmake-js supplies Node headers. Resolve node-addon-api's C++ headers from
# the benchmark's installed dependencies.
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
