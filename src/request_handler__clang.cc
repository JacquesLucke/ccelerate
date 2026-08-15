// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <fstream>
#include <xxhash.h>

#include "array.hh"
#include "ccelerate_extensions.hh"
#include "clang_call.hh"
#include "clang_for_ccelerate_io.hh"
#include "config.hh"
#include "get_current_executable_path.hh"
#include "local_code_frontmatter.hh"
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

ProcessResult
extract_local_code_with_clang(span<const string> cc1_args,
                              const path &output_path,
                              const path &working_dir,
                              const string_view local_id,
                              const span<const path> config_paths,
                              const span<const PathMap> path_maps) {
  const path &clang_for_ccelerate_exe = get_clang_for_ccelerate_executable();
  ProcessArgs args;
  args.arg(clang_for_ccelerate_exe)
      .args({"local-code",
             "--local-code-path",
             output_path,
             "--local-id",
             string(local_id)});
  for (const PathMap &mapping : path_maps) {
    args.args(
        {"--path-map",
         fmt::format("{}={}", mapping.path.string(), mapping.replacement)});
  }
  for (const path &config_path : config_paths) {
    args.args({"--config", config_path.string()});
  }
  args.args({"--"}).args(cc1_args).working_dir(working_dir);
  return run_process_traced(args);
}

static string path_to_local_id(const path &path) {
  const string s = path.native();
  const XXH64_hash_t hash = XXH3_64bits(s.data(), s.size());
  return fmt::format("_{:016x}", hash);
}

static ExitCodeOrError
handle_command__clang_cc1(const ClientID &client_id,
                          const clang_io::Command &command,
                          const string &working_dir) {
  const optional<ClangCC1ActionType> action_type_opt =
      get_clang_cc1_action_type(command.args);
  if (action_type_opt == ClangCC1ActionType::EmitObj) {
    const path obj_file = command.output_files[0];
    if (ConfigDiscovery::get().get_latest().is_eager_obj(obj_file)) {
      const path &clang_for_ccelerate_exe =
          get_clang_for_ccelerate_executable();
      return run_process_stream_output_traced(
          ProcessArgs()
              .arg(clang_for_ccelerate_exe)
              .args({"compile_obj", "--"})
              .args(command.args)
              .working_dir(working_dir),
          [&](string stdout_data, string stderr_data) {
            send_response_incomplete(
                client_id, std::move(stdout_data), std::move(stderr_data));
          });
    }
    path local_code_file = obj_file;
    local_code_file.replace_extension(extensions::object);

    const vector<path> config_paths = ConfigDiscovery::get().config_paths();
    const string local_id = path_to_local_id(obj_file);
    const ProcessResult extract_result = extract_local_code_with_clang(
        command.args, local_code_file, working_dir, local_id, config_paths);
    send_response_incomplete(
        client_id, extract_result.stdout_data, extract_result.stderr_data);
    if (extract_result.exit_code() != 0) {
      return extract_result.exit_code_or_error();
    }
    // Output a dummy .o file so because the caller might expect that. Actual
    // compilation will happen later.
    path obj_path = obj_file;
    if (obj_path.is_relative()) {
      obj_path = path(working_dir) / obj_path;
    }
    {
      std::ofstream placeholder(obj_path, std::ios::binary | std::ios::trunc);
      if (!placeholder) {
        return 1;
      }
    }
    return 0;
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

static ExitCodeOrError rewrite_link_args(const ClientID &client_id,
                                         const string &working_dir,
                                         const span<const string> old_args,
                                         vector<string> &new_args) {
  size_t old_arg_i = 0;
  while (old_arg_i < old_args.size()) {
    const string &arg = old_args[old_arg_i];

    // Handle parameters with arguments.
    if (arg == "-o" || arg == "-dynamic-linker" || arg == "-m" ||
        arg == "-rpath") {
      new_args.push_back(arg);
      old_arg_i++;
      if (old_arg_i < old_args.size()) {
        new_args.push_back(old_args[old_arg_i]);
        old_arg_i++;
      }
      continue;
    }

    // Handle some options.
    if (arg.starts_with("-")) {
      new_args.push_back(arg);
      old_arg_i++;
      continue;
    }

    if (arg.ends_with(".o") &&
        local_code_path_of_obj_if_exists(working_dir, arg)) {
      vector<path> local_obj_paths;
      while (old_arg_i < old_args.size()) {
        const string &obj_arg = old_args[old_arg_i];
        if (!obj_arg.ends_with(".o")) {
          break;
        }
        const optional<path> local_obj_path =
            local_code_path_of_obj_if_exists(working_dir, obj_arg);
        if (!local_obj_path) {
          break;
        }
        local_obj_paths.push_back(*local_obj_path);
        old_arg_i++;
      }
      assert(!local_obj_paths.empty());
      BuildLocalObjectsResult build_result =
          build_local_objects(client_id, local_obj_paths);
      if (!build_result.success) {
        return 1;
      }
      for (const path &p : build_result.paths) {
        new_args.push_back(p);
      }
      continue;
    }

    // Can rewrite arguments in the future.
    new_args.push_back(arg);
    old_arg_i++;
  }
  return 0;
}

static ExitCodeOrError handle_command__link(const ClientID &client_id,
                                            const clang_io::Command &command,
                                            const string &working_dir) {
  vector<string> link_args;
  ExitCodeOrError rewrite_result =
      rewrite_link_args(client_id, working_dir, command.args, link_args);
  if (rewrite_result.exit_code() != 0) {
    return rewrite_result;
  }
  return run_process_stream_output_traced(
      ProcessArgs()
          .arg(command.executable)
          .args(link_args)
          .working_dir(working_dir),
      [&](string stdout_data, string stderr_data) {
        send_response_incomplete(
            client_id, std::move(stdout_data), std::move(stderr_data));
      });
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
    } else if (command.kind ==
               clang::driver::Action::ActionClass::LinkJobClass) {
      exit_or_error =
          handle_command__link(request.client_id, command, request.working_dir);
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