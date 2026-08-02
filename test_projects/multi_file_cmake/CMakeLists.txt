cmake_minimum_required(VERSION 3.20)
project(multi_file_cmake)

add_executable(multi_file_cmake
    multi_file_cmake.cc
    file_c.c
    file_cc.cc
)