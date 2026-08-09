struct A {
  explicit A(int x) : v(x) {}
  int v;
};

namespace {

struct B : A {
  using A::A;
};

struct C : B {
  using B::B;
};

} // namespace

int main() { return B(1).v; }
