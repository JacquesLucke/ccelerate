// SPDX-License-Identifier: MIT

#pragma once

#include <fmt/format.h>
#include <fmt/ranges.h>
#include <msgpack.hpp>

#include "base/optional.hh"
#include "base/string.hh"
#include "base/vector.hh"

namespace ccelerate::wrap_io {

enum class Program {
  Clang = 0,
  Clangxx = 1,
  Ar = 2,
  CMake = 3,
};
}
MSGPACK_ADD_ENUM(ccelerate::wrap_io::Program);

namespace ccelerate::wrap_io {

struct CallRequest {
  Program program;
  string working_dir;
  vector<string> args;

  MSGPACK_DEFINE(program, working_dir, args);
};

inline string_view to_string(const Program wrapped_program) {
  switch (wrapped_program) {
    case Program::Clang:
      return "clang";
    case Program::Clangxx:
      return "clang++";
    case Program::Ar:
      return "ar";
    case Program::CMake:
      return "cmake";
  }
  return "unknown";
}

struct CallResponseFrame {
  string stdout;
  string stderr;

  // Exit code is only set when the process finished.
  optional<int> exit_code;

  MSGPACK_DEFINE(stdout, stderr, exit_code);
};

inline string format_as(const CallRequest &call) {
  return fmt::format(
      "{} {}", to_string(call.program), fmt::join(call.args, " "));
}

} // namespace ccelerate::wrap_io