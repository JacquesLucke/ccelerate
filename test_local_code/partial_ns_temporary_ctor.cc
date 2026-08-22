#include "partial_ns_temporary_ctor.hh"

namespace A {
namespace B {
const F &F::g() {
  static F f{};
  return f;
}
} // namespace B

namespace C {
namespace D {
void h() {
  B::E x{};
  auto r = B::f(x, B::Options()).v;
  (void)r;
}
} // namespace D
} // namespace C
} // namespace A
