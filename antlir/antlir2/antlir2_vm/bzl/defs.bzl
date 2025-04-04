# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/linux/vm/console:defs.bzl", "TTY_NAME")
load(":run_command.bzl", "vm_run_command")
load(":test.bzl", "vm_cpp_test", "vm_python_test", "vm_rust_test", "vm_sh_test")
load(":types.bzl", "DiskInfo", "VMHostInfo")

def _machine_json(ctx: AnalysisContext) -> (Artifact, typing.Any):
    """Generate json file that describes VM hardware and setup"""

    # We restrict the ways of booting the VM to prevent accidentally
    # booting from unintended source. Allowed combinations are:
    # 1) Exactly one bootable disk. No kernel or initrd is passed in.
    # 2) No bootable disk. Both kernel and initrd are passed in.
    if bool(ctx.attrs.kernel) != bool(ctx.attrs.initrd):
        fail("To boot from initrd, both kernel and initrd are required.")

    boot_disks = [d for d in ctx.attrs.disks if d[DiskInfo].bootable]
    if boot_disks and ctx.attrs.initrd:
        fail(
            "Ambiguous boot requirement with both boot disk and initrd. Either \
            remove initrd and kernel attributes to boot from disk, or remove all \
            bootable disks to boot from initrd.",
        )

    if not boot_disks and not ctx.attrs.initrd:
        fail("No bootable media. Pass in either a bootable disk, or initrd and kernel")

    if len(boot_disks) > 1:
        fail("Ambiguous boot requirement with more than one bootable disk.")
    elif len(boot_disks) == 1:
        # VM assumes first disk is boot disk
        disks = [boot_disks[0]] + [d for d in ctx.attrs.disks if not d[DiskInfo].bootable]
    else:
        disks = ctx.attrs.disks

    # Format the tty name
    append = ctx.attrs.append or ""
    append = append.format(tty = ctx.attrs.tty_name)

    machine_json = ctx.actions.declare_output("machine.json")
    machine_json_args = ctx.actions.write_json(
        machine_json,
        {
            "arch": ctx.attrs.arch,
            "cpus": ctx.attrs.cpus,
            "disks": [d[DiskInfo] for d in disks],
            "max_combined_channels": ctx.attrs.max_combined_channels,
            "mem_mib": ctx.attrs.mem_mib,
            "mount_platform": ctx.attrs.mount_platform,
            "non_disk_boot_opts": {
                "append": append,
                "initrd": ctx.attrs.initrd,
                "kernel": ctx.attrs.kernel,
            } if ctx.attrs.initrd else None,
            "num_nics": ctx.attrs.num_nics,
            "serial_index": ctx.attrs.serial_index,
            "sidecar_services": ctx.attrs.sidecar_services,
            "use_legacy_share": ctx.attrs.use_legacy_share,
            "use_tpm": ctx.attrs.use_tpm,
        },
        with_inputs = True,
    )
    return machine_json, machine_json_args

def _impl(ctx: AnalysisContext) -> list[Provider]:
    """Create the json specs used as input for VM target."""
    machine_json, machine_json_args = _machine_json(ctx)
    run_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_exec[RunInfo]),
        "isolate",
        cmd_args(ensure_single_output(ctx.attrs.image), format = "--image={}"),
        cmd_args(machine_json_args, format = "--machine-spec={}"),
    )
    if ctx.attrs.timeout_secs:
        run_cmd = cmd_args(
            run_cmd,
            cmd_args(str(ctx.attrs.timeout_secs), format = "--timeout-secs={}"),
        )

    run_script, _ = ctx.actions.write(
        "run.sh",
        cmd_args("#!/bin/bash", cmd_args(run_cmd, delimiter = " \\\n  "), "\n"),
        is_executable = True,
        allow_args = True,
    )
    return [
        DefaultInfo(
            default_output = run_script,
            sub_targets = {
                "console": [DefaultInfo(run_script), RunInfo(cmd_args(run_cmd, "--console"))],
                "container": [DefaultInfo(run_script), RunInfo(cmd_args(run_cmd, "--container"))],
                "machine_json": [DefaultInfo(machine_json)],
            },
        ),
        RunInfo(run_cmd),
        VMHostInfo(
            vm_exec = ctx.attrs.vm_exec,
            image = ctx.attrs.image,
            machine_spec = machine_json_args,
        ),
    ]

_vm_host = rule(
    impl = _impl,
    attrs = {
        # Hardware parameters for the VM
        "arch": attrs.default_only(
            attrs.string(
                default = arch_select(x86_64 = "x86_64", aarch64 = "aarch64"),
            ),
            doc = "ISA of the emulated machine",
        ),
        "cpus": attrs.int(default = 1, doc = "number for CPUs for the VM"),
        "disks": attrs.list(
            attrs.dep(providers = [DiskInfo]),
            doc = "list of disks to attach to VM",
        ),
        # buck target labels
        "labels": attrs.list(attrs.string(), default = []),
        "max_combined_channels": attrs.int(default = 1),
        "mem_mib": attrs.int(default = 4096, doc = "memory size in MiB"),
        "num_nics": attrs.int(default = 1),
        "serial_index": attrs.int(default = 0, doc = "index of the serial port"),
        "tty_name": attrs.default_only(
            attrs.string(default = TTY_NAME),
            doc = "arch dependent name of the console device",
        ),
        "use_legacy_share": attrs.bool(
            default = False,
            doc = "use 9p instead of virtiofs for sharing for older kernels",
        ),
        "use_tpm": attrs.bool(default = False, doc = "enable software TPM"),
    } | {
        # Non-hardware parameters for the VM
        "append": attrs.option(
            attrs.string(),
            default = None,
            doc = "kernel command line parameter when booting from initrd",
        ),
        "initrd": attrs.option(
            attrs.source(),
            default = None,
            doc = "initrd to boot from when not booting from disk",
        ),
        "kernel": attrs.option(
            attrs.source(),
            default = None,
            doc = "kernel image to boot from when not booting from disk",
        ),
        "mount_platform": attrs.bool(
            default = True,
            doc = "Mount runtime platform (aka /usr/local/fbcode) from the host",
        ),
        "sidecar_services": attrs.list(
            attrs.arg(),
            default = [],
            doc = "list of commands to spawn outside VM that VM can communicate with",
        ),
        "timeout_secs": attrs.option(
            attrs.int(),
            default = None,
            doc = "total allowed execution time for the VM",
        ),
    } | {
        # VM runtime. Genearlly shouldn't be overwritten
        "image": attrs.exec_dep(
            default = "antlir//antlir/antlir2/antlir2_vm:container-dir",
            doc = "container image directory to execute the VM inside",
        ),
        "vm_exec": attrs.default_only(
            attrs.exec_dep(
                default = "antlir//antlir/antlir2/antlir2_vm:antlir2_vm",
                doc = "executable that runs VM in isolation",
            ),
        ),
    },
)

vm = struct(
    host = rule_with_default_target_platform(_vm_host),
    cpp_test = vm_cpp_test,
    python_test = vm_python_test,
    rust_test = vm_rust_test,
    sh_test = vm_sh_test,
    run_command = vm_run_command,

    # Various pre-built targets useful for building VM or writing tests
    artifacts = struct(
        # Pre-built VMs for `vm_host` of tests
        default_vms = struct(
            # initrd_boot is recommended for faster boot performance
            initrd_boot = "antlir//metalos/vm:default-initrd-boot",
            # disk boots are recommended for more real boot sequence
            disk_boot = "antlir//metalos/vm:default-disk-boot",
            nvme_disk_boot = "antlir//metalos/vm:default-nvme-disk-boot",
        ),
        rootfs = struct(
            # Base layer to start from when customizing VM rootfs layer
            layer = "antlir//metalos/vm/os:rootfs",
            # Features to add onto an existing layer to make it work for VM
            virtualization_features = "antlir//metalos/vm/os:virtualization-features",
        ),
    ),
)
