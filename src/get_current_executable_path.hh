#pragma once

#include <filesystem>
#include <vector>
#include <whereami.h>

namespace ccelerate {

inline std::filesystem::path get_current_executable_path() {
  int length = wai_getExecutablePath(NULL, 0, NULL);
  if (length <= 0) {
    return {};
  }

  std::vector<char> buffer(length + 1);
  int dirname_length = 0;
  wai_getExecutablePath(buffer.data(), length, &dirname_length);
  buffer[length] = '\0';

  return std::filesystem::path(buffer.data());
}

} // namespace ccelerate