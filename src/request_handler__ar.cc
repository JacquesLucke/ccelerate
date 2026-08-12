// SPDX-License-Identifier: MIT

#include <fmt/format.h>
#include <fstream>
#include <toml.hpp>

#include "ccelerate_extensions.hh"
#include "request_handler.hh"

namespace ccelerate {

void handle_request__ar(const Request &request) {
  if (request.args.size() <= 1) {
    handle_request__eager(request);
    return;
  }
  // https://sourceware.org/binutils/docs/binutils/ar-cmdline.html
  const string_view operation_arg = request.args[0];
  if (operation_arg == "qc") {
    const path archive_file =
        std::filesystem::absolute(request.working_dir / request.args[1])
            .lexically_normal();
    vector<string> src_files;
    for (size_t i = 2; i < request.args.size(); i++) {
      src_files.push_back(
          std::filesystem::absolute(request.working_dir / request.args[i])
              .lexically_normal());
    }
    path ccelerate_output_file = archive_file;
    ccelerate_output_file.replace_extension(extensions::archive);

    toml::array_format_info fmt;
    fmt.fmt = toml::array_format::multiline;
    toml::value table;
    table["sources"] = toml::value(src_files, fmt);
    const string toml_str = toml::format(table);
    std::ofstream file(ccelerate_output_file);
    file << toml_str;

    handle_request__eager(request);
  } else {
    handle_request__eager(request);
  }
}

} // namespace ccelerate