namespace A {

enum B : unsigned char {
  C = 'c',
};

} // namespace A

namespace D {
using namespace A;

namespace E {
struct F {};
struct C : F {};
} // namespace E

} // namespace D

namespace N {
using namespace D;

int use(B t) {
  return t == C;
}

} // namespace N
