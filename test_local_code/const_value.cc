constexpr int a = 1;
const int b = 2;
int c = 3;
extern const int d = 10;

namespace test {
constexpr int e = 4;
const int f = 5;
int g = 6;
extern const int h = 11;
} // namespace test
namespace {
constexpr int i = 7;
const int j = 8;
int k = 9;
extern const int l = 12;
} // namespace

int main() {
  return a + b + c + d + test::e + test::f + test::g + test::h + i + j + k + l;
}
