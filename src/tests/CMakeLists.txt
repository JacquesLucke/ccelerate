add_executable(integration_tests
  integration_tests.cc
)
target_link_libraries(integration_tests PRIVATE
  GTest::gtest
  ccelerate::lib
)

include(GoogleTest)
gtest_discover_tests(integration_tests
  EXTRA_ARGS --repo-dir ${CMAKE_SOURCE_DIR}
)
