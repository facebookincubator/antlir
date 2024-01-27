# antlir2_facts

Facts can represent basic filesystem properties like:

- path /foo exists
- path /foo is a directory
- path /foo is owned by root:root
- path /foo/bar is a regular file
- path /foo/bar is executable

Or higher level semantics like:

- user 'foo' exists
- user 'foo' has uid 100
- user 'foo' is a member of 'wheel'
- rpm 'bar' is installed

Or even higher level facts that impact the runtime like:

- systemd unit foo.service exists
- systemd unit foo.service depends on bar.service
- systemd unit foo.service is enabled for multi-user.target
- systemd default.target is multi-user.target

Any of these facts can be used by an image feature to assert some safety
properties about the image, and guarantee that features are compiled into the
image in the correct order.

Take this `feature.user_add` as an example

```
feature.user_add(
    name = "foo",
    primary_group = "foo",
    shell = "/bin/bash",
    home_dir = "/home/foo",
)
```

- No user `foo` can exist already
- Group `foo` must exist
- `/bin/bash` must be a regular file
- `/bin/bash` must be executable
- `/home/foo` must exist
