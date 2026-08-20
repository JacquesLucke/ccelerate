namespace N {

template <typename T> class A {
public:
  A() {}
  A(const A &other) {}
  A(A &&other) noexcept {}
  ~A() = default;
  A &operator=(const A &other) { return *this; }
  A &operator=(A &&other) noexcept { return *this; }
};

template <typename T> struct B {
  T len;
  B(const B &other) : len(other.len) {}
  B(B &&other) noexcept : len(other.len) {}
  B &operator=(const B &other) { return *this; }
  B &operator=(B &&other) { return *this; }
};

template <typename T> void use(A<T> *p) { (void)p; }

} // namespace N

void test() {
  N::A<int> x;
  N::B<int> e{0};
  N::use(&x);
  (void)e;
}
