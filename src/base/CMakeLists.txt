add_library(ccelerate_base STATIC
  run_process.cc
)
target_include_directories(ccelerate_base PUBLIC
  .
  ${TL_FUNCTION_REF_INCLUDE_DIRS}
)
target_link_libraries(ccelerate_base PRIVATE
  reproc
  reproc++
  unofficial::whereami::whereami
  spdlog::spdlog
)

add_library(ccelerate::base ALIAS ccelerate_base)
