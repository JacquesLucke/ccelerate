// SPDX-License-Identifier: MIT

#include <spdlog/spdlog.h>
#include <tbb/task_arena.h>
#include <tbb/task_group.h>
#include <tracy/Tracy.hpp>
#include <zmq.hpp>
#include <zmq_addon.hpp>

#include "config.hh"
#include "default_endpoint.hh"
#include "get_current_executable_path.hh"
#include "request_handler.hh"
#include "wrap_io.hh"

namespace ccelerate {

struct ServerGlobalState {
  // Process-wide zmq context so that we can send messages between threads.
  zmq::context_t ctx;
  // Each incoming request is added to the task group for parallel processing.
  tbb::task_group task_group;
};

struct ServerThreadState {
  // A socket for sending message to the main thread which communicates with
  // external processes.
  zmq::socket_t iproc_socket;

  ServerThreadState(ServerGlobalState &global_state) {
    this->iproc_socket =
        zmq::socket_t(global_state.ctx, zmq::socket_type::dealer);
    this->iproc_socket.connect(inproc_endpoint);
  }
};

static ServerGlobalState &get_global_state() {
  static ServerGlobalState global_state;
  return global_state;
}

static ServerThreadState &get_thread_state() {
  static thread_local ServerThreadState thread_state{get_global_state()};
  return thread_state;
}

static void send_response_frame(const ClientID &client_id,
                                const wrap_io::CallResponseFrame &frame) {
  ServerThreadState &thread_state = get_thread_state();

  // Serialize the response.
  msgpack::sbuffer response;
  msgpack::pack(response, frame);

  for (const string &id_part : client_id.parts) {
    (void)thread_state.iproc_socket.send(zmq::message_t(id_part),
                                         zmq::send_flags::sndmore);
  }
  (void)thread_state.iproc_socket.send(zmq::message_t(),
                                       zmq::send_flags::sndmore);
  (void)thread_state.iproc_socket.send(
      zmq::message_t(response.data(), response.size()), zmq::send_flags::none);
}

void send_response_incomplete(const ClientID &client_id,
                              string stdout_data,
                              string stderr_data) {
  wrap_io::CallResponseFrame frame;
  frame.stdout_data = std::move(stdout_data);
  frame.stderr_data = std::move(stderr_data);
  send_response_frame(client_id, frame);
}

void send_response_final(const ClientID &client_id,
                         string stdout_data,
                         string stderr_data,
                         const int exit_code) {
  wrap_io::CallResponseFrame frame;
  frame.stdout_data = std::move(stdout_data);
  frame.stderr_data = std::move(stderr_data);
  frame.exit_code = exit_code;
  send_response_frame(client_id, frame);
}

void send_response_error(const ClientID &client_id, const string_view message) {
  spdlog::error("{}", message);
  send_response_final(
      client_id, "", fmt::format("ccelerate: {}\n", message), 1);
}

static void handle_incoming_message(vector<zmq::message_t> &request_frames) {
  // A valid request has at least and id frame, one empty frame and the message.
  if (request_frames.size() <= 2) {
    fmt::println(stderr,
                 "Ignoring malformed request with {} frame(s).",
                 request_frames.size());
    return;
  }

  Request request;
  // The last frame and the empty frame before that are not part of the id.
  assert(request_frames[request_frames.size() - 2].size() == 0);
  for (size_t i = 0; i < request_frames.size() - 2; i++) {
    request.client_id.parts.push_back(request_frames[i].to_string());
  }
  const std::string message = request_frames.end()[-1].to_string();

  try {
    wrap_io::CallRequest call;
    msgpack::unpack(message.data(), message.size()).get().convert(call);

    request.args = std::move(call.args);
    request.working_dir = std::move(call.working_dir);
    switch (call.program) {
      case wrap_io::Program::Clang:
      case wrap_io::Program::Clangxx:
      case wrap_io::Program::Ar:
      case wrap_io::Program::CMake: {
        request.program = call.program;
        break;
      }
      default: {
        send_response_error(
            request.client_id,
            fmt::format("Unknown program id: {}", int(call.program)));
        return;
      }
    }

    handle_request(request);
  } catch (const msgpack::parse_error &e) {
    send_response_error(request.client_id,
                        fmt::format("Could not parse request: {}", e.what()));
  } catch (const msgpack::type_error &e) {
    send_response_error(request.client_id,
                        fmt::format("Unexpected request format: {}", e.what()));
  } catch (const std::exception &e) {
    send_response_error(
        request.client_id,
        fmt::format("Error while handling request: {}", e.what()));
  } catch (...) {
    send_response_error(request.client_id,
                        "Unknown error while handling request");
  }
}

int ccelerate_main(const int argc, char **argv) {
  ServerGlobalState &global_state = get_global_state();
  ConfigDiscovery::get().add_search_path(get_current_executable_path());
  ConfigDiscovery::get().add_search_path(std::filesystem::current_path());

  try {
    // Initialize socket for communicating with the wrapper processes.
    zmq::socket_t external_sock(global_state.ctx, zmq::socket_type::router);
    const string endpoint = get_default_ccelerate_endpoint();
    external_sock.bind(endpoint);
    spdlog::info("Listening on {}", endpoint);

    // Initialize internal socket for receiving messages that should be
    // forwarded to the wrapper processes.
    zmq::socket_t proxy_sock(global_state.ctx, zmq::socket_type::dealer);
    proxy_sock.bind(inproc_endpoint);

    // Use polling to be able to wait for incoming message on either of the
    // two sockets.
    vector<zmq::pollitem_t> poll_items = {
        {external_sock.handle(), 0, ZMQ_POLLIN, 0},
        {proxy_sock.handle(), 0, ZMQ_POLLIN, 0},
    };

    while (true) {
      // Wait until either of the sockets has a message.
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
