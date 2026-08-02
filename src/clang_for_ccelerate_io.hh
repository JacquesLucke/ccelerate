// SPDX-License-Identifier: MIT

#pragma once

#include <msgpack.hpp>

#include "string.hh"
#include "vector.hh"

namespace ccelerate::clang_io {

struct Command {
  string executable;
  vector<string> args;

  MSGPACK_DEFINE(executable, args);
};

struct ParsedArgs {
  vector<Command> commands;

  MSGPACK_DEFINE(commands);
};

} // namespace ccelerate::clang_io