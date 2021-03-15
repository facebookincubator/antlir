Ideal state:

`cxx` config in `.buckconfig` has a single `target_toolchain` that points
to a target using a `select` to return the proper toolchain target

The `select` uses constraint_setting/value targets to figure out what the right
toolchain is.

The `exe` macro is built using the host platform so that they can be executed on the host. (Controlled by `parser.host_configuration_switch_enabled` in the `.buckconfig)
There is another `exe_target` that can be used to force the `exe` to be built against the desired target platform

Use `configured_alias` to force a specific target to use the host platform.
