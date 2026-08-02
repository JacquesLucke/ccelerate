// SPDX-License-Identifier: MIT

#include "common_c.h"
#include "common_cc.hh"

#include <cstdio>

int main() {
  printf("%s\n", get_message_c());
  printf("%s\n", get_message_cc().c_str());
  return 0;
}
