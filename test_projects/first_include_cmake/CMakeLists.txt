cmake_minimum_required(VERSION 3.20)
project(first_include_cmake)

add_library(first_include_lib STATIC
    a/a.cc
    b/b.cc
)
target_include_directories(first_include_lib PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})

add_executable(first_include_cmake main.cc)
target_link_libraries(first_include_cmake PRIVATE first_include_lib)
