namespace A {

struct Node {
  static constexpr int k = 3;
};

struct B {
  using Inner = Node;
  template <int N> struct Leaf {};
  using Open = Leaf<Inner::k>;
};

template <typename T> struct C {
  using Inner = Node;
  template <int N> struct Box {};
  using Open = Box<Inner::k>;
};

} // namespace A
