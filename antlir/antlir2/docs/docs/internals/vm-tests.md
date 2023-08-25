---
sidebar_position: 1
---

# Antlir2 VM Tests

Antlir2 comes with VM framework for testing images or any services within the images. This is a complement to unit test and container image test that enables more system level testing, like booting, initrd, etc.

## Improvements over Antlir1 VM
Antlir1 also comes with VM tests. Antlir2 has overhauled the test framework.

Notable benefits for VM test owners includes:
* Use of modern containers for better resource isolation, with its own container image instead of inheriting the host file system.
* Use of virtiofsd for file sharing with better performance
* All common benefits of antlir2, including faster builds and better cached artifacts

For developers, there are additional benefits:
* Antlir2 VM is written in Rust, and thus it's safer to iterate
* Buck2 elimated a lot of hacks in antlir1, like cleaner dependency tracking
* Data types are decoupled from buck, which makes it easier wrap a VM standalone
* Enables multi-arch testing (still WIP)

## For Test Users

VM itself and VM tests are presented as a normal buck2 target. For example, one can run the default VM with the following command and it will open a shell through ssh inside VM.
```
$ buck2 run //antlir/antlir2/antlir2_vm:default-nondisk-boot
```

Similarly, one can run the example tests. It will execute the test inside VM and report back results.
```
$ buck2 test //antlir/antlir2/antlir2_vm/tests:rust-test
```

The test itself is a normal test written in any supported languages, except that it will be executed inside the specified VM when created with VM test macros. See the [Test Developer](#for-test-developers) section for more details on the test target description.

### Useful Sub Targets
Both VM and test targets come with a few sub targets that enable interactive debugging.

You can get an ssh shell into the test VM through `[shell]` sub target. This is mostly equivalent to `buck2 run` the `vm_host` attribute specified in the test target, with additional benefit of having all relevant environmental variables for the test set in the ssh shell.
```
$ buck2 run //antlir/antlir2/antlir2_vm/tests:rust-test[shell]
```

If you want a console instead of ssh shell, use the `[console]` sub target. This also prints console output to screen.

```
$ buck2 run //antlir/antlir2/antlir2_vm/tests:rust-test[console]
$ buck2 run //antlir/antlir2/antlir2_vm:default-nondisk-boot[console]
```

If you want to inspect the VM related artifacts, `buck2 build <target> --show-output` should show you a bash script similar to what `buck2 test` or `buck2 run` would execute. Just be aware that buck2 doesn't execute the script, but the commands inside directly with more arguments potentially appended. If you want to know the exact command buck executed, you can run the desired buck command and then `buck2 log what-ran` or `buck2 log what-failed` should show you the exact command executed. This could be helpful when you want to run the test inside the VM shell.

### Logging

By default, the logging level is `info`. It only prints basic information like VM is booting, or any errors. To enable more verbose logging, you can use `RUST_LOG=debug` or even `trace` level. More syntax for `RUST_LOG` can be found at [tracing crate doc](https://docs.rs/tracing/latest/tracing/). Note that virtiofsd is rather spammy on `debug` level and thus it's hard-coded to a lower level. If you really want its log, you can set `RUST_LOG=virtiofsd=debug`.

Non-console interactive debugging sub targets will also capture console output into a temporary file and print out the path to the console output. The file is accessible the host system and thus you can tail it in a different terminal. We also have more [internal integration](fb/vm-tests.md#more-internal-debugging-tips) for console logs when tests are run.

## For Test Developers

### Write the tests

As mentioned already, a test is just a normal test and can be written in whatever language supported by the test framework. The difference comes when we specify the test target in buck.

For example, the example test target looks like this.
```
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

The `vm.rust_test` is one of the VM test rules provided by `antlir2/antlir2_vm/bzl:defs.bzl`. It wraps normal test macros to specify a VM target that the test will be executed in. Other than the last `vm_host` field, they are standard test attributes. The test will also do what standard tests might do, like listing tests first before executing each individually. The optional `env` will be passed through into the VM, so your test will have access to them.

The `vm_host` field specifies the VM host target to execute the test in. `get_vm()` is a function provided by `antlir2/antlir2_vm/bzl:preconfigured.bzl` for you to select from a list of pre-configured VMs, if you can find one that satisfies your need.

### Build a custom VM for your test (optional)

The core of the VM test is the VM. If the default MetalOS based VM fits your need, you can use the pre-configured target. More likely though, you want to customize your VM, whether for hardware configuration or root disk. We provide relevant API for each.

The default example VM is in `antlir2/antlir2_vm/TARGETS` and can be stripped down to the following.
```
vm.host(
    name = "default-disk-boot",
    disks = [simple_disk.default_boot_disk],
)
```
`vm.host` is again a rule provided by `antlir2/antlir2_vm/bzl:defs.bzl`. The main non-optional field is `name` and `disks`. Or if the VM doesn't boot from a disk, one can specify `initrd` and `kernel`, instead of a bootable disk. You can also customize CPU count, NIC count and memory size. More parameters are documented in the bzl file.

The disk is likely the most interesing part for the VM. Currently, we only provide MetalOS based artifacts for one to use, but there is no restriction for what disk imagg one can use, so long as it's a valid image file. `antlir2/antlir2_vm/bzl/disk.bzl` provides API to wrap your disk image target into `DiskInfo` for the `disks` field. `create_disk_from_package` takes the image target while `create_empty_disk` creates an empty scratch disk for testing.

Moving on the image, MetalOS provides helper functions for them as well. `metalos/vm/disks/defs.bzl` contains main functions to start from any antlir2 layer, to a partition, to a disk image and make it bootable. `metalos/vm/disks/simple.bzl` uses these API to provide the default disk used above and also serves as an example.

Various folders inside `metalos/vm/` provides targets for initrd, kernel, bootloader, etc that one can use to complete the construction from layer to disk image. The goal is to provide anyone with an antlir2 image layer all the tools needed to create a MetalOS rootfs disk. It can be a bootable disk or can be combined with MetalOS kernel and initrd to boot the VM.

## For VM Developers

This section is generally not useful for test users or developers, but people interested in developing the VM framework itself. All code resides in `antlir2/antlir2_vm/` folder and `antlir2/antlir2_vm:antlir2_vm` is the buck target manages the VM process. For now, only Linux on x86_64 is supported and it uses qemu underneath.

### High Level

`antlir2_vm` binary has a few commands. `test`, `test-debug` and `isolate` only differs in the action it would take after VM boots. They all create an ephemeral container and respawn itself within the container with the `run` command. The container image is located at `antlir2/antlir2_vm/antlir2_vm:container-image`. Currently we use systemd-nspawn container, but it could change in the future.

The `run` command is the core part manages the VM inside the container and it takes three sets of parameters. `--machine-spec` captures hardware and boot configuration, like CPU count, memory, disk image location, etc. `--runtime-spec` describes location any runtime binary required by the VM itself, like `qemu-img` and `qemu-system-*`. `VMArgs` specifies execution related details, like where output goes and what command to run.

In theory, if one can pack all artifacts and rewrite relevant paths in `--machine-spec` and `--runtime-spec`, the VM should be able to run standalone independent of buck.

### Debugging

`RUST_LOG=debug` should print all information needed for debugging the VM framework itself. In addition, `[container]` allows us to inspect the container outside the VM. One can modify the container image target to install necessary tools for local investigation.
```
$ buck2 run //antlir/antlir2/antlir2_vm:default-nondisk-boot[container]
```
