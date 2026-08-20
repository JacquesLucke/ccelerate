namespace A {
namespace B {
struct R {};
int test() {
  struct S {
    int x = 4;
    R r;
  };
  return S().x;
}
} // namespace B
} // namespace A