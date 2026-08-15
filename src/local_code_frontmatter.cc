// SPDX-License-Identifier: MIT

#include "local_code_frontmatter.hh"

#include <fstream>
#include <toml.hpp>

namespace ccelerate {

static string_view trim_whitespace(const string_view str) {
  const size_t start = str.find_first_not_of(" \t\n\r");
  if (start == string_view::npos) {
    return {};
  }
  const size_t end = str.find_last_not_of(" \t\n\r");
  return str.substr(start, end - start + 1);
}

string LocalCodeFrontmatter::to_toml_string() const {
  toml::array_format_info fmt;
  fmt.fmt = toml::array_format::multiline;

  vector<toml::value> direct_include_values;
  direct_include_values.reserve(direct_includes.size());
  for (const DirectInclude &include : direct_includes) {
    toml::value entry;
    entry["path"] = include.include_path.string();
    entry["is_system_header"] = include.is_system_header;
    if (include.is_pure_c) {
      entry["is_pure_c"] = true;
    }
    direct_include_values.push_back(std::move(entry));
  }

  toml::value table;
  table["local_code_path"] = local_code_path.string();
  table["direct_includes"] = toml::value(direct_include_values, fmt);
  table["include_defines"] = toml::value(include_defines, fmt);
  table["source_language"] = source_language;
  if (cc1_args) {
    table["cc1_args"] = toml::value(*cc1_args, fmt);
  }
  return string(trim_whitespace(toml::format(table))) + '\n';
}

bool LocalCodeFrontmatter::write_to_path(const path &path) const {
  std::ofstream file(path);
  if (!file.is_open()) {
    return false;
  }
  file << to_toml_string();
  return static_cast<bool>(file);
}

optional<LocalCodeFrontmatter>
LocalCodeFrontmatter::from_toml_string(string toml_str) {
  auto data_opt = toml::try_parse_str(std::move(toml_str));
  if (data_opt.is_err()) {
    return nullopt;
  }
  toml::value &data = data_opt.unwrap();

  LocalCodeFrontmatter frontmatter;
  frontmatter.local_code_path =
      path(toml::find_or_default<string>(data, "local_code_path"));
  frontmatter.source_language =
      toml::find_or_default<string>(data, "source_language");
  frontmatter.include_defines =
      toml::find_or_default<vector<string>>(data, "include_defines");

  for (const toml::value &entry :
       toml::find_or_default<toml::array>(data, "direct_includes")) {
    frontmatter.direct_includes.push_back(DirectInclude{
        .include_path = path(toml::find<string>(entry, "path")),
        .is_system_header =
            toml::find_or<bool>(entry, "is_system_header", false),
        .is_pure_c = toml::find_or<bool>(entry, "is_pure_c", false),
    });
  }

  if (data.contains("cc1_args")) {
    frontmatter.cc1_args = toml::find<vector<string>>(data, "cc1_args");
  }

  return frontmatter;
}

optional<LocalCodeFrontmatter>
LocalCodeFrontmatter::from_path(const path &path) {
  std::ifstream file(path);
  if (!file.is_open()) {
    return nullopt;
  }
  string toml_str((std::istreambuf_iterator<char>(file)),
                  std::istreambuf_iterator<char>());
  return LocalCodeFrontmatter::from_toml_string(std::move(toml_str));
}

} // namespace ccelerate
