namespace ns {

struct A {
  int v;
};
struct B {
  int v;
};

// External linkage: must not be renamed.
template <typename T>
void f(T &dst, const A &src) {
  dst.v = src.v;
}

// Internal linkage: must also stay unrenamed so the overload set is not split.
static void f(A &dst, const B &src) {
  dst.v = src.v;
}

template <typename D, typename S>
void g(D *dst, const S *src) {
  f(dst[0], src[0]);
}

int use() {
  A a{};
  B b{1};
  g(&a, &b);
  return a.v;
}

} // namespace ns

int main() { return ns::use(); }
