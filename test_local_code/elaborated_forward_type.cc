namespace A {

struct Context {
  const struct B *gen;
};

struct B {
  int x;
};

void use(const B *p) { (void)p; }

} // namespace A
