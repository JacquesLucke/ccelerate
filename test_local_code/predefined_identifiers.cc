static const char *A() { return __func__; }

namespace {
struct B {
  const char *f() { return __PRETTY_FUNCTION__; }
};
} // namespace

static const char *C()
{
  auto lambda = []() { return __FUNCTION__; };
  return lambda();
}

template<typename T> static const char *D() { return __func__; }

int main() { return A()[0] + B().f()[0] + C()[0] + D<int>()[0]; }
