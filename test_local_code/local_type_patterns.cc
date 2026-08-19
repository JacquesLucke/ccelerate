namespace pattern_local_type {
struct OnlyLocalType {
  int x;
  OnlyLocalType(int value) : x(value) {}
};

OnlyLocalType make_value() { return OnlyLocalType(7); }
} // namespace pattern_local_type

int main() { return pattern_local_type::make_value().x; }
