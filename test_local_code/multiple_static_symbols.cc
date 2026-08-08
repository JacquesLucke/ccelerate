#include <vector>

static int get_size() { return 10; }

namespace hello {
static std::vector<int> v;
static int result;

static int compute() {
  v.resize(get_size(), 2);
  int sum = 0;
  for (const int &i : v) {
    sum += i;
  }
  result = sum;
}

} // namespace hello