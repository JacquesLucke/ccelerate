namespace A {

template <typename T> struct Map {};

using Alias = Map<struct B *>;

struct Context {
  const struct B *gen;
  Alias *alias;
};

struct B {
  int x;
};

void use(const B *p, Alias *a) {
  (void)p;
  (void)a;
}

} // namespace A
