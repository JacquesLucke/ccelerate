// SPDX-License-Identifier: MIT

#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>

#include "error_code.hh"
#include "get_current_executable_path.hh"
#include "pair.hh"
#include "request_handler.hh"

namespace ccelerate {

static void pass_through_external_call(const ClientID &client_id,
                                       const vector<string> &args,
                                       reproc::options options) {
  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  reproc::process proc;
  error_code ec = proc.start(args, options);
  std::string program_stdout;
  std::string program_stderr;
  int exit_code = 0;
  if (!ec) {
    ec = reproc::drain(proc,
                       reproc::sink::string(program_stdout),
                       reproc::sink::string(program_stderr));
    if (!ec) {
      std::tie(exit_code, ec) = proc.wait(reproc::infinite);
    } else {
      exit_code = 1;
      program_stderr = ec.message();
    }
  } else {
    exit_code = 1;
    program_stderr = ec.message();
  }

  send_response_final(client_id, program_stdout, program_stderr, exit_code);
}

static void handle_eager_program_call(const Request &request) {
  reproc::options options;
  options.working_directory = request.working_dir.c_str();
  vector<string> args;
  args.push_back(string(to_string(request.program)));
  for (const auto &arg : request.args) {
    args.push_back(arg);
  }
  pass_through_external_call(request.client_id, args, std::move(options));
}

static void handle_cmake_call(const Request &request) {
  const path binary_path = get_current_executable_path();
  const path dir = binary_path.parent_path();

  reproc::options options;
  options.working_directory = request.working_dir.c_str();
  const vector<pair<string, string>> extra_env = {
      {"CC", dir / "ccelerate_clang"},
      {"CXX", dir / "ccelerate_clang++"},
  };
  options.env.extra = extra_env;
  vector<string> args;
  args.push_back("cmake");
  bool has_build_arg = false;
  for (const auto &arg : request.args) {
    args.push_back(arg);
    if (arg == "--build") {
      has_build_arg = true;
    }
  }
  if (!has_build_arg) {
    args.push_back(
        fmt::format("-DCMAKE_AR={}", (dir / "ccelerate_ar").string()));
  }
  pass_through_external_call(request.client_id, args, std::move(options));
}

void handle_request(const Request &request) {
  switch (request.program) {
    case wrap_io::Program::Clang:
    case wrap_io::Program::Clangxx:
    case wrap_io::Program::Ar: {
      handle_eager_program_call(request);
      break;
    }
    case wrap_io::Program::CMake:
      handle_cmake_call(request);
      break;
  }
}

} // namespace ccelerate