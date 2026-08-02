// SPDX-License-Identifier: MIT

#include "get_current_executable_path.hh"
#include "request_handler.hh"
#include "run_process.hh"

namespace ccelerate {

void handle_request__cmake(const Request &request) {
  const path binary_path = get_current_executable_path();
  const path dir = binary_path.parent_path();

  ProcessArgs args;
  args.arg("cmake")
      .args(request.args)
      .working_dir(request.working_dir)
      .env("CC", dir / "ccelerate_clang")
      .env("CXX", dir / "ccelerate_clang++");

  const bool has_build_arg = std::ranges::any_of(
      request.args, [](const string &arg) { return arg == "--build"; });
  if (!has_build_arg) {
    args.arg(fmt::format("-DCMAKE_AR={}", (dir / "ccelerate_ar").string()));
  }
  pass_through_external_call(request.client_id, args, true);
}

} // namespace ccelerate