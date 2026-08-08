int x = 3;
static int y = 4;

namespace hello {
int z = 5;
int get_value() { return 10; }
namespace {
int w = 6;
int get_other_value() { return 11; }
} // namespace
} // namespace hello

int main() {
  return x + y + hello::z + hello::w + hello::get_value() +
         hello::get_other_value();
}