// SPDX-License-Identifier: MIT

#include <fcntl.h>
#include <fmt/format.h>
#include <fmt/ranges.h>
#include <poll.h>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>
#include <spawn.h>
#include <spdlog/spdlog.h>
#include <sys/wait.h>
#include <tracy/Tracy.hpp>
#include <unordered_map>

#include "array.hh"
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

enum class RunProcessError {
  PipeFailed = 1,
  PollFailed = 2,
  WaitFailed = 3,
  ReadFailed = 4,
  SpawnFailed = 5,
};

struct RunProcessErrorCategory : std::error_category {
  const char *name() const noexcept override { return "RunProcessError"; }

  std::string message(const int error) const override {
    switch (static_cast<RunProcessError>(error)) {
      case RunProcessError::PipeFailed:
        return "Failed to create pipe";
      case RunProcessError::PollFailed:
        return "Failed to poll";
      case RunProcessError::WaitFailed:
        return "Failed to wait";
      case RunProcessError::ReadFailed:
        return "Failed to read";
      case RunProcessError::SpawnFailed:
        return "Failed to spawn";
    }
    return "Unknown error";
  };
};

static const RunProcessErrorCategory run_process_error_category;

} // namespace ccelerate
namespace std {
template <>
struct is_error_code_enum<ccelerate::RunProcessError> : true_type {};
} // namespace std
namespace ccelerate {

error_code make_error_code(const RunProcessError e) {
  return error_code(static_cast<int>(e), run_process_error_category);
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

  constexpr int read_end = 0;
  constexpr int write_end = 1;

  int stdout_pipe[2];
  int stderr_pipe[2];

  if (pipe2(stdout_pipe, O_CLOEXEC) != 0) {
    return ProcessResult::from_error(
        make_error_code(RunProcessError::PipeFailed));
  }
  if (pipe2(stderr_pipe, O_CLOEXEC) != 0) {
    return ProcessResult::from_error(
        make_error_code(RunProcessError::PipeFailed));
  }

  posix_spawn_file_actions_t actions;
  posix_spawn_file_actions_init(&actions);

  posix_spawn_file_actions_adddup2(
      &actions, stdout_pipe[write_end], STDOUT_FILENO);
  posix_spawn_file_actions_adddup2(
      &actions, stderr_pipe[write_end], STDERR_FILENO);

  vector<char *> process_argv;
  for (const std::string &arg : args.data.args) {
    process_argv.push_back(const_cast<char *>(arg.c_str()));
  }
  process_argv.push_back(nullptr);

  vector<string> process_env_vars;
  std::unordered_map<string, string> env_vars;
  for (const auto &[key, value] : args.data.env_vars) {
    if (env_vars.contains(key)) {
      spdlog::warn("Duplicate environment variable: {}={}", key, value);
      continue;
    }
    process_env_vars.push_back(fmt::format("{}={}", key, value));
    env_vars.insert({std::move(key), std::move(value)});
  }
  if (args.data.env_mode == ProcessArgs::EnvMode::Inherit) {
    for (char **env = environ; *env != nullptr; env++) {
      const char *equals = strchr(*env, '=');
      if (!equals) {
        continue;
      }
      string key(*env, equals - *env);
      string value(equals + 1);
      if (env_vars.contains(key)) {
        continue;
      }
      env_vars.insert({std::move(key), std::move(value)});
    }
  }
  vector<char *> process_envp;
  for (const string &env_var : process_env_vars) {
    process_envp.push_back(const_cast<char *>(env_var.c_str()));
  }
  process_envp.push_back(nullptr);

  pid_t pid = 0;
  posix_spawnp(&pid,
               args.data.args[0].c_str(),
               &actions,
               nullptr,
               process_argv.data(),
               process_envp.data());
  posix_spawn_file_actions_destroy(&actions);
  if (pid == 0) {
    return ProcessResult::from_error(
        make_error_code(RunProcessError::SpawnFailed));
  }

  close(stdout_pipe[write_end]);
  close(stderr_pipe[write_end]);

  array<pollfd, 2> pollfds = {
      pollfd{stdout_pipe[read_end], POLLIN, 0},
      pollfd{stderr_pipe[read_end], POLLIN, 0},
  };

  string captured_stdout;
  string captured_stderr;
  array<char, 4096> buffer;
  int open_pipes = 2;

  bool any_poll_failed = false;
  bool any_read_failed = false;
  while (open_pipes > 0 && !any_read_failed) {
    const int num_events = poll(pollfds.data(), pollfds.size(), -1);
    if (num_events < 0) {
      any_poll_failed = true;
      break;
    }
    for (size_t fd_i = 0; fd_i < pollfds.size(); fd_i++) {
      pollfd &fd = pollfds[fd_i];
      if (fd.revents & (POLLIN | POLLHUP)) {
        const ssize_t bytes_read = read(fd.fd, buffer.data(), buffer.size());
        if (bytes_read > 0) {
          if (fd_i == 0) {
            captured_stdout.append(buffer.data(), bytes_read);
          } else {
            captured_stderr.append(buffer.data(), bytes_read);
          }
        } else if (bytes_read == 0) {
          close(fd.fd);
          fd.fd = -1;
          open_pipes--;
        } else {
          any_read_failed = true;
          break;
        }
      }
    }
  }

  int exit_code;
  if (waitpid(pid, &exit_code, 0) < 0) {
    return ProcessResult::from_error(
        make_error_code(RunProcessError::WaitFailed));
  }
  if (WIFEXITED(exit_code)) {
    exit_code = WEXITSTATUS(exit_code);
  }
  if (any_read_failed) {
    return ProcessResult::from_error(
        make_error_code(RunProcessError::ReadFailed),
        std::move(captured_stdout),
        std::move(captured_stderr));
  }
  if (any_poll_failed) {
    return ProcessResult::from_error(
        make_error_code(RunProcessError::PollFailed),
        std::move(captured_stdout),
        std::move(captured_stderr));
  }
  return ProcessResult::from_finished(
      exit_code, std::move(captured_stdout), std::move(captured_stderr));

  // reproc::options options;
  // if (args.data.working_dir) {
  //   options.working_directory = args.data.working_dir->c_str();
  // }
  // options.redirect.out.type = reproc::redirect::pipe;
  // options.redirect.err.type = reproc::redirect::pipe;
  // options.env.behavior = args.data.env_mode == ProcessArgs::EnvMode::Replace
  //                            ? reproc::env::type::empty
  //                            : reproc::env::type::extend;
  // options.env.extra = args.data.env_vars;
  // reproc::process proc;
  // {
  //   const error_code ec = proc.start(args.data.args, options);
  //   if (ec) {
  //     return ProcessResult::from_error(ec);
  //   }
  // }
  // std::string stdout;
  // std::string stderr;
  // {
  //   const error_code ec = reproc::drain(
  //       proc, reproc::sink::string(stdout), reproc::sink::string(stderr));
  //   if (ec) {
  //     return ProcessResult::from_error(
  //         ec, std::move(stdout), std::move(stderr));
  //   }
  // }
  // const auto &&[exit_code, ec] = proc.wait(reproc::infinite);
  // if (ec) {
  //   return ProcessResult::from_error(ec, std::move(stdout),
  //   std::move(stderr));
  // }
  // return ProcessResult::from_finished(
  //     exit_code, std::move(stdout), std::move(stderr));
}

} // namespace ccelerate