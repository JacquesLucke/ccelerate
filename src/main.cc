// SPDX-License-Identifier: MIT

#include <reproc++/drain.hpp>
#include <reproc++/reproc.hpp>
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

      reproc::options options;
      options.working_directory = call.cwd.c_str();
      options.redirect.out.type = reproc::redirect::pipe;
      options.redirect.err.type = reproc::redirect::pipe;
      reproc::process proc;
      std::vector<std::string> args;
      args.push_back(std::string(to_string(call.program)));
      for (const auto &arg : call.args) {
        args.push_back(arg);
      }
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