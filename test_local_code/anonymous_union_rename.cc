namespace demo {
static const int a = 0;
int b = a;
int c = a;

static union {
  void *p;
  char s[8];
} d[2];
char *const e = d[0].s;
char *f = e;

static union {
  void *p;
  char s[8];
} g[2];
char *const h = g[0].s;
char *i = h;
}
int main() { return 0; }
