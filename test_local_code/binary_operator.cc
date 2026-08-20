namespace A {
struct S {
  friend bool operator==(const S &a, const S &b);
};
} // namespace A

bool test() {
  using namespace A;
  return S() == S();
}