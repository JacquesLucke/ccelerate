namespace A {
namespace B {

template <typename T> struct TraitsType {
  using type = int;
};
template <typename T> using Traits = typename TraitsType<T>::type;

template <typename Fn> void foreach_index(Fn &&fn) {
  // Constructing the callable can nest CXXConstructExpr above the lambda body.
  fn(0);
}

} // namespace B
} // namespace A

namespace N {
using namespace A;

int use() {
  int sum = 0;
  B::foreach_index([&](int i) {
    using Color = float;
    // Must still qualify despite enclosing CXXConstructExpr on the lambda.
    using Traits = B::Traits<Color>;
    Traits x = i;
    sum += x;
  });
  return sum;
}

} // namespace N

int main() { return N::use(); }
