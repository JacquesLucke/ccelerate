namespace {

struct A {
  explicit A(int x) : v(x) {}
  int v;
};

struct B : A {
  using A::A;
};

} // namespace

int main() { return B(1).v; }
