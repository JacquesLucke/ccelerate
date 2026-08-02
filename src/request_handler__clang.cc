// SPDX-License-Identifier: MIT

#include "clang_for_ccelerate_io.hh"
#include "get_current_executable_path.hh"
#include "request_handler.hh"

namespace ccelerate {

void handle_request__clang(const Request &request) {
  const path self_path = get_current_executable_path();
  const ProcessResult parse_result =
      run_process(ProcessArgs()
                      .arg(self_path.parent_path() / "clang_for_ccelerate")
                      .args({"parse_args",
                             "--cwd",
                             request.working_dir,
                             "--binary",
                             to_string(request.program)})
                      .arg("--")
                      .args(request.args)
                      .working_dir(request.working_dir));
  if (parse_result.exit_code() != 0) {
    send_response_final(request.client_id,
                        std::move(parse_result.stdout_data),
                        std::move(parse_result.stderr_data),
                        parse_result.exit_code().value_or(1));
    return;
  }
  if (!string_view(parse_result.stdout_data).starts_with(clang_io::magic)) {
    send_response_final(request.client_id,
                        parse_result.stdout_data,
                        parse_result.stderr_data,
                        0);
    return;
  }

  clang_io::ParsedArgs parsed_args;
  try {
    string_view stdout_to_parse =
        string_view(parse_result.stdout_data).substr(clang_io::magic.size());
    msgpack::unpack(stdout_to_parse.data(), stdout_to_parse.size())
        .get()
        .convert(parsed_args);
  } catch (...) {
    send_response_error(request.client_id,
                        "internal error parsing clang_for_ccelerate output");
    return;
  }

  if (parsed_args.commands.empty()) {
    send_response_final(request.client_id, "", "no input files", 1);
    return;
  }

  for (const clang_io::Command &command : parsed_args.commands) {
    const ProcessResult cmd_result =
        run_process(ProcessArgs()
                        .arg(command.executable)
                        .args(command.args)
                        .working_dir(request.working_dir));
    send_response_incomplete(request.client_id,
                             std::move(cmd_result.stdout_data),
                             std::move(cmd_result.stderr_data));
    if (cmd_result.exit_code() != 0) {
      send_response_final(
          request.client_id, "", "", cmd_result.exit_code().value_or(1));
      return;
    }
  }
  send_response_final(request.client_id, "", "", 0);
}

} // namespace ccelerate