#pragma once
#include <functional>

namespace A {
namespace B {
struct E {};
struct F {
  static const F &g();
};
struct Options {
  bool a = false;
  bool b = true;
  std::reference_wrapper<const F> c = F::g();
};
struct Result {
  E v;
};
Result f(E x, const Options &options);
} // namespace B

namespace C {
namespace D {
void h();
}
} // namespace C
} // namespace A
