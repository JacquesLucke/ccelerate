add_executable(ccelerate_tests
  tests_main.cc
)
target_link_libraries(ccelerate_tests PRIVATE
  GTest::gtest
  ccelerate::lib
)

include(GoogleTest)
gtest_discover_tests(ccelerate_tests
  EXTRA_ARGS --repo-dir ${CMAKE_SOURCE_DIR}
  DISCOVERY_EXTRA_ARGS --repo-dir ${CMAKE_SOURCE_DIR}
)
