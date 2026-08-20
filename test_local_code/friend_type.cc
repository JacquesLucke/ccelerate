namespace A {
namespace {
struct R {};
struct S {
  friend R;
  friend class B;
};
class B {};

struct F {
  friend class FR;
};
struct L {
  friend class FR;
};
class FR {};
} // namespace
} // namespace A
