// SPDX-License-Identifier: MIT

#include "array.hh"
#include "clang_for_ccelerate_io.hh"
#include "get_current_executable_path.hh"
#include "request_handler.hh"
#include "run_process_traced.hh"

namespace ccelerate {

enum class ClangCC1ActionType {
  EmitPreprocessed,
  EmitObj,
};

static const path &get_clang_for_ccelerate_executable() {
  static const path executable = []() {
    const path self_path = get_current_executable_path();
    return self_path.parent_path() / "clang_for_ccelerate";
  }();
  return executable;
}

static const string &to_arg_name(const ClangCC1ActionType action) {
  switch (action) {
    case ClangCC1ActionType::EmitPreprocessed: {
      static const string name = "-E";
      return name;
    }
    case ClangCC1ActionType::EmitObj: {
      static const string name = "-emit-obj";
      return name;
    }
  }
  static const string name = "";
  return name;
}

static span<const ClangCC1ActionType> get_clang_cc1_actions_types() {
  static const array<ClangCC1ActionType, 2> actions = {
      ClangCC1ActionType::EmitPreprocessed,
      ClangCC1ActionType::EmitObj,
  };
  return actions;
}

static std::optional<ClangCC1ActionType>
get_clang_cc1_action_type(const span<const string> args) {
  for (const string &arg : args) {
    for (const ClangCC1ActionType action : get_clang_cc1_actions_types()) {
      if (arg == to_arg_name(action)) {
        return action;
      }
    }
  }
  return nullopt;
}

static void arg_rewrite__replace_arg(const span<string> args,
                                     const string_view old_arg,
                                     const string_view new_arg) {
  for (size_t i = 0; i < args.size(); i++) {
    if (args[i] == old_arg) {
      args[i] = new_arg;
      return;
    }
  }
}

// static void arg_rewrite__remove_dual_arg(vector<string> &args,
//                                          const string_view first_arg) {
//   for (size_t i = 0; i < args.size() - 1; i++) {
//     if (args[i] == first_arg) {
//       args.erase(args.begin() + i, args.begin() + i + 2);
//       return;
//     }
//   }
// }

static void arg_write__replace_dual_arg_value(vector<string> &args,
                                              const string_view name,
                                              string new_value) {
  for (size_t i = 0; i < args.size() - 1; i++) {
    if (args[i] == name) {
      args[i + 1] = new_value;
      return;
    }
  }
}

static optional<string>
arg_read__get_dual_arg_value(const span<const string> args,
                             const string_view name) {
  for (size_t i = 0; i < args.size() - 1; i++) {
    if (args[i] == name) {
      return args[i + 1];
    }
  }
  return nullopt;
}

static vector<string>
rewrite_clang_cc1_args__emit_obj__to__emit_preprocessed(vector<string> args,
                                                        string output_file) {
  arg_rewrite__replace_arg(args, "-emit-obj", "-E");
  arg_write__replace_dual_arg_value(args, "-o", std::move(output_file));
  return args;
}

static vector<string> rewrite_clang_cc1_args__change_source_file(
    vector<string> args, string new_source_file, string type_str) {
  for (size_t i = 0; i < args.size() - 2; i++) {
    if (args[i] == "-x") {
      args[i + 1] = type_str;
      args[i + 2] = new_source_file;
      return args;
    }
  }
  return args;
}

static ExitCodeOrError
handle_command__clang_cc1(const ClientID &client_id,
                          const clang_io::Command &command,
                          const string &working_dir) {
  const optional<ClangCC1ActionType> action_type_opt =
      get_clang_cc1_action_type(command.args);
  if (!action_type_opt.has_value() ||
      action_type_opt == ClangCC1ActionType::EmitPreprocessed) {
    return run_process_stream_output_traced(
        ProcessArgs()
            .arg(command.executable)
            .args(command.args)
            .working_dir(working_dir),
        [&](string stdout_data, string stderr_data) {
          send_response_incomplete(
              client_id, std::move(stdout_data), std::move(stderr_data));
        });
  }
  switch (*action_type_opt) {
    case ClangCC1ActionType::EmitPreprocessed: {
      /* Handled above already. */
      break;
    }
    case ClangCC1ActionType::EmitObj: {
      const clang::driver::types::ID orig_input_type =
          command.input_infos[0].type;
      const clang::driver::types::ID preprocessed_type =
          clang::driver::types::getPreprocessedType(orig_input_type);
      const string preprocess_file_suffix =
          clang::driver::types::getTypeTempSuffix(preprocessed_type);

      const optional<path> obj_file =
          arg_read__get_dual_arg_value(command.args, "-o");
      if (!obj_file.has_value()) {
        // TODO
        return {1};
      }
      const path &clang_for_ccelerate_exe =
          get_clang_for_ccelerate_executable();
      const string preprocessed_file =
          fmt::format("{}.{}", obj_file->string(), preprocess_file_suffix);
      const string local_code_file = fmt::format(
          "{}.local.{}", obj_file->string(), obj_file->extension().string());
      vector<string> preprocess_args =
          rewrite_clang_cc1_args__emit_obj__to__emit_preprocessed(
              vector<string>(command.args.begin(), command.args.end()),
              preprocessed_file);
      ProcessResult preprocess_result =
          run_process_traced(ProcessArgs()
                                 .arg(clang_for_ccelerate_exe)
                                 .args({"preprocess", "--"})
                                 .args(preprocess_args)
                                 .working_dir(working_dir));
      if (preprocess_result.exit_code() != 0) {
        send_response_incomplete(client_id,
                                 std::move(preprocess_result.stdout_data),
                                 std::move(preprocess_result.stderr_data));
        return preprocess_result.exit_code_or_error();
      }
      run_process_traced(ProcessArgs()
                             .arg(clang_for_ccelerate_exe)
                             .args({"extract_local_code",
                                    "--local-code-path",
                                    local_code_file,
                                    "--"})
                             .args(command.args)
                             .working_dir(working_dir));
      vector<string> compile_args = rewrite_clang_cc1_args__change_source_file(
          command.args,
          preprocessed_file,
          clang::driver::types::getTypeName(preprocessed_type));
      return run_process_stream_output_traced(
          ProcessArgs()
              .arg(clang_for_ccelerate_exe)
              .args({"compile_obj", "--"})
              .args(compile_args)
              .working_dir(working_dir),
          [&](string stdout_data, string stderr_data) {
            send_response_incomplete(
                client_id, std::move(stdout_data), std::move(stderr_data));
          });
    }
  }
  // Fallback handling.
  const ProcessResult cmd_result =
      run_process_traced(ProcessArgs()
                             .arg(command.executable)
                             .args(command.args)
                             .working_dir(working_dir));
  send_response_incomplete(client_id,
                           std::move(cmd_result.stdout_data),
                           std::move(cmd_result.stderr_data));
  return cmd_result.exit_code_or_error();
}

static bool is_clang_cc1_command(const string_view &executable,
                                 const span<const string> &args) {
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
  const path &clang_for_ccelerate_exe = get_clang_for_ccelerate_executable();
  const path self_path = get_current_executable_path();
  const path gcc_install_dir =
      self_path.parent_path() /
      "extern/gcc-13-install/lib/gcc/x86_64-linux-gnu/13";
  const ProcessResult parse_result = run_process_traced(
      ProcessArgs()
          .arg(clang_for_ccelerate_exe)
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
      exit_or_error = handle_command__clang_cc1(
          request.client_id, command, request.working_dir);
    } else {
      exit_or_error = run_process_stream_output_traced(
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