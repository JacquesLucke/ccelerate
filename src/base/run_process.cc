// SPDX-License-Identifier: MIT

#include "run_process.hh"

#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>

namespace ccelerate {

ProcessResult
ProcessResult::from_error(error_code ec, string stdout, string stderr) {
  return ProcessResult{ec, std::move(stdout), std::move(stderr)};
}

ProcessResult
ProcessResult::from_finished(int exit_code, string stdout, string stderr) {
  return ProcessResult{exit_code, std::move(stdout), std::move(stderr)};
}

ProcessResult run_process(const ProcessArgs &args) {
  reproc::options options;
  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  options.env.behavior = args.data.env_mode == ProcessArgs::EnvMode::Replace
                             ? reproc::env::type::extend
                             : reproc::env::type::empty;
  options.env.extra = args.data.env_vars;
  reproc::process proc;
  {
    const error_code ec = proc.start(args.data.args, options);
    if (ec) {
      return ProcessResult::from_error(ec);
    }
  }
  std::string stdout;
  std::string stderr;
  {
    const error_code ec = reproc::drain(
        proc, reproc::sink::string(stdout), reproc::sink::string(stderr));
    if (ec) {
      return ProcessResult::from_error(
          ec, std::move(stdout), std::move(stderr));
    }
  }
  const auto &&[exit_code, ec] = proc.wait(reproc::infinite);
  if (ec) {
    return ProcessResult::from_error(ec, std::move(stdout), std::move(stderr));
  }
  return ProcessResult::from_finished(
      exit_code, std::move(stdout), std::move(stderr));
}

} // namespace ccelerate