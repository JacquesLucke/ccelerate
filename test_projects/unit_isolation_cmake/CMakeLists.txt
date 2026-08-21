cmake_minimum_required(VERSION 3.20)
project(unit_isolation_cmake)

add_executable(unit_isolation_cmake
    main.cc
    a/a.cc
    b/b.cc
)
target_include_directories(unit_isolation_cmake PRIVATE a b)
