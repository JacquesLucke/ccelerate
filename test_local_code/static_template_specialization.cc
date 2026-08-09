template<typename T>
static int compute(const T &v);

template<>
int compute<int>(const int &v)
{
  return v + 1;
}

template<>
int compute<double>(const double &v)
{
  return static_cast<int>(v);
}

static int run(int x, double y)
{
  return compute(x) + compute(y);
}

int main()
{
  return run(1, 2.0);
}
