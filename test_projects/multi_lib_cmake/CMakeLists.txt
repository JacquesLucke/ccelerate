cmake_minimum_required(VERSION 3.20)
project(multi_lib_cmake LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Keep artifacts next to each other so the shared library is found at runtime
# on Windows (DLL search path) and Unix (build-tree RPATH).
set(CMAKE_RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR})
set(CMAKE_LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR})
set(CMAKE_ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR})

# Static libs are linked into the shared library, so they need PIC on Unix.
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

add_library(multi_lib_alpha STATIC
    lib_alpha/a01.cc
    lib_alpha/a02.cc
    lib_alpha/a03.cc
    lib_alpha/a04.cc
    lib_alpha/a05.cc
    lib_alpha/a06.cc
)
target_include_directories(multi_lib_alpha PUBLIC lib_alpha)

add_library(multi_lib_beta STATIC
    lib_beta/b01.cc
    lib_beta/b02.cc
    lib_beta/b03.cc
    lib_beta/b04.cc
    lib_beta/b05.cc
    lib_beta/b06.cc
)
target_include_directories(multi_lib_beta PUBLIC lib_beta)

add_library(multi_lib_gamma STATIC
    lib_gamma/g01.cc
    lib_gamma/g02.cc
    lib_gamma/g03.cc
    lib_gamma/g04.cc
    lib_gamma/g05.cc
)
target_include_directories(multi_lib_gamma PUBLIC lib_gamma)

add_library(multi_lib_shared SHARED
    lib_shared/shared.cc
    lib_shared/part_r.cc
    lib_shared/part_s.cc
)
target_include_directories(multi_lib_shared
    PUBLIC lib_shared
)
target_link_libraries(multi_lib_shared
    PRIVATE
        multi_lib_alpha
        multi_lib_beta
        multi_lib_gamma
)
target_compile_definitions(multi_lib_shared PRIVATE MULTI_LIB_SHARED_BUILD)
set_target_properties(multi_lib_shared PROPERTIES
    CXX_VISIBILITY_PRESET hidden
    VISIBILITY_INLINES_HIDDEN ON
)

add_executable(multi_lib_cmake main.cc)
target_link_libraries(multi_lib_cmake PRIVATE multi_lib_shared)
