---
sidebar_position: 1
---

# Antlir2 VM Tests

Antlir2 comes with VM framework for testing images or any services within the
images. This is a complement to unit test and container image test that enables
more system level testing, like booting, initrd, etc.

## Improvements over Antlir1 VM

Antlir1 also comes with VM tests. Antlir2 has overhauled the test framework.

Notable benefits for VM test owners includes:

- Use of modern containers for better resource isolation, with its own container
  image instead of inheriting the host file system.
- Use of virtiofsd for file sharing with better performance
- All common benefits of antlir2, including faster builds and better cached
  artifacts

For developers, there are additional benefits:

- Antlir2 VM is written in Rust, and thus it's safer to iterate
- Buck2 elimated a lot of hacks in antlir1, like cleaner dependency tracking
- Data types are decoupled from buck, which makes it easier wrap a VM standalone
- Enables multi-arch testing (still WIP)

## For Test Users

VM itself and VM tests are presented as a normal buck2 target. For example, one
can run the default VM with the following command and it will open a shell
through ssh inside VM.

```
$ buck2 run //antlir/antlir2/antlir2_vm:default-nondisk-boot
2023-09-12T18:43:03.624505Z  INFO antlir2_vm::vm: Booting VM. It could take seconds to minutes...
2023-09-12T18:43:03.725167Z  INFO antlir2_vm::vm: Note: console output is redirected to /tmp/.tmpcgsDxD/console.txt
2023-09-12T18:43:39.881741Z  INFO antlir2_vm::vm: Received boot event READY after 36.256695 seconds
[root@vmtest ~]#
```

Similarly, one can run the example tests. It will execute the test inside VM and
report back results.

```
$ buck2 test //antlir/antlir2/antlir2_vm/tests:rust-test
<test output just like a normal tests>
```

The test itself is a normal test written in any supported languages, except that
it will be executed inside the specified VM when created with VM test macros.
See the [Test Developer](#for-test-developers) section for more details on the
test target description.

### Useful Sub Targets

Both VM and test targets come with a few sub targets that enable interactive
debugging.

You can get an ssh shell into the test VM through `[shell]` sub target. This is
mostly equivalent to `buck2 run` the `vm_host` attribute specified in the test
target, with additional benefit of having all relevant environmental variables
for the test set in the ssh shell.

```
$ buck2 run //antlir/antlir2/antlir2_vm/tests:rust-test[shell]
```

If you want a console instead of ssh shell, use the `[console]` sub target. This
also prints console output to screen.

```
$ buck2 run //antlir/antlir2/antlir2_vm/tests:rust-test[console]
$ buck2 run //antlir/antlir2/antlir2_vm:default-nondisk-boot[console]
```

If you want to inspect the VM related artifacts,
`buck2 build <target> --show-output` should show you a bash script similar to
what `buck2 test` or `buck2 run` would execute. Just be aware that buck2 doesn't
execute the script, but the commands inside directly with more arguments
potentially appended.

If you want to know the exact command buck executed, you can run the desired
`buck test` command first and then `buck2 log what-ran` or
`buck2 log what-failed` should show you the exact command executed. This could
be helpful when you want to run the test inside the VM shell.

### Logging

By default, the logging level is `info`. It only prints basic information like
VM is booting, or any errors. To enable more verbose logging, you can use
`RUST_LOG=debug` or even `trace` level. More syntax for `RUST_LOG` can be found
at [tracing crate doc](https://docs.rs/tracing/latest/tracing/). Note that
virtiofsd is rather spammy on `debug` level and thus it's hard-coded to a lower
level. If you really want its log, you can set `RUST_LOG=virtiofsd=debug`.

Non-console interactive debugging sub targets will also capture console output
into a temporary file and print out the path to the console output. The file is
accessible the host system and thus you can tail it in a different terminal. We
also have more
[internal integration](fb/vm-tests.md#more-internal-debugging-tips) for console
logs when tests are run.

### Debugging Tips

One additional failure mode in VM test compared to normal tests is failure from
the VM itself. This can be caused by a non-booting VM, or bad parameters for
starting the VM. Either way, it will show up as a "FATAL" test result, because
our VM test framework will exit with a non-zero status. You won't find normal
test output because the test binary isn't invoked at all due to the failed VM.

For bad parameter when starting the VM, it should show up at the same place
where normal test output would show up. You can `buck2 run` the `[shell]` or
`[console]` sub target to produce it, optionally prepend `RUST_LOG=debug` for
more details. This is generally not expected for test users, as it can only
happen when core VM test framework is broken, which is owned by antlir team.

For a non-booting VM, `[console]` sub target on the test should show you the
full console log in realtime and drop you inside an emergency shell for
debugging if available. This can happen if the VM setup changes (bootloader,
initrd, kernel, rootfs, etc). This is either owned by the test owner for custom
setup, or if using common image artifacts, the image owner.

## For Test Developers

### Write the tests

As mentioned already, a test is just a normal test and can be written in
whatever language supported by the test framework. The difference comes when we
specify the test target in buck.

For example, the example test target looks like this.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//antlir/antlir2/antlir2_vm/bzl:preconfigured.bzl", "get_vm")

vm.rust_test(
    name = "rust-test",
    srcs = ["test.rs"],
    crate = "test_rs",
    crate_root = "test.rs",
    env = {
        "ANTLIR2_TEST": "1",
    },
    vm_host = get_vm(),  # vm specific
)
```

The `vm.rust_test` is one of the VM test rules provided by
`antlir2/antlir2_vm/bzl:defs.bzl`. It wraps normal test macros to specify a VM
target that the test will be executed in. Other than the last `vm_host` field,
they are standard test attributes. The test will also do what standard tests
might do, like listing tests first before executing each individually. The
optional `env` will be passed through into the VM, so your test will have access
to them.

The `vm_host` field specifies the VM host target to execute the test in.
`get_vm()` is a function provided by `antlir2/antlir2_vm/bzl:preconfigured.bzl`
for you to select from a list of pre-configured VMs, if you can find one that
satisfies your need.

### Build a custom VM for your test (optional)

The core of the VM test is the VM. If the default MetalOS based VM fits your
need, you can use the pre-configured target. More likely though, you want to
customize your VM, whether for hardware configuration or root disk. We provide
relevant API for each.

The default example VM is in `antlir2/antlir2_vm/TARGETS` and can be stripped
down to the following for a VM boots from disk.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//metalos/vm/disks:simple.bzl", "simple_disk")

vm.host(
    name = "default-disk-boot",
    disks = [simple_disk.default_boot_disk],
)
```

`vm.host` is again a rule provided by `antlir2/antlir2_vm/bzl:defs.bzl`. The
main non-optional field is `name` and `disks`. You can customize CPU count, NIC
count and memory size. More parameters are documented in the bzl file.

The VM doesn't have to boot from a disk, and one can specify `initrd` and
`kernel`, instead of a bootable disk. This is recommended if you want the
fastest boot time. An example is also provided in `antlir2/antlir2_vm/TARGETS`.
Note the change in `disks` and additional `initrd` and `kernel` fields.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//metalos/kernel/bzl:defs.bzl", "metalos_kernel")
load("//metalos/vm/disks:simple.bzl", "simple_disk")
load("//metalos/vm/initrd:defs.bzl", "initrd")

vm.host(
    name = "default-nondisk-boot",
    disks = [simple_disk.default_control_disk],
    initrd = initrd.default,
    kernel = metalos_kernel.default.vmlinuz,
)
```

The disk is likely the most interesting part for the VM. Currently, we only
provide MetalOS based artifacts for one to use, but there is no restriction for
what disk image one can use, so long as it's a valid image file.
`antlir2/antlir2_vm/bzl/disk.bzl` provides API to wrap your disk image target
into `DiskInfo` for the `disks` field. `create_disk_from_package` takes the
image target while `create_empty_disk` creates an empty scratch disk for
testing. A few hardware related properties like `interface` and
`logical_block_size` can be specified when creating the disk.

Moving on the image, MetalOS provides helper functions for them as well.
`metalos/vm/disks/defs.bzl` contains main functions to start from any antlir2
layer, to a partition, to a disk image and make it bootable.
`metalos/vm/disks/simple.bzl` uses these API to provide the default disk used
above and also serves as an example.

Various folders inside `metalos/vm/` provides targets for initrd, kernel,
bootloader, etc that one can use to complete the construction from layer to disk
image. The goal is to provide anyone with an antlir2 image layer all the tools
needed to create a MetalOS rootfs disk. It can be a bootable disk or can be
combined with MetalOS kernel and initrd to boot the VM.

### Customizing the kernel

One common need is to change the kernel used by tests. A list of supported
kernels are in `metalos/vm/kernels/versions.bzl`. This will eventually replace
antlir1 kernel types, but due to technical reasons they are unfortunately
separate and have to co-exist for now. The kernel artifact is the same even
though you might see a different kernel target.

All APIs under `metalos/vm/*/defs.bzl` supports one or multiple `get*()`
function that takes `arch` and kernel `uname`. So long as the arch and uname
combination in the `versions.bzl`, the target can be used. For example, this
turns the default VM into a different kernel.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//antlir/antlir2/antlir2_vm/bzl:simple.bzl", "simple_disk")

vm.host(
    name = "disk-boot-5.19",
    compatible_with = ["ovr_config//cpu:x86_64"],
    disks = [simple_disk.get_boot_disk(
        arch = "x86_64",
        interface = "virtio-blk",
        uname = "5.19",
    )],
)
```

If you want a non-disk boot VM with metalos bits, it's a bit more verbose.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//antlir/antlir2/antlir2_vm/bzl:simple.bzl", "simple_disk")
load("//metalos/kernel/bzl:defs.bzl", "metalos_kernel")
load("//metalos/vm/initrd:defs.bzl", "initrd")

vm.host(
    name = "nondisk-boot-5.19",
    compatible_with = ["ovr_config//cpu:x86_64"],
    disks = [simple_disk.get_control_disk(
        arch = "x86_64",
        interface = "virtio-blk",
        uname = "5.19",
    )],
    initrd = initrd.get("x86_64", "5.19"),
    kernel = metalos_kernel.get("x86_64", "5.19").vmlinuz,
)
```

Some of the details would change when we unify the kernel type later, and that
means the parameters we use to fill the fields could change. Of course they can
be customized to whatever buck target you want as well. In addition,
`compatible_with` might need to be set given the image content is arch specific.
This might look awkward now, but in the future when multi-arch support is fully
ready, the various `get*()` will return `select()` to avoid the need of setting
`compatible_with` or passing in `arch` entirely.

### Migrating from Antlir1 VM test

For internal users, migration will be done for you. The goal of the section is
mostly for developers familiar with Antlir1 VM to quickly understand the general
changes.

A somewhat complicated antlir1 VM test could look like this.

```
load("//antlir/vm/bzl:defs.bzl", "vm")
load("//metalos/disk:disk.bzl", "disk")

vm.rust_unittest(
    name = "test",
    srcs = ["test.rs"],
    crate_root = "test.rs",
    timeout_secs = 600,
    vm_opts = vm.types.opts.new(
        boot_from_disk = True,
        disks = [
            vm.types.disk.from_package(
                interface = "virtio-blk",
                package = ":some-disk",
            ),
            vm.types.disk.scratch(
                interface = "nvme",
                size_mb = 1024,
            ),
        ],
    ),
    deps = [
        "something",
    ],
)
```

When migrated to antlir2, it will look like this.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//antlir/antlir2/antlir2_vm/bzl:disk.bzl", "disk")

vm.host(
    name = "test-vm",
    disks = [
        disk.create_disk_from_package(
            name = "test-boot-disk",
            bootable = True,
            interface = "virtio-blk",
            image = ":some-disk",
        ),
        disk.create_empty_disk(
            name = "test-scratch-disk",
            interface = "nvme",
            size_mib = 1024,
        ),
    ],
)

vm.rust_test(
    name = "test",
    srcs = ["test.rs"],
    crate_root = "test.rs",
    timeout_secs = 600,
    vm_host = ":test-vm",
    deps = [
        "something",
    ],
)
```

The general pattern is that whatever used to belong to `vm_opts` in antlir1 VM
tests, are now separated to the VM target that the test will refer to. The
upside is that the VM target can be easily reused for multiple tests. The
downside is that if the VM is only used once, it's more verbose.

The disk API also changes, but the general parameter for creating each disk is
similar. Instead of specifying `boot_from_disk`, in antlir2 VM, mark the
bootdisk `bootable` instead.

Another common pattern are for VMs that don't really care about underlying
image. The owner just wants to run the test inside some VM.

```
load("//antlir/vm/bzl:defs.bzl", "vm")

vm.python_unittest(
    name = "test",
    srcs = ["test.py"],
    vm_opts = vm.types.opts.new(),
)
```

In these cases, instead of splitting it out, check the
`antlir2/antlir2_vm/bzl:preconfigured.bzl` to see if such a VM exists already.
If so, refer to it directly.

```
load("//antlir/antlir2/antlir2_vm/bzl:defs.bzl", "vm")
load("//antlir/antlir2/antlir2_vm/bzl:preconfigured.bzl", "get_vm")

vm.python_test(
    name = "test",
    srcs = ["test.py"],
    # "nondisk-boot" can be omitted as it's the default
    vm_host = get_vm("nondisk-boot"),
)
```

## For VM Developers

This section is generally not useful for test users or developers, but people
interested in developing the VM framework itself. All code resides in
`antlir2/antlir2_vm/` folder and `antlir2/antlir2_vm:antlir2_vm` is the buck
target manages the VM process. For now, only Linux on x86_64 is supported and it
uses qemu underneath.

### High Level

`antlir2_vm` binary has a few commands. `test`, `test-debug` and `isolate` only
differs in the action it would take after VM boots. They all create an ephemeral
container and respawn itself within the container with the `run` command. The
container image is located at `antlir2/antlir2_vm/antlir2_vm:container-image`.
Currently we use systemd-nspawn container, but it could change in the future.

The `run` command is the core part manages the VM inside the container and it
takes three sets of parameters. `--machine-spec` captures hardware and boot
configuration, like CPU count, memory, disk image location, etc.
`--runtime-spec` describes location any runtime binary required by the VM
itself, like `qemu-img` and `qemu-system-*`. `VMArgs` specifies execution
related details, like where output goes and what command to run.

In theory, if one can pack all artifacts and rewrite relevant paths in
`--machine-spec` and `--runtime-spec`, the VM should be able to run standalone
independent of buck.

When invoked through buck, these parameters are filled in by buck rules.
`antlir2/antlir2_vm/bzl/defs.bzl` defines the rules for VM host itself. It also
provides `[machine_json]` and `[runtime_json]` sub targets so that one can
easily inspect the generated config. (Note that the actual artifacts may not
exist unless you've run `buck2 run` or `buck2 test`, as buck2 is very good at
avoiding unnecessary work.) `antlir2/antlir2_vm/bzl/test.bzl` defines rules and
helpers for various types of tests. It takes the VM host as an input with
additional parameters for tests. Generally, the test rules should not be used
directly, but instead use the wrapped unittest macros.

### Debugging

`RUST_LOG=debug` should print all information needed for debugging the VM
framework itself. In addition, `[container]` allows us to inspect the container
outside the VM. One can modify the container image target
(`antlir2/antlir2_vm:container-image`) to install necessary tools for local
investigation inside the container. This is mostly useful for investigating
`sidecar_services` that run outside VM.

```
$ buck2 run //antlir/antlir2/antlir2_vm:default-nondisk-boot[container]
```

The VM will continue to boot in the background and you will still have access to
the redirected console log just like other interactive debugging sub targets.
However, you won't get a shell inside VM unless it boots and you ssh into the VM
from the container shell.
