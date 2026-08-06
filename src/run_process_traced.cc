// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <fmt/ranges.h>
#include <limits>
#include <tracy/Tracy.hpp>
#include <utility>

#include "run_process_traced.hh"

namespace ccelerate {

static void annotate_process_zone(tracy::ScopedZone &zone,
                                  const ProcessArgs &args) {
  const string zone_name = path(args.data.args[0]).filename();
  const string zone_text = fmt::format(
      "{}\ncwd: {}",
      fmt::join(args.data.args, " "),
      args.data.working_dir ? args.data.working_dir->string() : "(default)");
  zone.Name(zone_name.data(), zone_name.size());
  zone.Text(zone_text.data(),
            std::min<size_t>(zone_text.size(),
                             std::numeric_limits<uint16_t>::max() - 1));
  zone.Color(tracy::Color::Gray50);
}

template <typename Fn>
static decltype(auto) with_process_zone(const ProcessArgs &args, Fn &&fn) {
  ZoneNamed(zone, true);
  annotate_process_zone(zone, args);
  return std::forward<Fn>(fn)();
}

ProcessResult run_process_traced(const ProcessArgs &args) {
  return with_process_zone(args, [&] { return run_process(args); });
}

ExitCodeOrError run_process_stream_output_traced(
    const ProcessArgs &args,
    function_ref<void(string stdout_data, string stderr_data)> on_output_fn) {
  return with_process_zone(
      args, [&] { return run_process_stream_output(args, on_output_fn); });
}

} // namespace ccelerate
