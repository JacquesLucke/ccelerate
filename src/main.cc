// SPDX-License-Identifier: MIT

#include <zmq.hpp>
#include <zmq_addon.hpp>

#include "program_wrapper.hh"

namespace ccelerate {

int ccelerate_main(const int argc, char **argv) {
  try {
    zmq::context_t ctx;
    zmq::socket_t socket(ctx, zmq::socket_type::router);
    const std::string endpoint = get_socket_endpoint();
    socket.bind(endpoint);

    fmt::println("Listening on {}", endpoint);

    while (true) {
      std::vector<zmq::message_t> recv_parts;
      (void)zmq::recv_multipart(socket, std::back_inserter(recv_parts));
      if (recv_parts.size() < 3) {
        // Expect at least an address, an empty frame and the actual message.
        continue;
      }

      const zmq::message_t &actual_message = recv_parts.end()[-1];
      WrappedProgramCall call;
      msgpack::unpack(actual_message.data<char>(), actual_message.size())
          .get()
          .convert(call);
      fmt::println("call: {}", call);

      WrappedProgramResult program_result;
      program_result.stdout = "Hello world!\n";
      program_result.stderr = "";

      msgpack::sbuffer response;
      msgpack::pack(response, program_result);

      for (int i = 0; i < recv_parts.size() - 1; ++i) {
        (void)socket.send(recv_parts[i], zmq::send_flags::sndmore);
      }
      (void)socket.send(zmq::message_t(response.data(), response.size()),
                        zmq::send_flags::none);
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