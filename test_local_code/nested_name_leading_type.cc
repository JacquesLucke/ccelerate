namespace N {
namespace M {

struct A {
  struct B {
    static B &get() {
      static B data;
      return data;
    }
  };
  using C = B;
};

template <typename T> struct D {
  static int value() { return 0; }

  struct E {
    static E &get() {
      static E data;
      return data;
    }
  };
};

} // namespace M

namespace Other {
struct A {};
} // namespace Other
} // namespace N

namespace N {
using namespace M;

void use() {
  A::B &x = A::B::get();
  (void)x;

  A::C &y = A::C::get();
  (void)y;

  int v = D<int>::value();
  (void)v;

  D<int>::E &z = D<int>::E::get();
  (void)z;
}
} // namespace N
