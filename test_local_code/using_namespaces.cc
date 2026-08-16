namespace A {
namespace B {
int value = 1;
}
} // namespace A

namespace C {
using namespace A::B;

int use_value() { return value; }
} // namespace C

using namespace C;

namespace {
using namespace A;
}

namespace D {
namespace {
using namespace A;
}
} // namespace D

int main() {
  using namespace A;
  return use_value();
}
