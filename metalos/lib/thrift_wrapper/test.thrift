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

struct UnionA {
  1: string foo;
} (rust.exhaustive)

struct UnionB {
  1: i32 bar;
} (rust.exhaustive)

union MyUnion {
  1: UnionA a;
  2: UnionB nEw;
}
