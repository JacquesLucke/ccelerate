// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <fmt/format.h>
#include <fmt/ranges.h>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <random>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>

#include "base/error_code.hh"
#include "base/filesystem.hh"
#include "base/get_current_executable_path.hh"
#include "base/pair.hh"
#include "base/run_process.hh"
#include "base/string.hh"
#include "base/vector.hh"
#include "default_endpoint.hh"

namespace ccelerate::tests {

struct Args {
  bool use_external_server = false;
  path repo_dir;
  path test_projects_dir;
  path test_out_dir;
  path binary_dir;

  static Args &get() {
    static Args args;
    return args;
  }
};

static string get_random_endpoint() {
  static std::random_device rd;
  static std::mt19937 gen(rd());
  static std::uniform_int_distribution<int> dist(
      0, std::numeric_limits<int>::max());
  return fmt::format("ipc:///tmp/ccelerate-test-{}.ipc", dist(gen));
}

class CcelerateServerContext {
private:
  bool uses_external_server_;
  reproc::process server_process_;
  string endpoint_;

public:
  vector<pair<string, string>> env;

  CcelerateServerContext() {
    const Args &args = Args::get();
    uses_external_server_ = args.use_external_server;
    if (uses_external_server_) {
      endpoint_ = get_default_ccelerate_endpoint();
    } else {
      endpoint_ = get_random_endpoint();
      env.push_back(std::make_pair("CCELERATE_ENDPOINT", endpoint_));
      reproc::options options;
      options.env.extra = env;

      vector<string> cmd_args;
      cmd_args.push_back(args.binary_dir / "ccelerate");
      const error_code ec = server_process_.start(cmd_args, options);
      EXPECT_FALSE(ec) << "Failed to start server: " << ec.message();
    }
  }

  ~CcelerateServerContext() {
    if (!uses_external_server_) {
      const error_code ec = server_process_.terminate();
      EXPECT_FALSE(ec) << "Failed to terminate server: " << ec.message();
    }
  }
};

TEST(Integration, ClangNoArgs) {
  const Args &args = Args::get();
  CcelerateServerContext server_ctx;
  const ProcessResult result =
      run_process(ProcessArgs()
                      .arg(args.binary_dir / "ccelerate_clang")
                      .envs(server_ctx.env));
  EXPECT_EQ(result.exit_code(), 1);
  EXPECT_EQ(result.stdout, "");
  EXPECT_THAT(result.stderr, testing::HasSubstr("no input files"));
}

TEST(Integration, HelloWorld) {
  const Args &args = Args::get();
  CcelerateServerContext server_ctx;

  const path project_dir = args.test_projects_dir / "hello_world";
  const path output_file = args.test_out_dir / "hello_world";
  std::filesystem::create_directories(output_file.parent_path());

  const ProcessResult build_result =
      run_process(ProcessArgs()
                      .args({args.binary_dir / "ccelerate_clang", "-o",
                             output_file, project_dir / "hello_world.cc"})
                      .envs(server_ctx.env));
  ASSERT_EQ(build_result.exit_code(), 0);

  const ProcessResult run_result = run_process(ProcessArgs().arg(output_file));
  ASSERT_EQ(run_result.exit_code(), 0);
  EXPECT_EQ(run_result.stdout, "Hello World!\n");
  EXPECT_EQ(run_result.stderr, "");
}

TEST(Integration, BasicCMake) {
  const Args &args = Args::get();
  CcelerateServerContext server_ctx;

  const path project_dir = args.test_projects_dir / "basic_cmake";
  const path output_dir = args.test_out_dir / "basic_cmake";
  std::filesystem::create_directories(output_dir);

  const ProcessResult configure_result =
      run_process(ProcessArgs()
                      .args({args.binary_dir / "ccelerate_cmake", "-B",
                             output_dir, "-S", project_dir})
                      .envs(server_ctx.env));
  ASSERT_EQ(configure_result.exit_code(), 0);

  const ProcessResult build_result = run_process(
      ProcessArgs()
          .args({args.binary_dir / "ccelerate_cmake", "--build", output_dir})
          .envs(server_ctx.env));
  ASSERT_EQ(build_result.exit_code(), 0);

  const ProcessResult run_result =
      run_process(ProcessArgs().arg(output_dir / "basic_cmake"));
  ASSERT_EQ(run_result.exit_code(), 0);
  EXPECT_EQ(run_result.stdout, "It worked!\n");
  EXPECT_EQ(run_result.stderr, "");
}

} // namespace ccelerate::tests

int main(int argc, char **argv) {
  using namespace ccelerate::tests;
  ::testing::InitGoogleTest(&argc, argv);

  CLI::App app{"Integration tests for ccelerate"};
  argv = app.ensure_utf8(argv);

  Args &args = Args::get();
  app.add_option("--repo-dir", args.repo_dir,
                 "path to the ccelerate repository")
      ->required();
  app.add_flag("--external-server", args.use_external_server,
               "Use an existing external ccelerate server for the tests");

  CLI11_PARSE(app, argc, argv);

  args.test_projects_dir = args.repo_dir / "test_projects";
  args.binary_dir = ccelerate::get_current_executable_path().parent_path();
  args.test_out_dir = args.binary_dir / "tests_tmp";

  std::filesystem::remove_all(args.test_out_dir);
  return RUN_ALL_TESTS();
}