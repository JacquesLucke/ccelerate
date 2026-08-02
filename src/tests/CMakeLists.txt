add_executable(integration_tests
  integration_tests.cc
)
target_include_directories(integration_tests PRIVATE
  ../
)
target_link_libraries(integration_tests PRIVATE
  fmt::fmt
  reproc
  reproc++
  CLI11::CLI11
  GTest::gtest
  tl::expected
  ccelerate::base
  spdlog::spdlog
)

include(GoogleTest)
gtest_discover_tests(integration_tests
  EXTRA_ARGS --repo-dir ${CMAKE_SOURCE_DIR}
)
