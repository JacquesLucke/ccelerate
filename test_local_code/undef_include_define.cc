#define DEFINE_A 1
#undef DEFINE_A
#define DEFINE_A 10

#include "after_redefine.hh"

int main() { return value(); }
