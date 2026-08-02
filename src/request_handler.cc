// SPDX-License-Identifier: MIT

#include "request_handler.hh"
#include "error_code.hh"
#include "run_process.hh"

namespace ccelerate {

void pass_through_external_call(const ClientID &client_id,
                                const ProcessArgs &args,
                                const bool is_final) {
  ProcessResult result = run_process(args);
  string final_stdout = std::move(result.stdout);
  string final_stderr = std::move(result.stderr);
  if (const optional<error_code> ec = result.error()) {
    final_stderr += "\n";
    final_stderr += ec->message();
    final_stderr += "\n";
  }
  if (is_final) {
    const int final_exit_code = result.exit_code().value_or(1);
    send_response_final(client_id,
                        std::move(final_stdout),
                        std::move(final_stderr),
                        final_exit_code);
  } else {
    send_response_incomplete(
        client_id, std::move(final_stdout), std::move(final_stderr));
  }
}

static void handle_request__eager(const Request &request) {
  pass_through_external_call(request.client_id,
                             ProcessArgs()
                                 .arg(to_string(request.program))
                                 .args(request.args)
                                 .working_dir(request.working_dir),
                             true);
}

void handle_request(const Request &request) {
  switch (request.program) {
    case wrap_io::Program::Clang:
    case wrap_io::Program::Clangxx:
    case wrap_io::Program::Ar: {
      handle_request__eager(request);
      break;
    }
    case wrap_io::Program::CMake:
      handle_request__cmake(request);
      break;
  }
}

} // namespace ccelerate