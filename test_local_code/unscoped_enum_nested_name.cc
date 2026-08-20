#include "unscoped_enum_nested_name.hh"

namespace N {
using namespace A;
using namespace A::B;

int use() {
  // Unscoped Enum::Enumerator: qualify the type, not the enumerator token.
  return int(C::D) + int(C::E) + int(F::G);
}

int local_alias() {
  using G = C;
  // Function-local alias must not become `::G`.
  return int(G::D);
}

int local_struct() {
  struct H {
    int array[4];
  };
  // Function-local type must not become `::H`.
  return sizeof(H::array);
}

} // namespace N
