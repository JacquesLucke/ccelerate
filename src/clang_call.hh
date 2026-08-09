// SPDX-License-Identifier: MIT

#pragma once

#include "clang_for_ccelerate_io.hh"
#include "filesystem.hh"
#include "run_process.hh"

namespace ccelerate {

const path &get_clang_for_ccelerate_executable();

using ParseClangArgsResult = variant<ProcessResult, clang_io::ParsedArgs>;

ParseClangArgsResult parse_clang_args(span<const string> args,
                                      const path &working_dir,
                                      const string_view clang_name);

ProcessResult extract_local_code_with_clang(span<const string> cc1_args,
                                            const path &output_path,
                                            const path &working_dir,
                                            string_view local_id,
                                            span<const path> config_paths);

} // namespace ccelerate
