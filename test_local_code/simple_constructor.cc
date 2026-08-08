namespace hello {
struct A {
  int x_;
  A(int x) : x_(x) {}
};
} // namespace hello

namespace {
struct B {
  int x_;
  B(int x) : x_(x) {}
};
} // namespace

namespace {
struct C {
  C();
  ~C();
  C(const C &);

  operator int() const;

  C hello();
};
} // namespace

C::C() = default;
C::~C() {}
C::C(const C &) {}
C::operator int() const { return 0; }

C C::hello() { return C(); }