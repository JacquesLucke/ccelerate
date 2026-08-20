namespace A {

struct M {
  M();
  M(M &&other);
  M &operator=(M &&other);
};

struct B {
  M m;
  B();
  B(const B &other);
  B(B &&other);
  ~B();
  B &operator=(const B &other);
  B &operator=(B &&other);
};

} // namespace A
