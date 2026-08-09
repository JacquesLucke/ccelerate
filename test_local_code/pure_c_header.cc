extern "C" {
#include "pure_c_api.h"
}

extern "C" int pure_c_fn(void) { return 0; }

int main() { return pure_c_fn(); }
