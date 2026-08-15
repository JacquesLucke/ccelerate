// SPDX-License-Identifier: MIT

#pragma once

#include "filesystem.hh"
#include "run_process.hh"
#include "string.hh"
#include "vector.hh"
#include "wrap_io.hh"

namespace ccelerate {

struct ClientID {
  vector<string> parts;
};

struct Request {
  ClientID client_id;
  wrap_io::Program program;
  path working_dir;
  vector<string> args;
};

void handle_request(const Request &request);
void handle_request__cmake(const Request &request);
void handle_request__clang(const Request &request);
void handle_request__ar(const Request &request);
void handle_request__eager(const Request &request);

void send_response_incomplete(const ClientID &client_id,
                              string stdout,
                              string stderr);
void send_response_final(const ClientID &client_id,
                         string stdout,
                         string stderr,
                         int exit_code);
void send_response_error(const ClientID &client_id, string_view message);

struct BuildLocalObjectsResult {
  vector<path> paths;
  bool success = false;
};
BuildLocalObjectsResult
build_local_objects(const ClientID &client_id,
                    const span<const path> local_obj_files);
optional<path> local_code_path_of_obj_if_exists(const path cwd,
                                                const path &obj_file);

} // namespace ccelerate