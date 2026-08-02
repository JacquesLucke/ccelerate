add_library(ccelerate_base STATIC
    run_process.cc
)
target_include_directories(ccelerate_base PUBLIC .)
target_link_libraries(ccelerate_base PRIVATE
    reproc
    reproc++
)

add_library(ccelerate::base ALIAS ccelerate_base)
