cmake_minimum_required(VERSION 3.20)
project(ccelerate)

if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
  set(CMAKE_INSTALL_PREFIX ${CMAKE_BINARY_DIR}/bin CACHE PATH "default install path" FORCE)
endif()

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

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
  clangLex
  clangSerialization
  clangTooling
)

add_subdirectory(src/base)
add_subdirectory(src/tests)

# Setup ccelerate executable.
add_executable(ccelerate
  ./src/server_main.cc
  ./src/request_handler.cc
  ./src/request_handler__cmake.cc
  ./src/request_handler__clang.cc
)
target_link_libraries(ccelerate PRIVATE
  fmt::fmt
  cppzmq-static
  msgpack-cxx
  reproc
  reproc++
  TBB::tbb
  CLI11::CLI11
  tl::expected
  ccelerate::base
  spdlog::spdlog
  ${llvm_libs}
  ${clang_libs}
)
target_compile_definitions(ccelerate PRIVATE MSGPACK_NO_BOOST)

function(add_ccelerate_wrapper wrapper_name type)
  add_executable(${wrapper_name}
    ./src/wrap_main.cc
  )
  target_link_libraries(${wrapper_name} PRIVATE
    fmt::fmt
    cppzmq-static
    msgpack-cxx
    tl::expected
    ccelerate::base
    spdlog::spdlog
  )
  target_compile_definitions(${wrapper_name} PRIVATE MSGPACK_NO_BOOST)
  target_compile_definitions(${wrapper_name} PRIVATE CCELERATE_WRAP_TYPE=${type})
endfunction()

add_ccelerate_wrapper(ccelerate_clang Clang)
add_ccelerate_wrapper(ccelerate_clang++ Clangxx)
add_ccelerate_wrapper(ccelerate_ar Ar)
add_ccelerate_wrapper(ccelerate_cmake CMake)
