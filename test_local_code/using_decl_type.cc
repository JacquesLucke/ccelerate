namespace A {
namespace B {

struct C {
  struct D {
    static constexpr int k = 1;
  };
  enum class E { F = 2 };
};

} // namespace B
} // namespace A

namespace N {
using namespace A;
using B::C;

int use() {
  // Names imported by a using-declaration are UsingType in the AST and must
  // still be fully qualified at use sites (and in nested-name prefixes).
  C x;
  C::D d{};
  (void)x;
  return int(C::E::F) + d.k;
}

} // namespace N

int main() { return N::use(); }
