typedef binary Path
typedef string Uuid
typedef string UnitName

struct ServiceInstance {
  1: string name;
  2: Uuid version;
  3: Uuid run_uuid;
  4: Paths paths;
  5: UnitName unit_name;
} (rust.exhaustive)

struct Paths {
  1: Path root_source;
  2: Path root;
  3: Path state;
  4: Path cache;
  5: Path logs;
  6: Path runtime;
} (rust.exhaustive)
