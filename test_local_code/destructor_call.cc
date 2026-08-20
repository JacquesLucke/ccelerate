namespace A {
namespace B {
namespace C {
struct Type {
  ~Type() {}
};
} // namespace C
} // namespace B
using Type = B::C::Type;
} // namespace A

using namespace A;

void end(Type *p) { p->~Type(); }
