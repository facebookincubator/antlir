# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:target_helpers.bzl", "antlir_dep")
load(":test.bzl", "vm_cpp_test", "vm_rust_test", "vm_sh_test")
load(":types.bzl", "DiskInfo", "VMHostInfo")

def _machine_json(ctx: AnalysisContext) -> (Artifact, "write_json_cli_args"):
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

    machine_json = ctx.actions.declare_output("machine.json")
    machine_json_args = ctx.actions.write_json(
        machine_json,
        {
            "cpus": ctx.attrs.cpus,
            "disks": [d[DiskInfo] for d in disks],
            "mem_mib": ctx.attrs.mem_mib,
            "non_disk_boot_opts": {
                "append": "",
                "initrd": ctx.attrs.initrd,
                "kernel": ctx.attrs.kernel,
            } if ctx.attrs.initrd else None,
            "num_nics": ctx.attrs.num_nics,
        },
        with_inputs = True,
    )
    return machine_json, machine_json_args

def _runtime_json(ctx: AnalysisContext) -> (Artifact, "write_json_cli_args"):
    """Generate the json file to pass runtime information to the VM"""
    runtime_json = ctx.actions.declare_output("runtime.json")
    runtime_json_args = ctx.actions.write_json(
        runtime_json,
        {
            "firmware": ctx.attrs.firmware,
            "qemu_img": ensure_single_output(ctx.attrs.qemu_img),
            "qemu_system": ensure_single_output(ctx.attrs.qemu_system),
            "roms_dir": ctx.attrs.roms_dir,
        },
        with_inputs = True,
    )
    return runtime_json, runtime_json_args

def _impl(ctx: AnalysisContext) -> list[Provider]:
    """Create the json specs used as input for VM target."""
    machine_json, machine_json_args = _machine_json(ctx)
    runtime_json, runtime_json_args = _runtime_json(ctx)
    run_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_exec[RunInfo]),
        "isolate",
        cmd_args(ctx.attrs.image[LayerInfo].subvol_symlink, format = "--image={}"),
        cmd_args(machine_json_args, format = "--machine-spec={}"),
        cmd_args(runtime_json_args, format = "--runtime-spec={}"),
    )
    if ctx.attrs.timeout_s:
        run_cmd = cmd_args(
            run_cmd,
            cmd_args(str(ctx.attrs.timeout_s), format = "--timeout-s={}"),
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
                "machine_json": [DefaultInfo(machine_json)],
                "runtime_json": [DefaultInfo(runtime_json)],
            },
        ),
        RunInfo(run_cmd),
        VMHostInfo(
            vm_exec = ctx.attrs.vm_exec,
            image = ctx.attrs.image,
            machine_spec = machine_json_args,
            runtime_spec = runtime_json_args,
        ),
    ]

_vm_host = rule(
    impl = _impl,
    attrs = {
        # Hardware parameters for the VM
        "cpus": attrs.int(default = 1, doc = "number for CPUs for the VM"),
        "disks": attrs.list(
            attrs.dep(providers = [DiskInfo]),
            doc = "list of disks to attach to VM",
        ),
        "mem_mib": attrs.int(default = 4096, doc = "memory size in MiB"),
        "num_nics": attrs.int(default = 1),
    } | {
        # Non-hardware parameters for the VM
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
        "timeout_s": attrs.int(
            default = 300,
            doc = "total allowed execution time for the VM",
        ),
    } | {
        # VM runtime. Genearlly shouldn't be overwritten
        "firmware": attrs.default_only(
            attrs.source(default = antlir_dep("vm/runtime:edk2-x86_64-code.fd")),
            doc = "firmware for the VM",
        ),
        "image": attrs.dep(
            providers = [LayerInfo],
            default = antlir_dep("antlir2/antlir2_vm:container-image"),
            doc = "container image to execute the VM inside",
        ),
        "qemu_img": attrs.default_only(
            attrs.exec_dep(default = antlir_dep("vm/runtime:qemu-img")),
            doc = "qemu-img binary for manipulating disk images",
        ),
        "qemu_system": attrs.default_only(
            attrs.exec_dep(default = antlir_dep("vm/runtime:qemu-system-x86_64")),
            doc = "qemu-system binary that should match target arch",
        ),
        "roms_dir": attrs.default_only(
            attrs.source(default = antlir_dep("vm/runtime:roms")),
            doc = "ROMs for the VM",
        ),
        "vm_exec": attrs.default_only(
            attrs.exec_dep(
                default = antlir_dep("antlir2/antlir2_vm:antlir2_vm"),
                doc = "executable that runs VM in isolation",
            ),
        ),
    },
)

vm = struct(
    host = rule_with_default_target_platform(_vm_host),
    cpp_test = vm_cpp_test,
    rust_test = vm_rust_test,
    sh_test = vm_sh_test,
)
