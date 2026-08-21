// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <functional>
#include <tbb/parallel_for.h>
#include <tbb/parallel_invoke.h>
#include <toml.hpp>
#include <unordered_map>

#include "ccelerate_extensions.hh"
#include "clang_call.hh"
#include "config.hh"
#include "create_temporary_path.hh"
#include "local_code_info.hh"
#include "request_handler.hh"
#include "run_process_traced.hh"
#include "string.hh"

namespace ccelerate {

struct CompatibilityKey {
  vector<string> cc1_args;
  string language;
  vector<string> include_defines;
  ConfigUnitIsolationKey unit_isolation_key;

  bool operator==(const CompatibilityKey &) const = default;
};

} // namespace ccelerate

static size_t hash_combine(size_t seed, const std::string_view str) {
  seed ^= std::hash<std::string_view>{}(str) + 0x9e3779b9 + (seed << 6) +
          (seed >> 2);
  return seed;
}

template <> struct std::hash<ccelerate::CompatibilityKey> {
  size_t operator()(const ccelerate::CompatibilityKey &key) const noexcept {
    size_t seed = key.cc1_args.size();
    for (const std::string &arg : key.cc1_args) {
      seed = hash_combine(seed, arg);
    }
    seed = hash_combine(seed, key.language);
    for (const std::string &define : key.include_defines) {
      seed = hash_combine(seed, define);
    }
    seed ^= 23436243 * std::hash<ccelerate::ConfigUnitIsolationKey>{}(
                           key.unit_isolation_key);
    return seed;
  }
};

namespace ccelerate {

static CompatibilityKey
cc1_args_to_compatibility_key(const LocalCodeInfo &info) {
  CompatibilityKey key;
  size_t old_arg_i = 0;
  while (old_arg_i < info.cc1_args.size()) {
    const string &arg = info.cc1_args[old_arg_i];

    // Skip some file arguments.
    if (arg == "-o" || arg == "-main-file-name" || arg == "-dependency-file" ||
        arg == "-MT") {
      old_arg_i += 2;
      continue;
    }

    // Input file is the last argument, skip it.
    if (old_arg_i + 1 == info.cc1_args.size()) {
      old_arg_i++;
      continue;
    }

    // Other options are kept.
    key.cc1_args.push_back(arg);
    old_arg_i++;
  }
  key.language = info.source_language;
  key.include_defines = info.include_defines;
  key.unit_isolation_key =
      ConfigDiscovery::get().get_latest().get_unit_isolation_key(
          info.object_path);
  return key;
}

optional<path> local_code_path_of_obj_if_exists(const path cwd,
                                                const path &obj_file) {
  if (obj_file.extension() != ".o") {
    return nullopt;
  }
  if (ConfigDiscovery::get().get_latest().is_eager_obj(obj_file)) {
    return nullopt;
  }
  path local_obj_path = (cwd / obj_file).lexically_normal();
  local_obj_path.replace_extension(extensions::object);
  if (std::filesystem::is_regular_file(local_obj_path)) {
    return local_obj_path;
  }
  return nullopt;
}

static BuildLocalObjectsResult
build_compatible_local_objects(const ClientID &client_id,
                               const CompatibilityKey &compatibility_key,
                               const span<const path> local_obj_files);

static BuildLocalObjectsResult
build_compatible_local_objects__split(const ClientID &client_id,
                                      const CompatibilityKey &compatibility_key,
                                      const span<const path> local_obj_files) {
  BuildLocalObjectsResult sub_result_1;
  BuildLocalObjectsResult sub_result_2;
  const int split_index = local_obj_files.size() / 2;
  const span<const path> files_1 = local_obj_files.first(split_index);
  const span<const path> files_2 = local_obj_files.subspan(split_index);
  tbb::parallel_invoke(
      [&]() {
        sub_result_1 = build_compatible_local_objects(
            client_id, compatibility_key, files_1);
      },
      [&]() {
        sub_result_2 = build_compatible_local_objects(
            client_id, compatibility_key, files_2);
      });
  BuildLocalObjectsResult result;
  if (sub_result_1.success && sub_result_2.success) {
    result.paths.insert(result.paths.end(),
                        sub_result_1.paths.begin(),
                        sub_result_1.paths.end());
    result.paths.insert(result.paths.end(),
                        sub_result_2.paths.begin(),
                        sub_result_2.paths.end());
    result.success = true;
    return result;
  }
  return result;
}

static BuildLocalObjectsResult
build_compatible_local_objects(const ClientID &client_id,
                               const CompatibilityKey &compatibility_key,
                               const span<const path> local_obj_files) {
  const int size_threshold = 10;
  BuildLocalObjectsResult result;
  if (local_obj_files.size() > size_threshold) {
    return build_compatible_local_objects__split(
        client_id, compatibility_key, local_obj_files);
  }

  const path &clang_for_ccelerate_exe = get_clang_for_ccelerate_executable();
  const bool is_single = local_obj_files.size() == 1;

  if (is_single) {
    // Compile the object file with its original command instead of using the
    // local code to produce more precise diagnostics.
    const path local_obj_path = local_obj_files[0];
    const optional<LocalCodeInfo> info =
        LocalCodeInfo::from_path(local_obj_path);
    if (!info) {
      return result;
    }
    ProcessArgs args;
    args.arg(clang_for_ccelerate_exe).arg("compile-object").arg("--");
    args.args(info->cc1_args);
    args.working_dir(info->cwd);
    const ExitCodeOrError compile_result = run_process_stream_output_traced(
        args, [&](string stdout_data, string stderr_data) {
          send_response_incomplete(
              client_id, std::move(stdout_data), std::move(stderr_data));
        });
    if (compile_result.exit_code() == 0) {
      result.paths.push_back(info->object_path);
      result.success = true;
      return result;
    }
    return result;
  } else {
    ProcessArgs args;
    args.arg(clang_for_ccelerate_exe).arg("compile-local-code");
    for (const path &p : local_obj_files) {
      args.arg("--input").arg(p);
    }

    const path out_path = create_temporary_path();
    if (out_path.empty()) {
      return result;
    }
    args.arg("--output").arg(out_path.native());
    args.arg("--");
    args.args(compatibility_key.cc1_args);

    ProcessResult compile_result = run_process_traced(args);
    if (compile_result.exit_code() == 0) {
      result.paths.push_back(out_path);
      result.success = true;
      return result;
    }
    if (is_single) {
      send_response_incomplete(client_id,
                               std::move(compile_result.stdout_data),
                               std::move(compile_result.stderr_data));
      return result;
    }
    return build_compatible_local_objects__split(
        client_id, compatibility_key, local_obj_files);
  }
}

BuildLocalObjectsResult
build_local_objects(const ClientID &client_id,
                    const span<const path> local_obj_files) {
  std::unordered_map<CompatibilityKey, vector<path>> compatibility_map;
  for (const path &local_obj_path : local_obj_files) {
    std::optional<LocalCodeInfo> frontmatter_opt =
        LocalCodeInfo::from_path(local_obj_path);
    if (!frontmatter_opt) {
      return {};
    }
    CompatibilityKey key = cc1_args_to_compatibility_key(*frontmatter_opt);
    compatibility_map[std::move(key)].push_back(local_obj_path);
  }
  vector<pair<const CompatibilityKey *, const vector<path> *>> groups;
  groups.reserve(compatibility_map.size());
  for (const auto &[key, paths] : compatibility_map) {
    groups.emplace_back(&key, &paths);
  }

  vector<BuildLocalObjectsResult> sub_results(groups.size());
  tbb::parallel_for(tbb::blocked_range<size_t>(0, groups.size(), 1),
                    [&](const tbb::blocked_range<size_t> &range) {
                      for (size_t i = range.begin(); i < range.end(); i++) {
                        const auto &[key, paths] = groups[i];
                        sub_results[i] = build_compatible_local_objects(
                            client_id, *key, *paths);
                      }
                    });

  BuildLocalObjectsResult result;
  for (const BuildLocalObjectsResult &sub_result : sub_results) {
    if (!sub_result.success) {
      return result;
    }
    result.paths.insert(
        result.paths.end(), sub_result.paths.begin(), sub_result.paths.end());
  }
  result.success = true;
  return result;
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
      if (const optional<path> local_obj_path =
              local_code_path_of_obj_if_exists(request.working_dir,
                                               request.args[i])) {
        local_obj_paths.push_back(*local_obj_path);
      } else {
        other_paths.push_back(request.args[i]);
      }
    }

    BuildLocalObjectsResult build_result =
        build_local_objects(request.client_id, local_obj_paths);
    if (!build_result.success) {
      send_response_final(request.client_id, "", "", 1);
      return;
    }

    vector<string> final_ar_args;
    final_ar_args.push_back("qc");
    final_ar_args.push_back(archive_file);
    for (const path &p : build_result.paths) {
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