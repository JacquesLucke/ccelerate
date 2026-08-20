#include "out_of_line_static_member.hh"

namespace A {

// Leading `::` on the declarator nested-name would parse as `T::A::B::…`.
const T B::k = 1;
T B::x = 0;
T B::f() { return x; }

} // namespace A
