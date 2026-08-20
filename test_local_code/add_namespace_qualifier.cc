namespace A {
void a();
}

namespace B {
struct BStruct {};

void fn(const BStruct &b);
} // namespace B

namespace C {
template <typename T> struct Box {
  T v;
  explicit Box(T x) : v(x) {}
};

typedef int IntAlias;
} // namespace C

using namespace A;
using namespace C;

void test() {
  a();
  fn(B::BStruct());

  Box<int> explicit_t(1);
  Box deduced_t(2);
  IntAlias alias_t = 0;
}
