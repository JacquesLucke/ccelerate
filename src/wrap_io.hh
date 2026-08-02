// SPDX-License-Identifier: MIT

#pragma once

#include <fmt/format.h>
#include <fmt/ranges.h>
#include <msgpack.hpp>

#include "base/optional.hh"
#include "base/string.hh"
#include "base/vector.hh"

namespace ccelerate::wrap_io {

enum class WrappedProgram {
  Clang = 0,
  Clangxx = 1,
  Ar = 2,
  CMake = 3,
};
}
MSGPACK_ADD_ENUM(ccelerate::wrap_io::WrappedProgram);

namespace ccelerate::wrap_io {

struct WrappedProgramCall {
  WrappedProgram program;
  string cwd;
  vector<string> args;

  MSGPACK_DEFINE(program, cwd, args);
};

inline string_view to_string(const WrappedProgram wrapped_program) {
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
  string stdout;
  string stderr;

  // Exit code is only set when the process finished.
  optional<int> exit_code;

  MSGPACK_DEFINE(stdout, stderr, exit_code);
};

inline string format_as(const WrappedProgramCall &call) {
  return fmt::format(
      "{} {}", to_string(call.program), fmt::join(call.args, " "));
}

} // namespace ccelerate::wrap_io