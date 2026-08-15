// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <fstream>
#include <functional>
#include <toml.hpp>
#include <unordered_map>

#include "ccelerate_extensions.hh"
#include "clang_call.hh"
#include "local_code_frontmatter.hh"
#include "request_handler.hh"
#include "run_process_traced.hh"

namespace ccelerate {

struct CompatibilityKey {
  vector<string> cc1_args;

  bool operator==(const CompatibilityKey &) const = default;
};

} // namespace ccelerate

template <> struct std::hash<ccelerate::CompatibilityKey> {
  size_t operator()(const ccelerate::CompatibilityKey &key) const noexcept {
    size_t seed = key.cc1_args.size();
    for (const std::string &arg : key.cc1_args) {
      seed ^= std::hash<std::string>{}(arg) + 0x9e3779b9 + (seed << 6) +
              (seed >> 2);
    }
    return seed;
  }
};

namespace ccelerate {

static CompatibilityKey
cc1_args_to_compatibility_key(const span<const string> old_args) {
  CompatibilityKey key;
  size_t old_arg_i = 0;
  while (old_arg_i < old_args.size()) {
    const string &arg = old_args[old_arg_i];

    // Skip some file arguments.
    if (arg == "-o" || arg == "-main-file-name" || arg == "-dependency-file" ||
        arg == "-MT") {
      old_arg_i += 2;
      continue;
    }

    // Input file is the last argument, skip it.
    if (old_arg_i + 1 == old_args.size()) {
      old_arg_i++;
      continue;
    }

    // Other options are kept.
    key.cc1_args.push_back(arg);
    old_arg_i++;
  }
  return key;
}

void handle_request__ar(const Request &request) {
  if (request.args.size() <= 1) {
    handle_request__eager(request);
    return;
  }
  // https://sourceware.org/binutils/docs/binutils/ar-cmdline.html
  const string_view operation_arg = request.args[0];
  if (operation_arg == "qc") {
    const path archive_file =
        std::filesystem::absolute(request.working_dir / request.args[1])
            .lexically_normal();

    vector<path> local_obj_paths;
    vector<path> other_paths;
    for (size_t i = 2; i < request.args.size(); i++) {
      const path src_path =
          std::filesystem::absolute(request.working_dir / request.args[i])
              .lexically_normal();
      if (src_path.extension() == ".o") {
        path local_obj_path = src_path;
        local_obj_path.replace_extension(extensions::object);
        if (std::filesystem::is_regular_file(local_obj_path)) {
          local_obj_paths.push_back(local_obj_path);
        }
        continue;
      }
      other_paths.push_back(src_path);
    }

    std::unordered_map<CompatibilityKey, vector<path>> compatibility_map;
    for (const path &local_obj_path : local_obj_paths) {
      std::optional<LocalCodeFrontmatter> frontmatter_opt =
          LocalCodeFrontmatter::from_path(local_obj_path);
      if (!frontmatter_opt) {
        // TODO: Error handling.
        continue;
      }
      CompatibilityKey key =
          cc1_args_to_compatibility_key(*frontmatter_opt->cc1_args);
      compatibility_map[std::move(key)].push_back(local_obj_path);
    }

    vector<path> local_compiled_obj_files;
    for (const auto &[key, paths] : compatibility_map) {
      const path &clang_for_ccelerate_exe =
          get_clang_for_ccelerate_executable();

      ProcessArgs args;
      args.arg(clang_for_ccelerate_exe).arg("compile-local-code");
      for (const path &p : paths) {
        args.arg("--input").arg(p);
      }

      std::string out_path = std::tmpnam(nullptr);
      args.arg("--output").arg(out_path);
      args.arg("--");
      args.args(key.cc1_args);

      ExitCodeOrError result = run_process_stream_output_traced(
          args, [&](string stdout_data, string stderr_data) {
            send_response_incomplete(request.client_id,
                                     std::move(stdout_data),
                                     std::move(stderr_data));
          });
      if (result.exit_code() != 0) {
        send_response_final(request.client_id,
                            "",
                            result.error_message().value_or(""),
                            result.exit_code().value_or(1));
        return;
      }
      local_compiled_obj_files.push_back(out_path);
    }

    vector<string> final_ar_args;
    final_ar_args.push_back("qc");
    final_ar_args.push_back(archive_file);
    for (const path &p : local_compiled_obj_files) {
      final_ar_args.push_back(p);
    }
    for (const path &p : other_paths) {
      final_ar_args.push_back(p);
    }

    const ExitCodeOrError exit_code_or_error = run_process_stream_output_traced(
        ProcessArgs()
            .arg(to_string(request.program))
            .args(final_ar_args)
            .working_dir(request.working_dir),
        [&](string stdout_data, string stderr_data) {
          send_response_incomplete(request.client_id,
                                   std::move(stdout_data),
                                   std::move(stderr_data));
        });
    send_response_final(
        request.client_id, "", "", exit_code_or_error.exit_code().value_or(1));
  } else {
    handle_request__eager(request);
  }
}

} // namespace ccelerate