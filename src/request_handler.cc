// SPDX-License-Identifier: MIT

#include <spdlog/spdlog.h>
#include <tracy/Tracy.hpp>

#include "error_code.hh"
#include "request_handler.hh"
#include "run_process.hh"

namespace ccelerate {

void pass_through_external_call(const ClientID &client_id,
                                const ProcessArgs &args) {
  const ExitCodeOrError exit_code_or_error = run_process_stream_output(
      args, [&](string stdout_data, string stderr_data) {
        send_response_incomplete(
            client_id, std::move(stdout_data), std::move(stderr_data));
      });
  if (const error_code *ec = std::get_if<error_code>(&exit_code_or_error)) {
    string error_msg = ec->message();
    send_response_final(client_id, "", std::move(error_msg), 1);
    return;
  }
  const int exit_code = std::get<int>(exit_code_or_error);
  send_response_final(client_id, "", "", exit_code);
}

void handle_request__eager(const Request &request) {
  pass_through_external_call(request.client_id,
                             ProcessArgs()
                                 .arg(to_string(request.program))
                                 .args(request.args)
                                 .working_dir(request.working_dir));
}

static tracy::Color::ColorType
get_program_color(const wrap_io::Program program) {
  switch (program) {
    case wrap_io::Program::Clang:
    case wrap_io::Program::Clangxx:
      return tracy::Color::ColorType(0xB27236);
    case wrap_io::Program::Ar:
      return tracy::Color::ColorType(0xB9968D);
    case wrap_io::Program::CMake:
      return tracy::Color::ColorType(0x415557);
  }
  return tracy::Color::White;
}

void handle_request(const Request &request) {
  ZoneScoped;
  const string zone_name =
      fmt::format("request: {}", to_string(request.program));
  const string zone_text = fmt::format("{} {}\ncwd: {}",
                                       to_string(request.program),
                                       fmt::join(request.args, " "),
                                       request.working_dir.string());
  ZoneName(zone_name.data(), zone_name.size());
  ZoneText(zone_text.data(), zone_text.size());
  ZoneColor(get_program_color(request.program));

  spdlog::info("Handling request: {} {}",
               to_string(request.program),
               fmt::join(request.args, " "));
  switch (request.program) {
    case wrap_io::Program::Clang:
    case wrap_io::Program::Clangxx: {
      handle_request__clang(request);
      break;
    }
    case wrap_io::Program::Ar: {
      handle_request__eager(request);
      break;
    }
    case wrap_io::Program::CMake:
      handle_request__cmake(request);
      break;
  }
}

} // namespace ccelerate