// SPDX-License-Identifier: MIT

#include "request_handler.hh"

namespace ccelerate {

void handle_request__clang(const Request &request) {
  handle_request__eager(request);
}

} // namespace ccelerate