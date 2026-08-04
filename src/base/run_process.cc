// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <fmt/ranges.h>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>
#include <tracy/Tracy.hpp>

#include "run_process.hh"

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
  ZoneScoped;
  const string zone_name = path(args.data.args[0]).filename();
  const string zone_text = fmt::format(
      "{}\ncwd: {}",
      fmt::join(args.data.args, " "),
      args.data.working_dir ? args.data.working_dir->string() : "(default)");
  ZoneName(zone_name.data(), zone_name.size());
  ZoneText(zone_text.data(), zone_text.size());
  ZoneColor(tracy::Color::Gray50);

  reproc::options options;
  if (args.data.working_dir) {
    options.working_directory = args.data.working_dir->c_str();
  }
  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  options.env.behavior = args.data.env_mode == ProcessArgs::EnvMode::Replace
                             ? reproc::env::type::empty
                             : reproc::env::type::extend;
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