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

inline const std::string &to_string(const WrappedProgram wrapped_program) {
  switch (wrapped_program) {
  case WrappedProgram::Clang: {
    static const std::string str = "clang";
    return str;
  }
  case WrappedProgram::Clangxx: {
    static const std::string str = "clang++";
    return str;
  }
  case WrappedProgram::Ar: {
    static const std::string str = "ar";
    return str;
  }
  case WrappedProgram::CMake: {
    static const std::string str = "cmake";
    return str;
  }
  }
  static const std::string unknown = "unknown";
  return unknown;
}

struct WrappedProgramResult {
  std::string stdout;
  std::string stderr;

  // Exit code is only set when the process finished.
  std::optional<int> exit_code;

  MSGPACK_DEFINE(stdout, stderr, exit_code);
};

inline std::string format_as(const WrappedProgramCall &call) {
  return fmt::format("{} {}", to_string(call.program),
                     fmt::join(call.args, " "));
}

} // namespace ccelerate