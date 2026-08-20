namespace A {
namespace B {
int x = 1;
}
} // namespace A

namespace C {
namespace A {
int y = 2;
}

void use() {
  using namespace A::B;
  (void)x;
}

} // namespace C
