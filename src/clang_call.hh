// SPDX-License-Identifier: MIT

#pragma once

#include "clang_for_ccelerate_io.hh"
#include "filesystem.hh"
#include "run_process.hh"

namespace ccelerate {

const path &get_clang_for_ccelerate_executable();

using ParseClangArgsResult = variant<ProcessResult, clang_io::ParsedArgs>;

ParseClangArgsResult parse_clang_args();

} // namespace ccelerate