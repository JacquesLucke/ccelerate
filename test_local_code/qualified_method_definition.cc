namespace A {
struct S {
  struct P {
    S x();
  };
};

S S::P::x() { return {}; }
} // namespace A