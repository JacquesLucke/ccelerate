// SPDX-License-Identifier: MIT

#include "shared.hh"

#include <iostream>
#include <string>

namespace {

std::string part_t() { return "T"; }

} // namespace

int main() {
  std::cout << multi_lib::shared_message() << part_t() << '\n';
  return 0;
}
