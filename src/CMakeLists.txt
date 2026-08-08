add_subdirectory(base)
add_subdirectory(tests)

add_executable(ccelerate
  server_main.cc
  request_handler.cc
  request_handler__cmake.cc
  request_handler__clang.cc
  run_process_traced.cc
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
  Tracy::TracyClient
)
target_compile_options(ccelerate PRIVATE
  $<$<OR:$<CXX_COMPILER_ID:GNU>,$<CXX_COMPILER_ID:Clang>>:
  -Werror
  >
  $<$<CXX_COMPILER_ID:MSVC>:
  /WX
  >
)
target_compile_definitions(ccelerate PRIVATE MSGPACK_NO_BOOST)

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
)

# LLVM is built with -fno-rtti; subclasses of Clang AST/Frontend types need the same.
target_compile_options(clang_for_ccelerate PRIVATE
  $<$<OR:$<CXX_COMPILER_ID:GNU>,$<CXX_COMPILER_ID:Clang>>:-fno-rtti>
  $<$<CXX_COMPILER_ID:MSVC>:/GR->
)
target_compile_definitions(clang_for_ccelerate PRIVATE MSGPACK_NO_BOOST)