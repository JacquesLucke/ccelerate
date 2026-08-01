#include <cstdlib>
#include <string>

namespace ccelerate {

inline std::string get_default_ccelerate_endpoint() {
  const char *endpoint = std::getenv("CCELERATE_ENDPOINT");
  if (endpoint) {
    return endpoint;
  }
  return "ipc:///tmp/ccelerate.ipc";
}

} // namespace ccelerate