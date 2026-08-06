// SPDX-License-Identifier: MIT

#include <CLI/CLI.hpp>
#include <clang/Basic/Version.h>
#include <clang/CodeGen/CodeGenAction.h>
#include <clang/Driver/Compilation.h>
#include <clang/Driver/Driver.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/CompilerInvocation.h>
#include <clang/Frontend/FrontendOptions.h>
#include <clang/Frontend/TextDiagnosticBuffer.h>
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

struct Cmd_Preprocess {
  vector<string> clang_args;
};

static int handle__parse_args(const Cmd_ParseArgs &args) {
  const path self_path = get_current_executable_path();
  std::string clang_path = self_path.parent_path().parent_path() /
                           "vcpkg_installed/x64-linux/tools/llvm" / args.binary;
  std::string libstdcxx_path =
      self_path.parent_path().parent_path().parent_path().parent_path() /
      "extern" / "libstdc++";

  vector<const char *> driver_args;
  driver_args.push_back(clang_path.c_str());
  driver_args.push_back("-stdlib=libstdc++");
  const string toolchain_arg =
      fmt::format("--gcc-toolchain={}", libstdcxx_path);
  driver_args.push_back(toolchain_arg.c_str());
  for (const string &arg : args.clang_args) {
    driver_args.push_back(arg.c_str());
  }

  clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_ops =
      new clang::DiagnosticOptions();
  clang::TextDiagnosticPrinter *diag_client =
      new clang::TextDiagnosticPrinter(llvm::errs(), &*diag_ops);
  clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
      new clang::DiagnosticIDs());
  clang::DiagnosticsEngine diags(diag_id, diag_ops, diag_client);

  std::string target_triple = llvm::sys::getProcessTriple();
  clang::driver::Driver driver(
      clang_path, target_triple, diags, "clang LLVM Compiler");
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
  if (driver.getDiags().hasErrorOccurred()) {
    return 1;
  }
  const auto &jobs = compilation->getJobs();
  if (jobs.empty()) {
    return 0;
  }
  fwrite(clang_io::magic.data(), clang_io::magic.size(), 1, stdout);
  clang_io::ParsedArgs parsed_args;
  for (auto &job : jobs) {
    clang_io::Command command;
    command.executable = job.getExecutable();
    for (const auto &arg : job.getArguments()) {
      command.args.push_back(arg);
    }
    for (const clang::driver::InputInfo &input_info : job.getInputInfos()) {
      clang_io::InputInfo io_input;
      io_input.type = input_info.getType();
      if (const char *filename = input_info.getFilename()) {
        io_input.filename = string(filename);
      }
      command.input_infos.push_back(std::move(io_input));
    }
    for (const string &output_files : job.getOutputFilenames()) {
      command.output_files.push_back(output_files);
    }
    parsed_args.commands.push_back(command);
  }

  msgpack::sbuffer buffer;
  msgpack::pack(buffer, parsed_args);
  fwrite(buffer.data(), buffer.size(), 1, stdout);
  return 0;
}

static int handle__preprocess(const Cmd_Preprocess &args) {
  // The driver job args start with "-cc1"; CompilerInvocation expects the
  // remaining cc1 options only (same as clang's cc1_main entry point).
  vector<const char *> cc1_args;
  cc1_args.reserve(args.clang_args.size());
  for (const string &arg : args.clang_args) {
    if (cc1_args.empty() && arg == "-cc1") {
      continue;
    }
    cc1_args.push_back(arg.c_str());
  }

  llvm::InitializeAllTargets();
  llvm::InitializeAllTargetMCs();
  llvm::InitializeAllAsmPrinters();
  llvm::InitializeAllAsmParsers();

  clang::CompilerInstance clang_instance;
  clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
      new clang::DiagnosticIDs());
  clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_opts(
      new clang::DiagnosticOptions());
  clang::TextDiagnosticBuffer *diags_buffer = new clang::TextDiagnosticBuffer();
  clang::DiagnosticsEngine diags(diag_id, diag_opts, diags_buffer);

  bool success = clang::CompilerInvocation::CreateFromArgs(
      clang_instance.getInvocation(), cc1_args, diags);

  clang_instance.createDiagnostics();
  if (!clang_instance.hasDiagnostics()) {
    return 1;
  }
  diags_buffer->FlushDiagnostics(clang_instance.getDiagnostics());
  if (!success) {
    return 1;
  }

  success = clang::ExecuteCompilerInvocation(&clang_instance);
  return success ? 0 : 1;
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

  CLI::App &preprocess_cmd = *app.add_subcommand("preprocess");
  Cmd_Preprocess preprocess_args;
  preprocess_cmd.add_option(
      "passthrough", preprocess_args.clang_args, "Arguments passed to clang");

  CLI11_PARSE(app, argc, argv);

  if (parse_args_cmd) {
    return handle__parse_args(parse_args);
  } else if (preprocess_cmd) {
    return handle__preprocess(preprocess_args);
  } else {
    fmt::println(stderr, "Unknown command: {}", parse_args_cmd.get_name());
  }

  return 1;
}

} // namespace ccelerate

int main(int argc, char **argv) {
  return ccelerate::clang_ops_main(argc, argv);
}