# Image Tests

One of the key features of `antlir2` is the ability to write standard unit tests
that execute inside your image.

## Types

There is first-class support for all the major types of unit tests:

- `image_rust_test` (equivalent: `rust_unittest`)
- `image_python_test` (equivalent: `python_unittest`)
- `image_cpp_test` (equivalent: `cpp_unittest`)
- `image_sh_test` (equivalent: `sh_unittest`)

The api for using each of these is the same as the non-image equivalent, with
the addition of one required attribute, `layer`, which is the layer in which the
test should be run.

```python title="my/team/BUCK"
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_rust_test")

image.layer(
    name = "layer",
    features = [
        # It probably goes without saying, but images do need some base level of
        # "OS-ness" in order to run a test inside them. Most images install
        # specific RPMs that they need, but for an otherwise empty image, we
        # must install at least `basesystem`
        features.rpms_install(rpms = ["basesystem"]),
    ],
)

image_rust_test(
    name = "test",
    srcs = ["test.rs"],
    layer = ":layer"
)
```

That's all that's required to run a simple unit test inside of a container.

There are a number of more advanced features when it comes to running image
tests, mainly booted containers.

## Booted Containers

If your container normally runs a full `init` (aka `systemd`) as PID 1, you
probably want your test environment to mimic that.

This is also useful if you are writing tests for libraries that need to interact
with `systemd` and you don't want to mess with your host's PID 1 (for obvious
reasons).

For the following examples, we assume that our `BUCK`(/`TARGETS`) file starts
with this content

```python title="my/team/BUCK"
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_rust_test")

image.layer(
    name = "layer",
    features = [
        # Duh, if we're booting systemd, we need to install it
        features.rpms_install(rpms = ["systemd"]),
    ],
)
```

### Simplest use

If you just want to boot systemd and don't care about anything in particular,
simply adding `boot=True` to your test will do that.

```python title="my/team/BUCK"
image_rust_test(
    name = "test",
    srcs = ["test.rs"],
    layer = ":layer",
    boot = True,
)
```

Your test will run as a systemd unit, with these dependencies

```systemd title="systemd configuration"
[Unit]
DefaultDependencies=no
Requires=sysinit.target
After=sysinit.target basic.target
Wants=default.target
```

Basically, the only guarantee is that `systemd` itself is up and running with
the most basic amount of functionality.

:::warning

This setup is good enough for test that barely care about other units (which is
most tests!), but the container system might be quite unhealthy by the time the
test runs - even `basic.target` might not have been reached!

:::

### Explicit dependencies

You can fully specify the dependencies (in other words, when your test will
start to run) if the defaults above are unsuitable.

```python title="my/team/BUCK"
image_rust_test(
    name = "test",
    srcs = ["test.rs"],
    layer = ":layer",
    boot = True,
    boot_after_units = ["foo.service"],
    boot_requires_units = ["foo.service"],
    boot_wants_units = ["bar.service"],
)
```

Your test will run as a systemd unit with these dependencies

```
[Unit]
DefaultDependencies=no
Requires=foo.service
After=foo.service
Wants=bar.service
```

### Wait for the whole system

`default.target` is available for use in any of the above attributes if you want
your test to wait for the system as configured the layer to fully come up.

:::warning

This is obviously the slowest mode of operation. Your tests will have a lot of
extra, most likely unnecessary overhead if you do this. Prefer to set
[explicit dependencies](#explicit-dependencies) to exactly what you need.

:::

## Debugging

What are tests for if they never fail? Inevitably, you will need to debug your
test.

The ergonomics of this debugging are improving rapidly, so these docs will be
updated continuously.

The most obvious thing you'll want is a shell in your container:

```sh
buck2 run //my/team:test[container]
```

This will drop you into a root shell in your container where you can poke
around. You'll also be greeted with a short message with more information about
how to effectively use the environment.
