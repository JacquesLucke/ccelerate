cmake_minimum_required(VERSION 3.20)
project(ccelerate)

set(CCELERATE_OUTPUT_DIR ${CMAKE_BINARY_DIR}/bin)

if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
  set(CMAKE_INSTALL_PREFIX ${CCELERATE_OUTPUT_DIR} CACHE PATH "default install path" FORCE)
endif()

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CCELERATE_OUTPUT_DIR})

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

enable_testing()

# Find dependencies.
find_package(GTest CONFIG REQUIRED)
find_package(fmt CONFIG REQUIRED)
find_package(cppzmq CONFIG REQUIRED)
find_package(msgpack-cxx CONFIG REQUIRED)
find_package(reproc CONFIG REQUIRED)
find_package(reproc++ CONFIG REQUIRED)
find_package(TBB CONFIG REQUIRED)
find_package(CLI11 CONFIG REQUIRED)
find_package(unofficial-whereami CONFIG REQUIRED)
find_package(tl-expected CONFIG REQUIRED)
find_package(spdlog CONFIG REQUIRED)
find_package(Tracy CONFIG REQUIRED)
find_package(ctre CONFIG REQUIRED)
find_path(TL_FUNCTION_REF_INCLUDE_DIRS "tl/function_ref.hpp")

# Prepare LLVM and Clang libraries.
find_package(LLVM CONFIG REQUIRED)
find_package(Clang CONFIG REQUIRED)
list(APPEND CMAKE_MODULE_PATH "${LLVM_CMAKE_DIR}")
include(HandleLLVMOptions)
add_definitions(${LLVM_DEFINITIONS})
llvm_map_components_to_libnames(llvm_libs Support native)
set(clang_libs
  clangAST
  clangBasic
  clangDriver
  clangFrontend
  clangFrontendTool
  clangLex
  clangSerialization
  clangTooling
)

add_subdirectory(src)

# Quiet lots of "up-to-date" messages.
set(CMAKE_INSTALL_MESSAGE LAZY)

install(
  DIRECTORY extern/gcc-13-install
  DESTINATION ${CCELERATE_OUTPUT_DIR}/extern
)