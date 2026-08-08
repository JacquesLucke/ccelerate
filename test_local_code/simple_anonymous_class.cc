namespace {
class A {
public:
  int get_value() { return 10; }
};
} // namespace

int main() { return A().get_value(); }