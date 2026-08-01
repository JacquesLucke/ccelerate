cmake_minimum_required(VERSION 3.20)
project(ccelerate)

enable_testing()

# Setup GTest library.
find_package(GTest CONFIG REQUIRED)

# Setup fmt library.
find_package(fmt CONFIG REQUIRED)

# Setup cppzmq library.
find_package(cppzmq CONFIG REQUIRED)

# Setup msgpack library.
find_package(msgpack-cxx CONFIG REQUIRED)

# Setup reproc library.
find_package(reproc CONFIG REQUIRED)
find_package(reproc++ CONFIG REQUIRED)

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
