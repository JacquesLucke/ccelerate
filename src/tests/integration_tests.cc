// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <fmt/format.h>
#include <fmt/ranges.h>
#include <gtest/gtest.h>
#include <random>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>

#include "../default_endpoint.hh"
#include "../get_current_executable_path.hh"

namespace ccelerate::tests {

struct Args {
  bool use_external_server = false;
  std::filesystem::path repo_dir;
  std::filesystem::path test_projects_dir;
  std::filesystem::path test_out_dir;
  std::filesystem::path test_build_dir;

  static Args &get() {
    static Args args;
    return args;
  }
};

static std::string get_random_endpoint() {
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
  std::string endpoint_;

public:
  CcelerateServerContext() {
    const Args &args = Args::get();
    uses_external_server_ = args.use_external_server;
    if (uses_external_server_) {
      endpoint_ = get_default_ccelerate_endpoint();
    } else {
      endpoint_ = get_random_endpoint();
      reproc::options options;
      const std::vector<std::pair<std::string, std::string>> extra_env = {
          std::make_pair("CCELERATE_ENDPOINT", endpoint_)};
      options.env.extra = extra_env;

      std::vector<std::string> cmd_args;
      cmd_args.push_back(args.test_build_dir / "ccelerate");
      const std::error_code ec = server_process_.start(cmd_args, options);
      EXPECT_FALSE(ec) << "Failed to start server: " << ec.message();
    }
  }

  ~CcelerateServerContext() {
    if (!uses_external_server_) {
      const std::error_code ec = server_process_.terminate();
      EXPECT_FALSE(ec) << "Failed to terminate server: " << ec.message();
    }
  }

  const std::string &endpoint() const { return endpoint_; }
};

struct ProcessResult {
  std::string stdout;
  std::string stderr;
  int exit_code;
};

static std::optional<ProcessResult>
run_and_get_result(const std::vector<std::string> &args,
                   reproc::options options = {},
                   const bool expect_exit_0 = false) {
  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  reproc::process proc;
  std::error_code ec = proc.start(args, options);
  EXPECT_FALSE(ec) << "Failed to start process: " << ec.message() << " --- "
                   << fmt::format("{}", fmt::join(args, " "));
  std::string stdout;
  std::string stderr;
  ec = reproc::drain(proc, reproc::sink::string(stdout),
                     reproc::sink::string(stderr));
  EXPECT_FALSE(ec) << "Failed to drain process: " << ec.message();
  if (ec) {
    return std::nullopt;
  }
  const int exit_code = proc.wait(reproc::infinite).first;
  if (expect_exit_0) {
    EXPECT_EQ(exit_code, 0) << "Process exited with non-zero code. --- "
                            << fmt::format("{}", fmt::join(args, " "));
  }
  return ProcessResult{std::move(stdout), std::move(stderr), exit_code};
}

static std::optional<ProcessResult>
run_wrapper_process(const CcelerateServerContext &server_ctx,
                    const std::vector<std::string> &args,
                    const bool expect_exit_0 = false) {
  reproc::options options;
  const std::vector<std::pair<std::string, std::string>> extra_env = {
      std::make_pair("CCELERATE_ENDPOINT", server_ctx.endpoint())};
  options.env.extra = extra_env;
  return run_and_get_result(args, std::move(options), expect_exit_0);
}

TEST(Integration, ClangNoArgs) {
  const Args &args = Args::get();
  CcelerateServerContext server_ctx;
  const std::optional<ProcessResult> result = run_wrapper_process(
      server_ctx, {args.test_build_dir / "ccelerate_clang++"});
  ASSERT_TRUE(result);
  EXPECT_EQ(result->stdout, "");
  EXPECT_NE(result->stderr.find("no input files"), std::string::npos);
  EXPECT_EQ(result->exit_code, 1);
}

TEST(Integration, HelloWorld) {
  const Args &args = Args::get();
  const std::filesystem::path project_dir =
      args.test_projects_dir / "hello_world";
  const std::filesystem::path output_file = args.test_out_dir / "hello_world";
  std::filesystem::create_directories(output_file.parent_path());
  CcelerateServerContext server_ctx;
  run_wrapper_process(server_ctx,
                      {args.test_build_dir / "ccelerate_clang++", "-o",
                       output_file, project_dir / "hello_world.cc"});
  const std::optional<ProcessResult> result = run_and_get_result({output_file});
  ASSERT_TRUE(result);
  EXPECT_EQ(result->stdout, "Hello World!\n");
  EXPECT_EQ(result->stderr, "");
  EXPECT_EQ(result->exit_code, 0);
}

TEST(Integration, BasicCMake) {
  const Args &args = Args::get();
  const std::filesystem::path project_dir =
      args.test_projects_dir / "basic_cmake";
  const std::filesystem::path output_dir = args.test_out_dir / "basic_cmake";
  std::filesystem::create_directories(output_dir);
  CcelerateServerContext server_ctx;
  run_wrapper_process(server_ctx,
                      {args.test_build_dir / "ccelerate_cmake", "-B",
                       output_dir, "-S", project_dir},
                      true);
  run_wrapper_process(
      server_ctx,
      {args.test_build_dir / "ccelerate_cmake", "--build", output_dir}, true);
  const std::optional<ProcessResult> result =
      run_and_get_result({output_dir / "basic_cmake"});
  ASSERT_TRUE(result);
  EXPECT_EQ(result->stdout, "It worked!\n");
  EXPECT_EQ(result->stderr, "");
  EXPECT_EQ(result->exit_code, 0);
}

} // namespace ccelerate::tests

int main(int argc, char **argv) {
  using namespace ccelerate::tests;
  ::testing::InitGoogleTest(&argc, argv);

  CLI::App app{"Integration tests for ccelerate"};
  argv = app.ensure_utf8(argv);

  Args &args = Args::get();
  app.add_option("--repo-dir", args.repo_dir,
                 "Path to the ccelerate repository")
      ->required();
  app.add_flag("--external-server", args.use_external_server,
               "Use an existing external ccelerate server for the tests");

  CLI11_PARSE(app, argc, argv);

  args.test_projects_dir = args.repo_dir / "test_projects";
  args.test_build_dir = ccelerate::get_current_executable_path().parent_path();
  args.test_out_dir = args.test_build_dir / "tests_tmp";

  std::filesystem::remove_all(args.test_out_dir);
  return RUN_ALL_TESTS();
}