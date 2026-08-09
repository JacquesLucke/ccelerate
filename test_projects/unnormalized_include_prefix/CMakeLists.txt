cmake_minimum_required(VERSION 3.20)
project(unnormalized_include_prefix)

# Intentionally unnormalized path.
include_directories(
  ${CMAKE_CURRENT_SOURCE_DIR}/a/b/../../decoy/include
  ${CMAKE_CURRENT_SOURCE_DIR}
)

add_executable(unnormalized_include_prefix a/b/main.cc)
