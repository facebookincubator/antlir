struct Example {
  1: string hello;
  2: binary bin;
  3: map<string, string> kv;
  4: list<string> string_list;
  5: set<string> string_set;
  6: list<ListItem> struct_list;
  7: optional string option_set;
  8: optional string option_unset;
} (rust.exhaustive)

struct ListItem {
  1: string key;
} (rust.exhaustive)
