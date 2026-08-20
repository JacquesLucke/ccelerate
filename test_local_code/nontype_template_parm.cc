namespace A {

template <int N> struct B {
  static int get();
};

template <int N> int B<N>::get() { return N; }

} // namespace A

int main() { return A::B<4>::get(); }
