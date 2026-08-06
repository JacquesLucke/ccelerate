// SPDX-License-Identifier: MIT

#pragma once

#include "run_process.hh"

namespace ccelerate {

ProcessResult run_process_traced(const ProcessArgs &args);
ExitCodeOrError run_process_stream_output_traced(
    const ProcessArgs &args,
    function_ref<void(string stdout_data, string stderr_data)> on_output_fn);

} // namespace ccelerate
