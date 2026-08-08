#define NAME get_value

static int NAME() { return 10; }

#define NAME2_(n) n##2
#define NAME2(n) NAME2_(n)
static int NAME2(NAME)() { return 20; }

float NAME(float a) { return a + 30.0f; }

int main() { return NAME() + NAME2(NAME)() + NAME(10.0f); }