#include <memory>
#include <string>

namespace A {

struct B {
  B(int context, std::string s, int font);
};

struct C {
  B &get(int context, const std::string string, int font) {
    auto &b = *std::make_unique<B>(context, string, font);
    return b;
  }

  B &get_lambda(int context, const std::string string, int font) {
    auto make = [&]() {
      return std::make_unique<B>(context, string, font);
    };
    auto &b = *make();
    return b;
  }
};

} // namespace A
