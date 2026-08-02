// SPDX-License-Identifier: MIT

#pragma once

#include "base/string.hh"
#include <cstdlib>

namespace ccelerate {

inline string get_default_ccelerate_endpoint() {
  const char *endpoint = std::getenv("CCELERATE_ENDPOINT");
  if (endpoint) {
    return endpoint;
  }
  return "ipc:///tmp/ccelerate.ipc";
}

static string inproc_endpoint = "inproc://server";

} // namespace ccelerate