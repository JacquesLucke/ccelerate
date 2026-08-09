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
  auto include_defines =
      toml::find_or_default<vector<string>>(data, "include_defines");

  ConfigFile config_file;
  config_file.local_header_patterns = std::move(local_header_patterns);
  config_file.include_defines = std::move(include_defines);
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
  string is_include_define_expr;
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
    for (const string &pattern : config_file.include_defines) {
      append_pattern(is_include_define_expr, pattern);
    }
  }
  if (!is_local_header_expr.empty()) {
    config.is_local_header_ = make_unique<re2::RE2>(is_local_header_expr);
  }
  if (!is_include_define_expr.empty()) {
    config.is_include_define_ = make_unique<re2::RE2>(is_include_define_expr);
  }
  return config;
}

bool Config::is_local_header(const path &path) const {
  if (!is_local_header_ || !is_local_header_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(path.string(), *is_local_header_);
}

bool Config::is_include_define(string_view define) const {
  if (!is_include_define_ || !is_include_define_->ok()) {
    return false;
  }
  return re2::RE2::FullMatch(define, *is_include_define_);
}

} // namespace ccelerate