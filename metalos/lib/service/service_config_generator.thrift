struct Input {
  1: string kernel_version;
} (rust.exhaustive)

struct Output {
  1: optional Dropin dropin;
} (rust.exhaustive)

// Maybe this should allow overriding any unit file settings, but for now let's
// just keep it to a small subset, and any "complicated" unit settings must go
// into the static .service files that we can more easily audit to understand in
// the later stages of the MVP
struct Dropin {
  1: map<string, string> environment;
} (rust.exhaustive)
