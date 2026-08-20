namespace A {
void a();
}

namespace B {
struct BStruct {};

void fn(const BStruct &b);
} // namespace B

using namespace A;

void test() {
  a();
  fn(B::BStruct());
}