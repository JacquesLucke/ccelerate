// SPDX-License-Identifier: MIT

#include <filesystem>
#include <zmq.hpp>

#include "program_wrapper.hh"

#ifndef CCELERATE_WRAP_TYPE
#error "CCELERATE_WRAP_TYPE must be defined"
#endif

namespace ccelerate {

int call(const int argc, char **argv) {
  // Prepare the call struct that is passed to the server.
  WrappedProgramCall call;
  call.cwd = std::filesystem::current_path().string();
  call.program = WrappedProgram::CCELERATE_WRAP_TYPE;
  for (int i = 1; i < argc; ++i) {
    call.args.push_back(argv[i]);
  }

  try {
    // Connect to the ccelerate server which will actually do the work.
    zmq::context_t ctx;
    zmq::socket_t socket(ctx, zmq::socket_type::req);
    const std::string endpoint = get_socket_endpoint();
    socket.connect(endpoint);

    // Encode the request into a byte buffer.
    msgpack::sbuffer request;
    msgpack::pack(request, call);

    // Send call to the server. Return value can be ignored in blocking mode.
    (void)socket.send(
        zmq::buffer(std::string_view(request.data(), request.size())),
        zmq::send_flags::none);

    // Receive response from the server. Return value can be ignored in blocking
    // mode.
    zmq::message_t response;
    (void)socket.recv(response, zmq::recv_flags::none);

    // Parse the response struct.
    WrappedProgramResult program_result;
    msgpack::unpack(response.data<char>(), response.size())
        .get()
        .convert(program_result);

    fmt::print(stdout, "{}", program_result.stdout);
    fmt::print(stderr, "{}", program_result.stderr);
    return program_result.exit_code;
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