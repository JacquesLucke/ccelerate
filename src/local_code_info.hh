// SPDX-License-Identifier: MIT

#pragma once

#include "filesystem.hh"
#include "optional.hh"
#include "string.hh"
#include "vector.hh"

namespace ccelerate {

struct LocalCodeInfo {
  struct DirectInclude {
    path include_path;
    bool is_system_header = false;
    // Match pure_c_header_patterns: wrap with extern "C" when replaying into
    // C++.
    bool is_pure_c = false;
  };

  struct UsingNamespace {
    string parent;
    string used;
  };

  path local_code_path;
  path object_path;
  string source_language;
  vector<string> include_defines;
  vector<DirectInclude> direct_includes;
  path cwd;
  vector<string> cc1_args;
  vector<UsingNamespace> using_namespaces;

  string to_toml_string() const;
  bool write_to_path(const path &path) const;

  static optional<LocalCodeInfo> from_toml_string(string toml_str);
  static optional<LocalCodeInfo> from_path(const path &path);
};

} // namespace ccelerate
