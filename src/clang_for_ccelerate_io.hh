// SPDX-License-Identifier: MIT

#pragma once

#include <clang/Driver/Types.h>
#include <msgpack.hpp>

#include "filesystem.hh"
#include "optional.hh"
#include "string.hh"
#include "vector.hh"

MSGPACK_ADD_ENUM(clang::driver::types::ID);

namespace ccelerate {

struct PathMap {
  std::filesystem::path path;
  string replacement;
};

namespace clang_io {

static const string magic = "VALID_RETURN_STRUCT";

struct InputInfo {
  clang::driver::types::ID type;
  optional<string> filename;

  MSGPACK_DEFINE(type, filename);
};

struct Command {
  string executable;
  vector<string> args;
  vector<InputInfo> input_infos;
  vector<string> output_files;

  MSGPACK_DEFINE(executable, args, input_infos, output_files);
};

struct ParsedArgs {
  vector<Command> commands;

  MSGPACK_DEFINE(commands);
};

} // namespace clang_io

} // namespace ccelerate