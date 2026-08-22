namespace A {

class B {
  struct C {
    int v;
  };

  struct D {
    C f() const;
  };
};

} // namespace A
