// SPDX-License-Identifier: MIT

#include <clang/CodeGen/CodeGenAction.h>
#include <clang/Driver/Compilation.h>
#include <clang/Driver/Driver.h>
#include <clang/Frontend/CompilerInstance.h>
#include <clang/Frontend/FrontendOptions.h>
#include <clang/Frontend/TextDiagnosticPrinter.h>
#include <clang/FrontendTool/Utils.h>
#include <filesystem>
#include <llvm/Config/llvm-config.h>
#include <llvm/Support/TargetSelect.h>
#include <llvm/Support/VirtualFileSystem.h>
#include <llvm/TargetParser/Host.h>
#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>
#include <tbb/task_arena.h>
#include <tbb/task_group.h>
#include <zmq.hpp>
#include <zmq_addon.hpp>

#include "default_endpoint.hh"
#include "get_current_executable_path.hh"
#include "program_wrapper.hh"

namespace ccelerate {

static std::string inproc_endpoint = "inproc://server";

struct GlobalState {
  std::filesystem::path binary_path;
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

static void send_program_result(std::vector<zmq::message_t> &request_frames,
                                const WrappedProgramResult &result) {
  ThreadState &thread_state = get_thread_state();
  msgpack::sbuffer response;
  msgpack::pack(response, result);

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

static void
pass_through_external_call(std::vector<zmq::message_t> &request_frames,
                           const std::vector<std::string> &args,
                           reproc::options options) {
  options.redirect.out.type = reproc::redirect::pipe;
  options.redirect.err.type = reproc::redirect::pipe;
  reproc::process proc;
  std::error_code ec = proc.start(args, options);
  WrappedProgramResult program_result;
  if (!ec) {
    ec = reproc::drain(proc, reproc::sink::string(program_result.stdout),
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

  send_program_result(request_frames, program_result);
}

static void
handle_eager_program_call(std::vector<zmq::message_t> &request_frames,
                          const WrappedProgramCall &call) {
  reproc::options options;
  options.working_directory = call.cwd.c_str();
  std::vector<std::string> args;
  args.push_back(std::string(to_string(call.program)));
  for (const auto &arg : call.args) {
    args.push_back(arg);
  }
  pass_through_external_call(request_frames, args, std::move(options));
}

static void handle_cmake_call(std::vector<zmq::message_t> &request_frames,
                              const WrappedProgramCall &call) {
  const GlobalState &global_state = get_global_state();
  const std::filesystem::path dir = global_state.binary_path.parent_path();

  reproc::options options;
  options.working_directory = call.cwd.c_str();
  const std::vector<std::pair<std::string, std::string>> extra_env = {
      {"CC", dir / "ccelerate_clang"},
      {"CXX", dir / "ccelerate_clang++"},
  };
  options.env.extra = extra_env;
  std::vector<std::string> args;
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

static void handle_clang_call(std::vector<zmq::message_t> &request_frames,
                              const WrappedProgramCall &call) {
  // TODO: Need to get header files compatible with the linked clang version.
  std::string clang_path = "/usr/bin/clang-18";

  std::vector<const char *> driver_args;
  driver_args.push_back(clang_path.c_str());
  for (const std::string &arg : call.args) {
    driver_args.push_back(arg.c_str());
  }

  llvm::SmallVector<char> captured_stdout;
  std::string captured_stderr;

  llvm::raw_string_ostream stderr_stream(captured_stderr);

  clang::IntrusiveRefCntPtr<clang::DiagnosticOptions> diag_ops =
      new clang::DiagnosticOptions();
  clang::TextDiagnosticPrinter *diag_client =
      new clang::TextDiagnosticPrinter(stderr_stream, &*diag_ops);
  clang::IntrusiveRefCntPtr<clang::DiagnosticIDs> diag_id(
      new clang::DiagnosticIDs());

  clang::DiagnosticsEngine diags(diag_id, diag_ops, diag_client);

  clang::CompilerInstance clang;
  clang.createDiagnostics(diag_client, false);
  clang.setOutputStream(
      std::make_unique<llvm::raw_svector_ostream>(captured_stdout));

  WrappedProgramResult result;

  std::string target_triple = llvm::sys::getProcessTriple();
  clang::driver::Driver driver(clang_path, target_triple, diags);
  std::unique_ptr<clang::driver::Compilation> compilation{
      driver.BuildCompilation(driver_args)};
  int exit_code = 0;
  if (compilation && !compilation->containsError()) {
    llvm::SmallVector<std::pair<int, const clang::driver::Command *>, 4>
        failing_commands;
    for (auto &job : compilation->getJobs()) {
      job.Print(llvm::outs(), "\n", true, nullptr);
    }
    exit_code = driver.ExecuteCompilation(*compilation, failing_commands);
  } else {
    exit_code = 1;
  }
  // if (clang.getDiagnostics().hasErrorOccurred()) {
  //   exit_code = 1;
  // }

  stderr_stream.flush();
  result.stderr = captured_stderr;
  result.stdout.insert(result.stdout.end(), captured_stdout.begin(),
                       captured_stdout.end());
  result.exit_code = exit_code;

  send_program_result(request_frames, result);
}

static void
handle_incoming_message(std::vector<zmq::message_t> &request_frames) {

  const zmq::message_t &actual_message = request_frames.end()[-1];
  WrappedProgramCall call;
  msgpack::unpack(actual_message.data<char>(), actual_message.size())
      .get()
      .convert(call);
  fmt::println("call: {}", call);
  switch (call.program) {
  case WrappedProgram::Clang:
  case WrappedProgram::Clangxx: {
    handle_clang_call(request_frames, call);
    break;
  }
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
    const std::string endpoint = get_default_ccelerate_endpoint();
    external_sock.bind(endpoint);
    fmt::println("Listening on {}", endpoint);

    zmq::socket_t proxy_sock(global_state.ctx, zmq::socket_type::dealer);
    proxy_sock.bind(inproc_endpoint);

    std::vector<zmq::pollitem_t> poll_items = {
        {external_sock.handle(), 0, ZMQ_POLLIN, 0},
        {proxy_sock.handle(), 0, ZMQ_POLLIN, 0},
    };

    while (true) {
      zmq::poll(poll_items);

      // Handle new incoming request from other process.
      if (poll_items[0].revents & ZMQ_POLLIN) {
        auto parts = std::make_shared<std::vector<zmq::message_t>>();
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