// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <gtest/gtest.h>
#include <random>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>

static std::string get_random_endpoint() {
  static std::random_device rd;
  static std::mt19937 gen(rd());
  static std::uniform_int_distribution<int> dist(
      0, std::numeric_limits<int>::max());
  return fmt::format("ipc:///tmp/ccelerate-test-{}.ipc", dist(gen));
}

class CcelerateServerContext {
private:
  reproc::process server_process_;
  std::string endpoint_;

public:
  CcelerateServerContext() {
    endpoint_ = get_random_endpoint();
    reproc::options options;
    const std::vector<std::pair<std::string, std::string>> extra_env = {
        std::make_pair("CCELERATE_ENDPOINT", endpoint_)};
    options.env.extra = extra_env;

    std::vector<std::string> args;
    args.push_back("./ccelerate");
    const std::error_code ec = server_process_.start(args, options);
    EXPECT_FALSE(ec) << "Failed to start server: " << ec.message();
  }

  ~CcelerateServerContext() {
    const std::error_code ec = server_process_.terminate();
    EXPECT_FALSE(ec) << "Failed to terminate server: " << ec.message();
  }

  const std::string &endpoint() const { return endpoint_; }
};

struct ProcessResult {
  std::string stdout;
  std::string stderr;
  int exit_code;
};

static std::optional<ProcessResult>
run_wrapper_process(const CcelerateServerContext &server_ctx,
                    const std::vector<std::string> &args) {
  reproc::options options;
  const std::vector<std::pair<std::string, std::string>> extra_env = {
      std::make_pair("CCELERATE_ENDPOINT", server_ctx.endpoint())};
  options.env.extra = extra_env;
  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  reproc::process proc;
  std::error_code ec = proc.start(args, options);
  EXPECT_FALSE(ec) << "Failed to start process: " << ec.message();
  std::string stdout;
  std::string stderr;
  ec = reproc::drain(proc, reproc::sink::string(stdout),
                     reproc::sink::string(stderr));
  EXPECT_FALSE(ec) << "Failed to drain process: " << ec.message();
  if (ec) {
    return std::nullopt;
  }
  return ProcessResult{std::move(stdout), std::move(stderr),
                       proc.wait(reproc::infinite).first};
}

TEST(InvokeCommand, ClangNoArgs) {
  CcelerateServerContext server_ctx;
  const std::optional<ProcessResult> result =
      run_wrapper_process(server_ctx, {"./ccelerate_clang"});
  ASSERT_TRUE(result);
  EXPECT_EQ(result->stdout, "");
  EXPECT_EQ(result->stderr, "clang: error: no input files\n");
  EXPECT_EQ(result->exit_code, 1);
}
