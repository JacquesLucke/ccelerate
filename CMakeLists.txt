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

# Setup ccelerate executable.
add_executable(ccelerate
    ./src/ccelerate.cc
)
set_target_properties(ccelerate PROPERTIES
    CXX_STANDARD 20
    CXX_STANDARD_REQUIRED ON
    CXX_EXTENSIONS OFF
)
target_link_libraries(ccelerate PRIVATE fmt::fmt)
target_link_libraries(ccelerate PRIVATE cppzmq-static)
target_link_libraries(ccelerate PRIVATE msgpack-cxx)
target_compile_definitions(ccelerate PRIVATE MSGPACK_NO_BOOST)
