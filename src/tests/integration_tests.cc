// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <fmt/format.h>
#include <fmt/ranges.h>
#include <fstream>
#include <gmock/gmock.h>
#include <gtest/gtest.h>
#include <random>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>
#include <sstream>

#include "base/error_code.hh"
#include "base/filesystem.hh"
#include "base/get_current_executable_path.hh"
#include "base/pair.hh"
#include "base/run_process.hh"
#include "base/string.hh"
#include "base/vector.hh"
#include "clang_call.hh"
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
  EXPECT_EQ(result.exit_code(), 1) << result.stderr_data;
  EXPECT_EQ(result.stdout_data, "");
  EXPECT_THAT(result.stderr_data, testing::HasSubstr("no input files"));
}

TEST(Integration, HelloWorld) {
  const Args &args = Args::get();
  CcelerateServerContext server_ctx;

  const path project_dir = args.test_projects_dir / "hello_world";
  const path output_file = args.test_out_dir / "hello_world";
  std::filesystem::create_directories(output_file.parent_path());

  const ProcessResult build_result =
      run_process(ProcessArgs()
                      .args({args.binary_dir / "ccelerate_clang",
                             "-o",
                             output_file,
                             project_dir / "hello_world.cc"})
                      .envs(server_ctx.env));
  ASSERT_EQ(build_result.exit_code(), 0) << build_result.stderr_data;

  const ProcessResult run_result = run_process(ProcessArgs().arg(output_file));
  ASSERT_EQ(run_result.exit_code(), 0);
  EXPECT_EQ(run_result.stdout_data, "Hello World!\n");
  EXPECT_EQ(run_result.stderr_data, "");
}

static void test_simple_cmake_project(const string_view project_name,
                                      const string_view expected_stdout) {
  const Args &args = Args::get();
  CcelerateServerContext server_ctx;

  const path project_dir = args.test_projects_dir / project_name;
  const path output_dir = args.test_out_dir / project_name;
  std::filesystem::remove_all(output_dir);
  std::filesystem::create_directories(output_dir);

  const ProcessResult configure_result =
      run_process(ProcessArgs()
                      .args({args.binary_dir / "ccelerate_cmake",
                             "-B",
                             output_dir,
                             "-S",
                             project_dir,
                             "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"})
                      .envs(server_ctx.env));
  ASSERT_EQ(configure_result.exit_code(), 0) << configure_result.stderr_data;

  const ProcessResult build_result =
      run_process(ProcessArgs()
                      .args({args.binary_dir / "ccelerate_cmake",
                             "--build",
                             output_dir,
                             "--parallel"})
                      .envs(server_ctx.env));
  ASSERT_EQ(build_result.exit_code(), 0) << build_result.stderr_data;

  const ProcessResult run_result =
      run_process(ProcessArgs().arg(output_dir / project_name));
  ASSERT_EQ(run_result.exit_code(), 0) << run_result.stderr_data;
  EXPECT_EQ(run_result.stdout_data, expected_stdout);
  EXPECT_EQ(run_result.stderr_data, "");
}

TEST(Integration, BasicCMake) {
  test_simple_cmake_project("basic_cmake", "It worked!\n");
}

TEST(Integration, MultiFileCMake) {
  test_simple_cmake_project("multi_file_cmake",
                            "Hello from C\nHello from C++\n");
}

TEST(Integration, MultiLibCMake) {
  test_simple_cmake_project("multi_lib_cmake", "ABCDEFGHIJKLMNOPQRST\n");
}

static string read_file(const path &file_path) {
  std::ifstream in(file_path);
  std::ostringstream contents;
  contents << in.rdbuf();
  return contents.str();
}

static void test_local_code(const string_view project_name,
                            const string_view binary) {
  const Args &args = Args::get();
  const path src_dir = args.repo_dir / "test_local_code";
  const path stem = path(project_name).stem();
  const path output_dir = args.test_out_dir / stem;
  std::filesystem::remove_all(output_dir);
  std::filesystem::create_directories(output_dir);

  ParseClangArgsResult parse_args_result = parse_clang_args(
      vector<string>{"-c", string(project_name)}, src_dir, binary);
  ASSERT_TRUE(std::holds_alternative<clang_io::ParsedArgs>(parse_args_result))
      << std::get<ProcessResult>(parse_args_result).stderr_data;
  const clang_io::ParsedArgs &parsed_args =
      std::get<clang_io::ParsedArgs>(parse_args_result);
  ASSERT_EQ(parsed_args.commands.size(), 1);

  const path output_path = output_dir / (stem.string() + ".local.ii");
  const path reference_path = src_dir / (stem.string() + ".local-reference.ii");

  const ProcessResult extract_result = extract_local_code_with_clang(
      parsed_args.commands[0].args, output_path, src_dir);
  ASSERT_EQ(extract_result.exit_code(), 0) << extract_result.stderr_data;

  ASSERT_TRUE(std::filesystem::exists(output_path)) << output_path;
  ASSERT_TRUE(std::filesystem::exists(reference_path)) << reference_path;

  const string reference_output = read_file(reference_path);
  const string actual_output = read_file(output_path);

  EXPECT_EQ(actual_output, reference_output);
}

TEST(LocalCode, LocalVariable) {
  test_local_code("local_variable.cc", "clang++");
}

} // namespace ccelerate::tests

int main(int argc, char **argv) {
  using namespace ccelerate::tests;
  ::testing::InitGoogleTest(&argc, argv);

  if (testing::GTEST_FLAG(list_tests)) {
    return RUN_ALL_TESTS();
  }

  CLI::App app{"Integration tests for ccelerate"};
  argv = app.ensure_utf8(argv);

  Args &args = Args::get();
  app.add_option(
         "--repo-dir", args.repo_dir, "path to the ccelerate repository")
      ->required();
  app.add_flag("--external-server",
               args.use_external_server,
               "Use an existing external ccelerate server for the tests")
      ->envname("CCELERATE_TEST_EXTERNAL_SERVER");

  CLI11_PARSE(app, argc, argv);

  args.test_projects_dir = args.repo_dir / "test_projects";
  args.binary_dir = ccelerate::get_current_executable_path().parent_path();
  args.test_out_dir = args.binary_dir / "tests_tmp";

  return RUN_ALL_TESTS();
}