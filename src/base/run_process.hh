// SPDX-License-Identifier: MIT

#pragma once

#include "error_code.hh"
#include "filesystem.hh"
#include "optional.hh"
#include "pair.hh"
#include "span.hh"
#include "string.hh"
#include "variant.hh"
#include "vector.hh"

namespace ccelerate {

struct ProcessArgs {
  enum class EnvMode {
    Inherit,
    Replace,
  };

  struct {
    vector<string> args;
    EnvMode env_mode = EnvMode::Inherit;
    vector<pair<string, string>> env_vars;
    optional<path> working_dir;
  } data;

  ProcessArgs &env_mode(const EnvMode mode) {
    this->data.env_mode = mode;
    return *this;
  }

  ProcessArgs &arg(string arg) {
    this->data.args.push_back(std::move(arg));
    return *this;
  }

  ProcessArgs &args(std::ranges::range auto &&r) {
    this->data.args.insert(this->data.args.end(), std::begin(r), std::end(r));
    return *this;
  }

  ProcessArgs &args(std::initializer_list<string> r) {
    return this->args(span(r));
  }

  ProcessArgs &env(string key, string value) {
    this->data.env_vars.push_back(
        std::make_pair(std::move(key), std::move(value)));
    return *this;
  }

  ProcessArgs &env(pair<string, string> var) {
    this->data.env_vars.push_back(std::move(var));
    return *this;
  }

  ProcessArgs &envs(std::ranges::range auto &&r) {
    this->data.env_vars.insert(
        this->data.env_vars.end(), std::begin(r), std::end(r));
    return *this;
  }

  ProcessArgs &working_dir(path dir) {
    this->data.working_dir = std::move(dir);
    return *this;
  }
};

struct ProcessResult {
private:
  using ExitOrError = variant<int, error_code>;
  ExitOrError exit_or_error_;

  ProcessResult(ExitOrError exit_or_error,
                string stdout_data,
                string stderr_data)
      : exit_or_error_(std::move(exit_or_error)),
        stdout_data(std::move(stdout_data)),
        stderr_data(std::move(stderr_data)) {}

public:
  string stdout_data;
  string stderr_data;

  static ProcessResult
  from_error(error_code ec, string stdout_data = "", string stderr_data = "");
  static ProcessResult from_finished(int exit_code,
                                     string stdout_data = "",
                                     string stderr_data = "");

  optional<int> exit_code() const {
    if (const int *exit_code_ptr = std::get_if<int>(&exit_or_error_)) {
      return *exit_code_ptr;
    }
    return nullopt;
  }

  optional<error_code> error() const {
    if (const error_code *ec_ptr = std::get_if<error_code>(&exit_or_error_)) {
      return *ec_ptr;
    }
    return nullopt;
  }
};

ProcessResult run_process(const ProcessArgs &args);

} // namespace ccelerate