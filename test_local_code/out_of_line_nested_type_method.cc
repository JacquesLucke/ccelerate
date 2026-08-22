#include "out_of_line_nested_type_method.hh"

namespace A {

B::C B::D::f() const {
  return C{};
}

} // namespace A
