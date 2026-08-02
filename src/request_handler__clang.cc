// SPDX-License-Identifier: MIT

#include "request_handler.hh"

namespace ccelerate {

void handle_request__clang(const Request &request) {
  pass_through_external_call(request.client_id,
                             ProcessArgs()
                                 .arg(to_string(request.program))
                                 .args(request.args)
                                 .arg("-###")
                                 .working_dir(request.working_dir),
                             false);
  handle_request__eager(request);
}

} // namespace ccelerate