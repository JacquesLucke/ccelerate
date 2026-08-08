enum class A { VALUE_A = 0 };

namespace {
enum class B { VALUE_B = 1 };
}

namespace test {
enum class C { VALUE_C = 2 };
}

int main() {
  return int(A::VALUE_A) + int(B::VALUE_B) + int(test::C::VALUE_C);
}
