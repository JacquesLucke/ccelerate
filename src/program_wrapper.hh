// SPDX-License-Identifier: MIT

#pragma once

#include <string>
#include <vector>

#include <fmt/format.h>
#include <fmt/ranges.h>
#include <msgpack.hpp>

namespace ccelerate {
enum class WrappedProgram {
  Clang = 0,
  Clangxx = 1,
  Ar = 2,
  CMake = 3,
};
}
MSGPACK_ADD_ENUM(ccelerate::WrappedProgram);

namespace ccelerate {

struct WrappedProgramCall {
  WrappedProgram program;
  std::string cwd;
  std::vector<std::string> args;

  MSGPACK_DEFINE(program, cwd, args);
};

inline std::string_view to_string(const WrappedProgram wrapped_program) {
  switch (wrapped_program) {
  case WrappedProgram::Clang:
    return "clang";
  case WrappedProgram::Clangxx:
    return "clang++";
  case WrappedProgram::Ar:
    return "ar";
  case WrappedProgram::CMake:
    return "cmake";
  }
  return "unknown";
}

struct WrappedProgramResult {
  std::string stdout;
  std::string stderr;
  int exit_code;

  MSGPACK_DEFINE(stdout, stderr);
};

inline std::string format_as(const WrappedProgramCall &call) {
  return fmt::format("{} {}", to_string(call.program),
                     fmt::join(call.args, " "));
}

inline std::string get_socket_endpoint() { return "ipc:///tmp/ccelerate.ipc"; }

} // namespace ccelerate