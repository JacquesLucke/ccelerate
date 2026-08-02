// SPDX-License-Identifier: MIT

#include <filesystem>
#include <zmq.hpp>
#include <zmq_addon.hpp>

#include "default_endpoint.hh"
#include "wrap_io.hh"

#ifndef CCELERATE_WRAP_TYPE
#error "CCELERATE_WRAP_TYPE must be defined"
#endif

namespace ccelerate {

int call(const int argc, char **argv) {
  // Prepare the call struct that is passed to the server.
  wrap_io::WrappedProgramCall call;
  call.cwd = std::filesystem::current_path().string();
  call.program = wrap_io::WrappedProgram::CCELERATE_WRAP_TYPE;
  for (int i = 1; i < argc; ++i) {
    call.args.push_back(argv[i]);
  }

  zmq::message_t identity_msg;

  try {
    // Connect to the ccelerate server which will actually do the work.
    zmq::context_t ctx;
    zmq::socket_t socket(ctx, zmq::socket_type::dealer);
    const string endpoint = get_default_ccelerate_endpoint();
    socket.connect(endpoint);

    // Encode the request into a byte buffer.
    msgpack::sbuffer request;
    msgpack::pack(request, call);

    // Send call to the server. Return value can be ignored in blocking mode.
    socket.send(identity_msg, zmq::send_flags::sndmore);
    (void)socket.send(zmq::buffer(string_view(request.data(), request.size())),
                      zmq::send_flags::none);

    while (true) {
      vector<zmq::message_t> recv_parts;
      // Receive response from the server. Return value can be ignored in
      // blocking mode.
      (void)zmq::recv_multipart(socket, std::back_inserter(recv_parts));
      if (recv_parts.size() < 2) {
        throw std::runtime_error("Invalid response");
      }
      const zmq::message_t &response = recv_parts.end()[-1];

      // Parse the response struct.
      wrap_io::WrappedProgramResult program_result;
      msgpack::unpack(response.data<char>(), response.size())
          .get()
          .convert(program_result);
      fmt::print(stdout, "{}", program_result.stdout);
      fmt::print(stderr, "{}", program_result.stderr);
      if (program_result.exit_code) {
        return *program_result.exit_code;
      }
    }
  } catch (const zmq::error_t &e) {
    fmt::print("Communication error: {}\n", e.what());
    return 1;
  } catch (const msgpack::type_error &e) {
    fmt::print("Invalid response: {}\n", e.what());
    return 1;
  } catch (const msgpack::parse_error &e) {
    fmt::print("Invalid response: {}\n", e.what());
    return 1;
  } catch (const std::exception &e) {
    fmt::print("Exception: {}\n", e.what());
    return 1;
  }
  return 1;
}

} // namespace ccelerate

int main(int argc, char **argv) { return ccelerate::call(argc, argv); }