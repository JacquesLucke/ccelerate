namespace N {
namespace A {

struct B {
  static constexpr int k = 1;
};

namespace {

// Same unqualified name as the enclosing namespace; string-replacing `A` in
// `::N::A::A::C` would rename the namespace instead of this type.
struct A {
  using C = B;
  int f() { return C::k; }
};

} // namespace

int use() { return A().f(); }

} // namespace A
} // namespace N

int main() { return N::A::use(); }
