// SPDX-License-Identifier: MIT

#pragma once

#include <atomic>
#include <cstdint>
#include <cstdlib>
#include <system_error>

#include "filesystem.hh"
#include "string.hh"

namespace ccelerate {

// Returns a unique path under a process-wide temporary directory. The
// directory is created once; later calls only increment an atomic counter.
// The file itself is not created. Returns an empty path on failure.
inline path create_temporary_path() {
  static const path dir = []() -> path {
    std::error_code ec;
    const path tmp_dir = std::filesystem::temp_directory_path(ec);
    if (ec) {
      return {};
    }
    string tmpl = (tmp_dir / "ccelerate-XXXXXX").string();
    if (::mkdtemp(tmpl.data()) == nullptr) {
      return {};
    }
    return path(std::move(tmpl));
  }();
  static std::atomic<uint64_t> next_id{0};
  if (dir.empty()) {
    return {};
  }
  return dir / std::to_string(next_id.fetch_add(1, std::memory_order_relaxed));
}

} // namespace ccelerate
