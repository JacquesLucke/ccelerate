#include <array>

constexpr int a = 1;
constexpr int b = a + a;

template <int N> struct A {
  static std::array<float, N> data;
};

int main() { return A<b * 2>::data[0]; }
