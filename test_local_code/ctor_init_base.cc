namespace A {

struct B {
  explicit B(int x) : v(x) {}
  int v;
};

struct C : B {
  C();
};

// No space after the ctor-initializer colon (as in itasc FixedObject.cpp).
// A leading `::` must be preceded by a space or it becomes `:::`.
C::C():B(1) {}

} // namespace A

int main() { return A::C().v; }
