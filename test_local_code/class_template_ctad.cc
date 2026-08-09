namespace {

template <typename T>
struct A {
  T v;
  explicit A(T x) : v(x) {}
};

template <typename T>
int f(T x) {
  return A(x).v;
}

} // namespace

int main() { return f(1); }
