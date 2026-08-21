namespace N {

template <typename T> struct A {
  explicit A(int x) : v(x) {}
  int v;
};

template <typename T> struct B : A<T> {
  using A<T>::A;
};

} // namespace N

int main() { return N::B<int>(1).v; }
