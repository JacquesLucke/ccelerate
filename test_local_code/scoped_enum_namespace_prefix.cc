namespace A {
namespace B {

enum class C { D = 1, E = 2 };

constexpr int k = 3;

struct F {
  static constexpr int g = 4;
};

} // namespace B
} // namespace A

namespace N {
using namespace A;

int use() {
  // Relative namespace prefix must become a global qualifier, including the
  // enum type, enumerator, namespace-scope constant, and static data member.
  return int(B::C::D) + int(B::C::E) + B::k + B::F::g;
}

} // namespace N

int main() { return N::use(); }
