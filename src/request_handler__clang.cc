// SPDX-License-Identifier: MIT

#include "array.hh"
#include "clang_call.hh"
#include "clang_for_ccelerate_io.hh"
#include "get_current_executable_path.hh"
#include "request_handler.hh"
#include "run_process_traced.hh"

namespace ccelerate {

enum class ClangCC1ActionType {
  EmitPreprocessed,
  EmitObj,
};

const path &get_clang_for_ccelerate_executable() {
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

// static void arg_rewrite__replace_arg(const span<string> args,
//                                      const string_view old_arg,
//                                      const string_view new_arg) {
//   for (size_t i = 0; i < args.size(); i++) {
//     if (args[i] == old_arg) {
//       args[i] = new_arg;
//       return;
//     }
//   }
// }

// static void arg_rewrite__remove_dual_arg(vector<string> &args,
//                                          const string_view first_arg) {
//   for (size_t i = 0; i < args.size() - 1; i++) {
//     if (args[i] == first_arg) {
//       args.erase(args.begin() + i, args.begin() + i + 2);
//       return;
//     }
//   }
// }

// static void arg_write__replace_dual_arg_value(vector<string> &args,
//                                               const string_view name,
//                                               string new_value) {
//   for (size_t i = 0; i < args.size() - 1; i++) {
//     if (args[i] == name) {
//       args[i + 1] = new_value;
//       return;
//     }
//   }
// }

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

// static vector<string> rewrite_clang_cc1_args__change_source_file(
//     vector<string> args, string new_source_file, string type_str) {
//   for (size_t i = 0; i < args.size() - 2; i++) {
//     if (args[i] == "-x") {
//       args[i + 1] = type_str;
//       args[i + 2] = new_source_file;
//       return args;
//     }
//   }
//   return args;
// }

ProcessResult extract_local_code_with_clang(span<const string> cc1_args,
                                            const path &output_path,
                                            const path &working_dir) {
  const path &clang_for_ccelerate_exe = get_clang_for_ccelerate_executable();
  return run_process_traced(
      ProcessArgs()
          .arg(clang_for_ccelerate_exe)
          .args({"local-code", "--local-code-path", output_path, "--"})
          .args(cc1_args)
          .working_dir(working_dir));
}

static ExitCodeOrError
handle_command__clang_cc1(const ClientID &client_id,
                          const clang_io::Command &command,
                          const string &working_dir) {
  const optional<ClangCC1ActionType> action_type_opt =
      get_clang_cc1_action_type(command.args);
  if (action_type_opt == ClangCC1ActionType::EmitObj) {
    const clang::driver::types::ID orig_input_type =
        command.input_infos[0].type;

    const optional<path> obj_file =
        arg_read__get_dual_arg_value(command.args, "-o");
    if (!obj_file.has_value()) {
      // TODO
      return {1};
    }
    const path &clang_for_ccelerate_exe = get_clang_for_ccelerate_executable();
    path local_code_file = command.input_infos[0].filename.value();
    local_code_file.replace_extension(fmt::format(
        "local.{}", clang::driver::types::getTypeTempSuffix(orig_input_type)));
    extract_local_code_with_clang(command.args, local_code_file, working_dir);
    vector<string> compile_args = command.args;
    //  rewrite_clang_cc1_args__change_source_file(
    //     command.args,
    //     local_code_file,
    //     clang::driver::types::getTypeName(orig_input_type));
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
  } else {
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

ParseClangArgsResult parse_clang_args(const span<const string> args,
                                      const path &working_dir,
                                      const string_view clang_name) {
  const path &clang_for_ccelerate_exe = get_clang_for_ccelerate_executable();
  const path self_path = get_current_executable_path();
  const path gcc_install_dir =
      self_path.parent_path() /
      "extern/gcc-13-install/lib/gcc/x86_64-linux-gnu/13";
  const ProcessResult parse_result = run_process_traced(
      ProcessArgs()
          .arg(clang_for_ccelerate_exe)
          .args({"parse-args",
                 "--cwd",
                 working_dir.c_str(),
                 "--binary",
                 string(clang_name)})
          .arg("--")
          .args(args)
          .arg(fmt::format("--gcc-install-dir={}", gcc_install_dir.string()))
          .working_dir(working_dir));
  if (parse_result.exit_code() != 0) {
    return parse_result;
  }
  if (!string_view(parse_result.stdout_data).starts_with(clang_io::magic)) {
    return parse_result;
  }
  clang_io::ParsedArgs parsed_args;
  try {
    string_view stdout_to_parse =
        string_view(parse_result.stdout_data).substr(clang_io::magic.size());
    msgpack::unpack(stdout_to_parse.data(), stdout_to_parse.size())
        .get()
        .convert(parsed_args);
  } catch (...) {
    return ProcessResult::from_finished(1);
  }
  return parsed_args;
}

void handle_request__clang(const Request &request) {
  ParseClangArgsResult parse_args_result = parse_clang_args(
      request.args, request.working_dir, to_string(request.program));
  if (ProcessResult *parse_result =
          std::get_if<ProcessResult>(&parse_args_result)) {
    send_response_final(request.client_id,
                        std::move(parse_result->stdout_data),
                        std::move(parse_result->stderr_data),
                        parse_result->exit_code().value_or(1));
    return;
  }
  const clang_io::ParsedArgs &parsed_args =
      std::get<clang_io::ParsedArgs>(parse_args_result);
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