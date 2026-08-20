#include "out_of_line_ctor.hh"

namespace A {

B::B() = default;
B::B(const B &other) = default;
B::B(B &&other) = default;
B::~B() = default;
B &B::operator=(const B &other) = default;
B &B::operator=(B &&other) = default;

} // namespace A
