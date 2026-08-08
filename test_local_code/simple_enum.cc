enum A { VALUE_A = 0 };

namespace {
enum B { VALUE_B = 1 };
}

namespace test {
enum C { VALUE_C = 2 };
}

int main() { return int(VALUE_A) + int(VALUE_B) + int(test::VALUE_C); }