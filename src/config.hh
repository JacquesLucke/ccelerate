// SPDX-License-Identifier: MIT

#pragma once

#include <mutex>
#include <re2/re2.h>
#include <unordered_set>

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
  vector<string> local_type_patterns;
  vector<string> pure_c_header_patterns;
  vector<string> include_defines;
  vector<string> eager_obj_patterns;

  static optional<ConfigFile> from_path(const path &path);
  static optional<ConfigFile> from_toml_string(string toml_str);
};

class Config {
private:
  vector<ConfigFile> config_files_;
  unique_ptr<re2::RE2> is_local_header_;
  unique_ptr<re2::RE2> is_local_type_;
  unique_ptr<re2::RE2> is_pure_c_header_;
  unique_ptr<re2::RE2> is_include_define_;
  unique_ptr<re2::RE2> is_eager_obj_;

public:
  static Config from_paths(span<const path> paths);
  static Config from_config_files(span<const ConfigFile> config_files);

  bool is_local_header(const path &path) const;
  bool is_local_type(string_view type_name) const;
  bool is_pure_c_header(const path &path) const;
  bool is_include_define(string_view define) const;
  bool is_eager_obj(const path &path) const;
};

class ConfigDiscovery {
private:
  std::mutex mutex_;
  vector<path> config_paths_;
  vector<unique_ptr<Config>> all_configs_;
  std::atomic<const Config *> latest_config_;
  std::unordered_set<path> visited_paths_;

public:
  ConfigDiscovery();

  void add_search_path(const path &search_path);

  const Config &get_latest() const {
    assert(latest_config_);
    return *latest_config_;
  }

  vector<path> config_paths() const { return config_paths_; }

  static ConfigDiscovery &get();
};

} // namespace ccelerate