// SPDX-License-Identifier: MIT

#include <cstdio>

static const char *hello() { return "Hello World!\n"; }

int main() {
  printf("%s", hello());
  return 0;
}