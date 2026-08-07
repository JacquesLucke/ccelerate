// SPDX-License-Identifier: MIT

#include <cstdio>

static const char *test() { return "Hello World!\n"; }

int main() {
  printf("%s", test());
  return 0;
}