cmake_minimum_required(VERSION 3.20)
project(ccelerate)

if(CMAKE_INSTALL_PREFIX_INITIALIZED_TO_DEFAULT)
  set(CMAKE_INSTALL_PREFIX ${CMAKE_BINARY_DIR}/bin CACHE PATH "default install path" FORCE)
endif()

set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin)

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

add_subdirectory(src/base)
add_subdirectory(src/tests)

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
  tl::expected
  ccelerate::base
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
    msgpack-cxx
    tl::expected
    ccelerate::base
  )
  target_compile_definitions(${wrapper_name} PRIVATE MSGPACK_NO_BOOST)
  target_compile_definitions(${wrapper_name} PRIVATE CCELERATE_WRAP_TYPE=${type})
endfunction()

add_ccelerate_wrapper(ccelerate_clang Clang)
add_ccelerate_wrapper(ccelerate_clang++ Clangxx)
add_ccelerate_wrapper(ccelerate_ar Ar)
add_ccelerate_wrapper(ccelerate_cmake CMake)
