// SPDX-License-Identifier: MIT

#pragma once

#include <whereami.h>

#include "filesystem.hh"
#include "vector.hh"

namespace ccelerate {

inline path get_current_executable_path() {
  int length = wai_getExecutablePath(NULL, 0, NULL);
  if (length <= 0) {
    return {};
  }

  vector<char> buffer(length + 1);
  int dirname_length = 0;
  wai_getExecutablePath(buffer.data(), length, &dirname_length);
  buffer[length] = '\0';

  return path(buffer.data());
}

} // namespace ccelerate