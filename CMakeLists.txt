cmake_minimum_required(VERSION 3.20)
project(ccelerate)

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

# Setup LLVM and Clang libraries.
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

# Setup ccelerate executable.
add_executable(ccelerate
  ./src/main.cc
)
set_target_properties(ccelerate PROPERTIES
  CXX_STANDARD 20
  CXX_STANDARD_REQUIRED ON
  CXX_EXTENSIONS OFF
)
target_link_libraries(ccelerate PRIVATE
  fmt::fmt
  cppzmq-static
  msgpack-cxx
  reproc
  reproc++
  TBB::tbb
  CLI11::CLI11
  unofficial::whereami::whereami
  ${llvm_libs}
  ${clang_libs}
)
target_compile_definitions(ccelerate PRIVATE MSGPACK_NO_BOOST)

function(add_ccelerate_wrapper wrapper_name type)
  add_executable(${wrapper_name}
    ./src/program_wrapper.cc
  )
  set_target_properties(${wrapper_name} PROPERTIES
    CXX_STANDARD 20
    CXX_STANDARD_REQUIRED ON
    CXX_EXTENSIONS OFF
  )
  target_link_libraries(${wrapper_name} PRIVATE
    fmt::fmt
    cppzmq-static
    msgpack-cxx)
  target_compile_definitions(${wrapper_name} PRIVATE MSGPACK_NO_BOOST)
  target_compile_definitions(${wrapper_name} PRIVATE CCELERATE_WRAP_TYPE=${type})
endfunction()

add_ccelerate_wrapper(ccelerate_clang Clang)
add_ccelerate_wrapper(ccelerate_clang++ Clangxx)
add_ccelerate_wrapper(ccelerate_ar Ar)
add_ccelerate_wrapper(ccelerate_cmake CMake)

add_executable(integration_tests
  ./src/tests/integration_tests.cc
)
set_target_properties(integration_tests PROPERTIES
  CXX_STANDARD 20
  CXX_STANDARD_REQUIRED ON
  CXX_EXTENSIONS OFF
)
target_link_libraries(integration_tests PRIVATE
  fmt::fmt
  reproc
  reproc++
  CLI11::CLI11
  GTest::gtest
  unofficial::whereami::whereami
)
add_test(NAME integration_tests COMMAND integration_tests --repo-dir ${CMAKE_CURRENT_SOURCE_DIR})