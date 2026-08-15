add_subdirectory(base)

add_library(ccelerate_lib STATIC
  ccelerate_server.cc
  request_handler.cc
  request_handler__ar.cc
  request_handler__cmake.cc
  request_handler__clang.cc
  run_process_traced.cc
  config.cc
  local_code_frontmatter.cc
)
target_include_directories(ccelerate_lib PUBLIC
  ${CMAKE_CURRENT_SOURCE_DIR}
)
target_link_libraries(ccelerate_lib PUBLIC
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
  Tracy::TracyClient
  toml11::toml11
  re2::re2
  xxHash::xxhash
)
target_compile_options(ccelerate_lib PRIVATE
  $<$<OR:$<CXX_COMPILER_ID:GNU>,$<CXX_COMPILER_ID:Clang>>:
  -Werror
  >
  $<$<CXX_COMPILER_ID:MSVC>:
  /WX
  >
)
target_compile_definitions(ccelerate_lib PUBLIC MSGPACK_NO_BOOST)
add_library(ccelerate::lib ALIAS ccelerate_lib)

add_executable(ccelerate
  ccelerate_main.cc
)
target_link_libraries(ccelerate PRIVATE
  ccelerate::lib
)

function(add_ccelerate_wrapper wrapper_name type)
  add_executable(${wrapper_name}
    wrap_main.cc
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

add_executable(clang_for_ccelerate
  clang_for_ccelerate.cc
  config.cc
  local_code_frontmatter.cc
)
target_link_libraries(clang_for_ccelerate PRIVATE
  fmt::fmt
  tl::expected
  ccelerate::base
  spdlog::spdlog
  CLI11::CLI11
  ${llvm_libs}
  ${clang_libs}
  ctre::ctre
  re2::re2
  toml11::toml11
)

# LLVM is built with -fno-rtti; subclasses of Clang AST/Frontend types need the same.
target_compile_options(clang_for_ccelerate PRIVATE
  $<$<OR:$<CXX_COMPILER_ID:GNU>,$<CXX_COMPILER_ID:Clang>>:-fno-rtti>
  $<$<CXX_COMPILER_ID:MSVC>:/GR->
)
target_compile_definitions(clang_for_ccelerate PRIVATE MSGPACK_NO_BOOST)

add_subdirectory(tests)
