struct MyStruct {
  1: string url;
  2: string hello;
  3: string newtyped_hello;
  4: i32 number;
  5: Nested nested;
} (rust.exhaustive)

struct Nested {
  1: string uuid;
} (rust.exhaustive)
