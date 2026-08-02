// SPDX-License-Identifier: MIT

#include <filesystem>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>
#include <tbb/task_arena.h>
#include <tbb/task_group.h>
#include <zmq.hpp>
#include <zmq_addon.hpp>

#include "base/error_code.hh"
#include "base/get_current_executable_path.hh"
#include "base/pair.hh"
#include "default_endpoint.hh"
#include "program_wrapper.hh"

namespace ccelerate {

static string inproc_endpoint = "inproc://server";

struct GlobalState {
  path binary_path;
  zmq::context_t ctx;
  tbb::task_group task_group;
};

struct ThreadState {
  zmq::socket_t iproc_socket;

  ThreadState(GlobalState &global_state) {
    this->iproc_socket =
        zmq::socket_t(global_state.ctx, zmq::socket_type::dealer);
    this->iproc_socket.connect(inproc_endpoint);
  }
};

static GlobalState &get_global_state() {
  static GlobalState global_state;
  return global_state;
}

static ThreadState &get_thread_state() {
  static thread_local ThreadState thread_state{get_global_state()};
  return thread_state;
}

static void pass_through_external_call(vector<zmq::message_t> &request_frames,
                                       const vector<string> &args,
                                       reproc::options options) {
  ThreadState &thread_state = get_thread_state();

  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  reproc::process proc;
  error_code ec = proc.start(args, options);
  WrappedProgramResult program_result;
  if (!ec) {
    ec = reproc::drain(proc,
                       reproc::sink::string(program_result.stdout),
                       reproc::sink::string(program_result.stderr));
    if (!ec) {
      std::tie(program_result.exit_code, ec) = proc.wait(reproc::infinite);
    } else {
      program_result.exit_code = 1;
      program_result.stderr = ec.message();
    }
  } else {
    program_result.exit_code = 1;
    program_result.stderr = ec.message();
  }

  msgpack::sbuffer response;
  msgpack::pack(response, program_result);

  for (zmq::message_t &frame : request_frames) {
    if (!frame.more()) {
      /* The last frame is the actual message. */
      break;
    }
    zmq::message_t msg;
    msg.copy(frame);
    (void)thread_state.iproc_socket.send(msg, zmq::send_flags::sndmore);
  }
  (void)thread_state.iproc_socket.send(
      zmq::message_t(response.data(), response.size()), zmq::send_flags::none);
}

static void handle_eager_program_call(vector<zmq::message_t> &request_frames,
                                      const WrappedProgramCall &call) {
  reproc::options options;
  options.working_directory = call.cwd.c_str();
  vector<string> args;
  args.push_back(string(to_string(call.program)));
  for (const auto &arg : call.args) {
    args.push_back(arg);
  }
  pass_through_external_call(request_frames, args, std::move(options));
}

static void handle_cmake_call(vector<zmq::message_t> &request_frames,
                              const WrappedProgramCall &call) {
  const GlobalState &global_state = get_global_state();
  const path dir = global_state.binary_path.parent_path();

  reproc::options options;
  options.working_directory = call.cwd.c_str();
  const vector<pair<string, string>> extra_env = {
      {"CC", dir / "ccelerate_clang"},
      {"CXX", dir / "ccelerate_clang++"},
  };
  options.env.extra = extra_env;
  vector<string> args;
  args.push_back("cmake");
  bool has_build_arg = false;
  for (const auto &arg : call.args) {
    args.push_back(arg);
    if (arg == "--build") {
      has_build_arg = true;
    }
  }
  if (!has_build_arg) {
    args.push_back(
        fmt::format("-DCMAKE_AR={}", (dir / "ccelerate_ar").string()));
  }
  pass_through_external_call(request_frames, args, std::move(options));
}

static void handle_incoming_message(vector<zmq::message_t> &request_frames) {

  const zmq::message_t &actual_message = request_frames.end()[-1];
  WrappedProgramCall call;
  msgpack::unpack(actual_message.data<char>(), actual_message.size())
      .get()
      .convert(call);
  fmt::println("call: {}", call);
  switch (call.program) {
    case WrappedProgram::Clang:
    case WrappedProgram::Clangxx:
    case WrappedProgram::Ar: {
      handle_eager_program_call(request_frames, call);
      break;
    }
    case WrappedProgram::CMake:
      handle_cmake_call(request_frames, call);
      break;
  }
}

int ccelerate_main(const int argc, char **argv) {
  GlobalState &global_state = get_global_state();
  global_state.binary_path = get_current_executable_path();

  try {
    zmq::socket_t external_sock(global_state.ctx, zmq::socket_type::router);
    const string endpoint = get_default_ccelerate_endpoint();
    external_sock.bind(endpoint);
    fmt::println("Listening on {}", endpoint);

    zmq::socket_t proxy_sock(global_state.ctx, zmq::socket_type::dealer);
    proxy_sock.bind(inproc_endpoint);

    vector<zmq::pollitem_t> poll_items = {
        {external_sock.handle(), 0, ZMQ_POLLIN, 0},
        {proxy_sock.handle(), 0, ZMQ_POLLIN, 0},
    };

    while (true) {
      zmq::poll(poll_items);

      // Handle new incoming request from other process.
      if (poll_items[0].revents & ZMQ_POLLIN) {
        auto parts = std::make_shared<vector<zmq::message_t>>();
        (void)zmq::recv_multipart(external_sock, std::back_inserter(*parts));
        global_state.task_group.run(
            [parts = std::move(parts)]() { handle_incoming_message(*parts); });
      }

      // Handle response that should be forwarded from a thread to the external
      // process.
      if (poll_items[1].revents & ZMQ_POLLIN) {
        while (true) {
          zmq::message_t frame;
          (void)proxy_sock.recv(frame);
          if (frame.more()) {
            (void)external_sock.send(frame, zmq::send_flags::sndmore);
          } else {
            (void)external_sock.send(frame, zmq::send_flags::none);
            break;
          }
        }
      }
    }
  } catch (const std::exception &e) {
    fmt::print("Exception: {}\n", e.what());
    return 1;
  }
  return 0;
}

} // namespace ccelerate

int main(int argc, char **argv) {
  return ccelerate::ccelerate_main(argc, argv);
}