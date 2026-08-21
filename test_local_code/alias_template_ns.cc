namespace A {
namespace B {

template <typename T> struct TraitsType {
  using type = int;
};
template <typename T> using Traits = typename TraitsType<T>::type;

} // namespace B
} // namespace A

namespace N {
using namespace A;

int use() {
  using Color = float;
  // Namespace-qualified alias-template-ids must be fully qualified (e.g.
  // blender `using Traits = color::Traits<Color>`).
  using Traits = B::Traits<Color>;
  Traits x = 0;
  return x;
}

} // namespace N

int main() { return N::use(); }
