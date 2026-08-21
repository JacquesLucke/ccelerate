namespace N {

struct B {
  static constexpr int k = 1;
};

namespace {

struct A {
  using C = B;
  int f() { return C::k; }
};

} // namespace

int use() { return A().f(); }

} // namespace N

int main() { return N::use(); }
