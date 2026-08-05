// SPDX-License-Identifier: MIT

#include "clang_for_ccelerate_io.hh"
#include "get_current_executable_path.hh"
#include "request_handler.hh"

namespace ccelerate {

static ExitCodeOrError handle_command__clang_cc1(const ClientID &client_id,
                                                 const string &executable,
                                                 const vector<string> &args,
                                                 const string &working_dir) {
  const ProcessResult cmd_result = run_process(
      ProcessArgs().arg(executable).args(args).working_dir(working_dir));
  send_response_incomplete(client_id,
                           std::move(cmd_result.stdout_data),
                           std::move(cmd_result.stderr_data));
  return cmd_result.exit_code_or_error();
}

static bool is_clang_cc1_command(const string_view &executable,
                                 const vector<string> &args) {
  if (executable.ends_with("clang++") || executable.ends_with("clang")) {
    if (args.size() >= 1) {
      if (args[0] == "-cc1") {
        return true;
      }
    }
  }
  return false;
}

void handle_request__clang(const Request &request) {
  const path self_path = get_current_executable_path();
  const path gcc_install_dir =
      self_path.parent_path() /
      "extern/gcc-13-install/lib/gcc/x86_64-linux-gnu/13";
  const ProcessResult parse_result = run_process(
      ProcessArgs()
          .arg(self_path.parent_path() / "clang_for_ccelerate")
          .args({"parse_args",
                 "--cwd",
                 request.working_dir,
                 "--binary",
                 to_string(request.program)})
          .arg("--")
          .args(request.args)
          .arg(fmt::format("--gcc-install-dir={}", gcc_install_dir.string()))
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
    ExitCodeOrError exit_or_error{1};
    if (is_clang_cc1_command(command.executable, command.args)) {
      exit_or_error = handle_command__clang_cc1(request.client_id,
                                                command.executable,
                                                command.args,
                                                request.working_dir);
    } else {
      exit_or_error = run_process_stream_output(
          ProcessArgs()
              .arg(command.executable)
              .args(command.args)
              .working_dir(request.working_dir),
          [&](string stdout_data, string stderr_data) {
            send_response_incomplete(request.client_id,
                                     std::move(stdout_data),
                                     std::move(stderr_data));
          });
    }
    if (exit_or_error.exit_code() != 0) {
      send_response_final(request.client_id,
                          "",
                          exit_or_error.error_message().value_or(""),
                          exit_or_error.exit_code().value_or(1));
      return;
    }
  }
  send_response_final(request.client_id, "", "", 0);
}

} // namespace ccelerate