// SPDX-License-Identifier: MIT

#pragma once

#include <re2/re2.h>

#include "filesystem.hh"
#include "memory.hh"
#include "optional.hh"
#include "span.hh"
#include "string.hh"
#include "vector.hh"

namespace ccelerate {

class ConfigFile {
public:
  vector<string> local_header_patterns;
  vector<string> include_defines;

  static optional<ConfigFile> from_path(const path &path);
  static optional<ConfigFile> from_toml_string(string toml_str);
};

class Config {
private:
  vector<ConfigFile> config_files_;
  unique_ptr<re2::RE2> is_local_header_;
  unique_ptr<re2::RE2> is_include_define_;

public:
  static Config from_paths(span<const path> paths);
  static Config from_config_files(span<const ConfigFile> config_files);

  bool is_local_header(const path &path) const;
  bool is_include_define(string_view define) const;
};

} // namespace ccelerate