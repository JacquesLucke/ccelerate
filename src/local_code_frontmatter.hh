// SPDX-License-Identifier: MIT

#pragma once

#include "filesystem.hh"
#include "optional.hh"
#include "string.hh"
#include "vector.hh"

namespace ccelerate {

struct DirectInclude {
  path include_path;
  bool is_system_header = false;
  // Match pure_c_header_patterns: wrap with extern "C" when replaying into C++.
  bool is_pure_c = false;
};

struct LocalCodeFrontmatter {
  path local_code_path;
  string source_language;
  vector<string> include_defines;
  vector<DirectInclude> direct_includes;
  optional<vector<string>> cc1_args;

  string to_toml_string() const;
  bool write_to_path(const path &path) const;

  static optional<LocalCodeFrontmatter> from_toml_string(string toml_str);
  static optional<LocalCodeFrontmatter> from_path(const path &path);
};

} // namespace ccelerate
