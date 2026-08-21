namespace N {

namespace {

struct X {};

template <typename T> struct A {
  struct Inner {};
};

template <typename T> struct B {
  struct Mid {
    template <typename U> struct C {
      struct Leaf {
        static constexpr int k = 1;
      };
    };
  };
};

} // namespace

int use() {
  // Outer/inner template names and every template argument must be renamed
  // inside the generated qualifier, including nested specializations.
  return B<A<X>>::Mid::C<A<X>::Inner>::Leaf::k;
}

} // namespace N

int main() { return N::use(); }
