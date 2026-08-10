namespace {

template <typename T> T f(T x) { return x; }

template <typename T> struct S {
  T operator()(T x) const { return f(x); }
};

} // namespace

int main() { return 0; }
