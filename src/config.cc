// SPDX-License-Identifier: MIT

#include <cstdlib>
#include <toml.hpp>

#include "config.hh"

namespace ccelerate {

optional<ConfigFile> ConfigFile::from_path(const path &path) {
  std::ifstream file(path);
  if (!file.is_open()) {
    return nullopt;
  }
  string toml_str((std::istreambuf_iterator<char>(file)),
                  std::istreambuf_iterator<char>());
  return ConfigFile::from_toml_string(std::move(toml_str));
}

optional<ConfigFile> ConfigFile::from_toml_string(string toml_str) {
  auto data_opt = toml::try_parse_str(std::move(toml_str));
  if (data_opt.is_err()) {
    return nullopt;
  }
  toml::value &data = data_opt.unwrap();
  auto local_header_patterns =
      toml::find_or_default<vector<string>>(data, "local_header_patterns");
  auto local_type_patterns =
      toml::find_or_default<vector<string>>(data, "local_type_patterns");
  auto pure_c_header_patterns =
      toml::find_or_default<vector<string>>(data, "pure_c_header_patterns");
  auto include_defines =
      toml::find_or_default<vector<string>>(data, "include_defines");
  auto eager_obj_patterns =
      toml::find_or_default<vector<string>>(data, "eager_obj_patterns");
  auto first_include_patterns =
      toml::find_or_default<vector<string>>(data, "first_include_patterns");

  ConfigFile config_file;
  config_file.local_header_patterns = std::move(local_header_patterns);
  config_file.local_type_patterns = std::move(local_type_patterns);
  config_file.pure_c_header_patterns = std::move(pure_c_header_patterns);
  config_file.include_defines = std::move(include_defines);
  config_file.eager_obj_patterns = std::move(eager_obj_patterns);
  config_file.first_include_patterns = std::move(first_include_patterns);

  for (const toml::value &entry :
       toml::find_or_default<toml::array>(data, "unit_isolation_patterns")) {
    ConfigFile::UnitIsolationPattern pattern;
    pattern.name = toml::find_or_default<string>(entry, "name");
    pattern.patterns = toml::find_or_default<vector<string>>(entry, "patterns");
    config_file.unit_isolation_patterns.push_back(std::move(pattern));
  }

  return config_file;
}

Config Config::from_paths(const span<const path> paths) {
  vector<ConfigFile> config_files;
  config_files.reserve(paths.size());
  for (const path &config_path : paths) {
    if (optional<ConfigFile> config_file_opt =
            ConfigFile::from_path(config_path)) {
      config_files.push_back(std::move(*config_file_opt));
    }
  }
  return Config::from_config_files(config_files);
}

static string glob_to_regex(string_view glob) {
  string regex;
  size_t i = 0;
  while (i < glob.size()) {
    const string_view remaining = glob.substr(i);
    if (remaining.starts_with("**")) {
      regex.append(".*");
      i += 2;
      continue;
    }
    const char c = remaining[0];
    switch (c) {
      case '*': {
        regex.append("[^/]*");
        break;
      }
      case '?': {
        regex.append("[^/]");
        break;
      }
      case '.':
      case '+':
      case '^':
      case '$':
      case '(':
      case ')':
      case '[':
      case ']':
      case '{':
      case '}':
      case '|': {
        regex += '\\';
        regex += c;
        break;
      }
      default: {
        regex += c;
        break;
      }
    }
    i++;
  }
  return regex;
}

Config Config::from_config_files(const span<const ConfigFile> config_files) {
  Config config;
  string is_local_header_expr;
  string is_local_type_expr;
  string is_pure_c_header_expr;
  string is_include_define_expr;
  string is_eager_obj_expr;
  auto append_pattern = [](string &expr, string_view pattern) {
    if (!expr.empty()) {
      expr += '|';
    }
    expr += glob_to_regex(pattern);
  };
  for (const ConfigFile &config_file : config_files) {
    for (const string &pattern : config_file.local_header_patterns) {
      append_pattern(is_local_header_expr, pattern);
    }
    for (const string &pattern : config_file.local_type_patterns) {
      append_pattern(is_local_type_expr, pattern);
    }
    for (const string &pattern : config_file.pure_c_header_patterns) {
      append_pattern(is_pure_c_header_expr, pattern);
    }
    for (const string &pattern : config_file.include_defines) {
      append_pattern(is_include_define_expr, pattern);
    }
    for (const string &pattern : config_file.eager_obj_patterns) {
      append_pattern(is_eager_obj_expr, pattern);
    }
    for (const ConfigFile::UnitIsolationPattern &unit_isolation_pattern :
         config_file.unit_isolation_patterns) {
      string expr;
      for (const string &pattern : unit_isolation_pattern.patterns) {
        append_pattern(expr, pattern);
      }
      if (expr.empty()) {
        continue;
      }
      config.unit_isolation_patterns_.push_back(UnitIsolationPattern{
          .name = unit_isolation_pattern.name,
          .pattern = make_unique<re2::RE2>(expr),
      });
    }
    for (const string &pattern : config_file.first_include_patterns) {
      config.first_include_patterns_.push_back(
          make_unique<re2::RE2>(glob_to_regex(pattern)));
    }
  }
  if (!is_local_header_expr.empty()) {
    config.is_local_header_ = make_unique<re2::RE2>(is_local_header_expr);
  }
  if (!is_local_type_expr.empty()) {
    config.is_local_type_ = make_unique<re2::RE2>(is_local_type_expr);
  }
  if (!is_pure_c_header_expr.empty()) {
    config.is_pure_c_header_ = make_unique<re2::RE2>(is_pure_c_header_expr);
  }
  if (!is_include_define_expr.empty()) {
    config.is_include_define_ = make_unique<re2::RE2>(is_include_define_expr);
  }
  if (!is_eager_obj_expr.empty()) {
    config.is_eager_obj_ = make_unique<re2::RE2>(is_eager_obj_expr);
  }
  return config;
}

bool Config::is_local_header(const path &path) const {
  if (!is_local_header_ || !is_local_header_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(path.string(), *is_local_header_);
}

bool Config::is_local_type(string_view type_name) const {
  if (!is_local_type_ || !is_local_type_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(type_name, *is_local_type_);
}

bool Config::is_pure_c_header(const path &path) const {
  if (!is_pure_c_header_ || !is_pure_c_header_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(path.string(), *is_pure_c_header_);
}

bool Config::is_include_define(string_view define) const {
  if (!is_include_define_ || !is_include_define_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(define, *is_include_define_);
}

bool Config::is_eager_obj(const path &path) const {
  if (!is_eager_obj_ || !is_eager_obj_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(path.string(), *is_eager_obj_);
}

ConfigUnitIsolationKey Config::get_unit_isolation_key(const path &path) const {
  ConfigUnitIsolationKey key;
  const string path_str = path.string();
  for (const UnitIsolationPattern &unit_isolation_pattern :
       unit_isolation_patterns_) {
    if (!unit_isolation_pattern.pattern ||
        !unit_isolation_pattern.pattern->ok()) {
      continue;
    }
    if (re2::RE2::FullMatch(path_str, *unit_isolation_pattern.pattern)) {
      key.ordered_matched_patterns.push_back(unit_isolation_pattern.name);
    }
  }
  return key;
}

optional<int> Config::include_order_key(const path &path) const {
  const string path_str = path.string();
  for (size_t i = 0; i < first_include_patterns_.size(); i++) {
    const unique_ptr<re2::RE2> &pattern = first_include_patterns_[i];
    if (pattern && pattern->ok() && re2::RE2::FullMatch(path_str, *pattern)) {
      return static_cast<int>(i);
    }
  }
  return nullopt;
}

ConfigDiscovery::ConfigDiscovery() {
  all_configs_.push_back(std::make_unique<Config>());
  latest_config_ = all_configs_[0].get();
}

void ConfigDiscovery::add_search_path(const path &search_path) {
  if (search_path.empty()) {
    return;
  }
  path current = std::filesystem::absolute(search_path).lexically_normal();
  if (!std::filesystem::is_directory(current)) {
    current = current.parent_path();
  }

  std::lock_guard<std::mutex> lock(mutex_);
  bool found_new_config = false;
  while (true) {
    // Already scanned this directory (and therefore all parents).
    if (!visited_paths_.insert(current).second) {
      break;
    }
    const path config_path = current / "ccelerate.toml";
    if (std::filesystem::is_regular_file(config_path)) {
      config_paths_.push_back(config_path);
      found_new_config = true;
    }
    const path parent = current.parent_path();
    if (parent == current) {
      break;
    }
    current = parent;
  }

  if (found_new_config) {
    all_configs_.push_back(
        std::make_unique<Config>(Config::from_paths(config_paths_)));
    latest_config_ = all_configs_.back().get();
  }
}

ConfigDiscovery &ConfigDiscovery::get() {
  static ConfigDiscovery discovery;
  return discovery;
}

} // namespace ccelerate
