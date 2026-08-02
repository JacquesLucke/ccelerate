// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <clang/Basic/Version.h>
#include <clang/CodeGen/CodeGenAction.h>
#include <clang/Driver/Compilation.h>
#include <clang/Driver/Driver.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/FrontendOptions.h>
#include <clang/Frontend/TextDiagnosticPrinter.h>
#include <clang/FrontendTool/Utils.h>
#include <filesystem>
#include <fmt/format.h>
#include <llvm/Config/llvm-config.h>
#include <llvm/Support/TargetSelect.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <llvm/TargetParser/Host.h>

#include "clang_for_ccelerate_io.hh"
#include "get_current_executable_path.hh"
#include "string.hh"

namespace ccelerate {

struct Cmd_ParseArgs {
  path cwd;
  string binary;
  vector<string> clang_args;
};

static int handle__parse_args(const Cmd_ParseArgs &args) {
  const path self_path = get_current_executable_path();
  std::string clang_path = self_path.parent_path().parent_path() /
                           "vcpkg_installed/x64-linux/tools/llvm" / args.binary;

  vector<const char *> driver_args;
  driver_args.push_back(clang_path.c_str());
  for (const string &arg : args.clang_args) {
    driver_args.push_back(arg.c_str());
  }

  std::string captured_stdout;
  std::string captured_stderr;
  llvm::raw_string_ostream stderr_stream(captured_stderr);

  clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_ops =
      new clang::DiagnosticOptions();
  clang::TextDiagnosticPrinter *diag_client =
      new clang::TextDiagnosticPrinter(stderr_stream, &*diag_ops);
  clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
      new clang::DiagnosticIDs());
  clang::DiagnosticsEngine diags(diag_id, diag_ops, diag_client);

  clang::CompilerInstance clang;

  llvm::IntrusiveRefCntPtr<llvm::vfs::FileSystem> vfs =
      llvm::vfs::createPhysicalFileSystem();
  std::error_code ec = vfs->setCurrentWorkingDirectory(args.cwd.string());
  (void)ec;

  std::string target_triple = llvm::sys::getProcessTriple();
  clang::driver::Driver driver(
      clang_path, target_triple, diags, "clang LLVM Compiler", vfs);
  std::unique_ptr<clang::driver::Compilation> compilation{
      driver.BuildCompilation(driver_args)};
  if (!compilation) {
    fmt::println(stderr, "Failed to build compilation");
    return 1;
  }
  if (compilation->containsError()) {
    fmt::println(stderr, "Compilation contains error");
    return 1;
  }

  clang_io::ParsedArgs parsed_args;

  for (auto &job : compilation->getJobs()) {
    clang_io::Command command;
    command.executable = job.getExecutable();
    for (const auto &arg : job.getArguments()) {
      command.args.push_back(arg);
    }
    parsed_args.commands.push_back(command);
  }

  msgpack::sbuffer buffer;
  msgpack::pack(buffer, parsed_args);
  fwrite(buffer.data(), buffer.size(), 1, stdout);
  return 0;
}

int clang_ops_main(const int argc, char **argv) {
  CLI::App app{"clang_for_ccelerate"};
  app.description("A utility used by ccelerate to do clang specific things "
                  "like argument parsing");
  CLI::App &parse_args_cmd = *app.add_subcommand("parse_args");
  Cmd_ParseArgs parse_args;
  parse_args_cmd.add_option("--cwd", parse_args.cwd)->required();
  parse_args_cmd.add_option("--binary", parse_args.binary)->required();
  parse_args_cmd.add_option(
      "passthrough", parse_args.clang_args, "Arguments passed to clang");

  CLI11_PARSE(app, argc, argv);

  if (parse_args_cmd) {
    return handle__parse_args(parse_args);
  } else {
    fmt::println(stderr, "Unknown command: {}", parse_args_cmd.get_name());
  }

  return 1;
}

} // namespace ccelerate

int main(int argc, char **argv) {
  return ccelerate::clang_ops_main(argc, argv);
}