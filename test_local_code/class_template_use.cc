namespace {

template <typename T>
struct A {
  T v;
  explicit A(T x) : v(x) {}
};

template <typename U>
struct B {
  A<U> *p;
  explicit B(A<U> *q) : p(q) {}
};

template <typename T>
int f(T x) {
  A<T> a(x);
  B<T> b(&a);
  return static_cast<int>(b.p->v);
}

} // namespace

int main() { return f(1); }
