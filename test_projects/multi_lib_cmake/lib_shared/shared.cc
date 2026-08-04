// SPDX-License-Identifier: MIT

#include "shared.hh"

#include "alpha.hh"
#include "beta.hh"
#include "gamma.hh"
#include "shared_parts.hh"

#include <string>

namespace multi_lib {

std::string shared_message() {
  return alpha::combine() + beta::combine() + gamma::combine() +
         shared_parts::part_r() + shared_parts::part_s();
}

} // namespace multi_lib
