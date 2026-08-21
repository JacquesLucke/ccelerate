namespace N {

namespace {

struct A {};

template <typename T> struct B {
  struct C {
    static constexpr int k = 1;
  };
};

} // namespace

int use() {
  // Qualifying `C` replaces `B<A>::C` wholesale; the `A` template argument only
  // survives if the generated name applies the local id too.
  return B<A>::C::k;
}

} // namespace N

int main() { return N::use(); }
