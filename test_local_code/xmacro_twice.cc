#define A(name) \
  static int name() { \
    return 1; \
  }
#include "xmacro_twice.hh"
#undef A

#define A(name) x += name();
int main() {
  int x = 0;
#include "xmacro_twice.hh"
  return x;
}
