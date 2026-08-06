// SPDX-License-Identifier: MIT

#include "get_current_executable_path.hh"
#include "request_handler.hh"
#include "run_process_traced.hh"

namespace ccelerate {

void handle_request__cmake(const Request &request) {
  const path binary_path = get_current_executable_path();
  const path dir = binary_path.parent_path();

  ProcessArgs args;
  args.arg("cmake")
      .args(request.args)
      .working_dir(request.working_dir)
      .env("CC", dir / "ccelerate_clang")
      .env("CXX", dir / "ccelerate_clang++");

  const bool has_build_arg = std::ranges::any_of(
      request.args, [](const string &arg) { return arg == "--build"; });
  if (!has_build_arg) {
    args.arg(fmt::format("-DCMAKE_AR={}", (dir / "ccelerate_ar").string()));
  }

  const ExitCodeOrError exit_code_or_error = run_process_stream_output_traced(
      args, [&](string stdout_data, string stderr_data) {
        send_response_incomplete(
            request.client_id, std::move(stdout_data), std::move(stderr_data));
      });
  string error_msg = exit_code_or_error.error_message().value_or("");
  const int exit_code = exit_code_or_error.exit_code().value_or(1);
  send_response_final(request.client_id, "", error_msg, exit_code);
}

} // namespace ccelerate