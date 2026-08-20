namespace A {
namespace {
struct R {};
struct S {
  friend R;
  friend class B;
};
class B {};
} // namespace
} // namespace A