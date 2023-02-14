# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:image.bzl", "image")
load("//antlir/bzl/image/feature:defs.bzl", "feature")
load("//antlir/bzl/image/package:btrfs.bzl", "btrfs")
load("//metalos/host_configs/tests:host.bzl", "host_config")
load(":kernel.bzl", "normalize_kernel")

def _host_config(kernel_version):
    return {
        "boot_config": {
            "bootloader": {
                "cmdline": "console=ttyS0,57600 systemd.unified_cgroup_hierarchy=1 selinux=0 cgroup_no_v1=all root=LABEL=/ macaddress=00:00:00:00:00:01",
                "pkg": {
                    "format": 2,
                    "id": {"uuid": "deadbeefdeadbeefdeadbeefdeadbeef"},
                    "kind": 8,
                    "name": "metalos.bootloader",
                },
            },
            "deployment_specific": {"metalos": {}},
            "initrd": {
                "format": 2,
                "id": {"uuid": "deadbeefdeadbeefdeadbeefdeadbeef"},
                "kind": 3,
                "name": "metalos.initrd",
            },
            "kernel": {
                "cmdline": "console=ttyS0,57600 systemd.unified_cgroup_hierarchy=1 selinux=0 cgroup_no_v1=all root=LABEL=/ macaddress=00:00:00:00:00:01",
                "pkg": {
                    "format": 1,
                    # deadbeef is always used as the version, but the package name is changed
                    "id": {"uuid": "deadbeefdeadbeefdeadbeefdeadbeef"},
                    "kind": 2,
                    "name": "kernel." + kernel_version,
                },
            },
            "rootfs": {
                "format": 1,
                "id": {"uuid": "deadbeefdeadbeefdeadbeefdeadbeef"},
                "kind": 1,
                "name": "metalos",
            },
        },
        "provisioning_config": {
            "deployment_specific": {"metalos": {}},
            "event_backend": {
                "base_uri": "http://vmtest-host:8000/send-event",
                "source": {
                    "asset_id": 1,
                },
            },
            "gpt_root_disk": {
                "format": 2,
                "id": {"uuid": "deadbeefdeadbeefdeadbeefdeadbeef"},
                "kind": 7,
                "name": "metalos.gpt-root-disk",
            },
            "identity": {
                "hostname": "vm00.abc00.facebook.com",
                "id": "1",
                "network": {
                    "dns": {
                        "search_domains": [
                            "example.com",
                            "subdomain.example.com",
                        ],
                        "servers": [
                            "beef::cafe:1",
                            "beef::cafe:2",
                        ],
                    },
                    "interfaces": [
                        {
                            "addrs": ["fd00::2"],
                            "essential": True,
                            "interface_type": 0,
                            "mac": "00:00:00:00:00:01",
                            "name": "eth0",
                            "structured_addrs": [
                                {
                                    "addr": "fd00::2",
                                    "mode": 0,
                                    "prefix_length": 64,
                                },
                            ],
                        },
                    ],
                    "primary_interface": {
                        "addrs": ["fd00::2"],
                        "essential": True,
                        "interface_type": 0,
                        "mac": "00:00:00:00:00:01",
                        "name": "eth0",
                        "structured_addrs": [
                            {
                                "addr": "fd00::2",
                                "mode": 0,
                                "prefix_length": 64,
                            },
                        ],
                    },
                },
            },
            "imaging_initrd": {
                "format": 2,
                "id": {"uuid": "deadbeefdeadbeefdeadbeefdeadbeef"},
                "kind": 4,
                "name": "metalos.imaging-initrd",
            },
            "root_disk_config": {
                "single_serial": {
                    "config": {},
                    "serial": "ROOT_DISK_SERIAL",
                },
            },
        },
        "runtime_config": {
            "deployment_specific": {"metalos": {}},
        },
    }

def _control_layer(
        name,
        kernel_version):
    host_config(name = name + "-host-config", host_config = _host_config(kernel_version))
    image.layer(
        name = name,
        features = [
            feature.ensure_dirs_exist("/run/state/metalos"),
            feature.ensure_dirs_exist("/image/initrd"),
            feature.install(
                ":{}-host-config".format(name),
                "/run/state/metalos/metalos_host_configs::host::HostConfig-current.json",
            ),
            # initrd is not actually used, so just install an empty file
            feature.install("//antlir:empty", "/image/initrd/metalos.initrd:deadbeefdeadbeefdeadbeefdeadbeef"),
        ],
        parent_layer = "//metalos/disk:control",
        visibility = [],
    )
    return ":" + name

def control_disk(
        name,
        rootfs,
        kernel,
        free_mb = 2560,  # 2.5G
        visibility = None):
    kernel = normalize_kernel(kernel)
    btrfs.new(
        name = name,
        antlir_rule = "user-internal",
        opts = btrfs.opts.new(
            default_subvol = "/volume",
            free_mb = free_mb,
            loopback_opts = image.opts(
                label = "/",
            ),
            subvols = {
                "/volume": btrfs.opts.subvol.new(
                    layer = _control_layer(name = name + "-control", kernel_version = kernel.uname),
                    writable = True,
                ),
                "/volume/image/kernel/kernel.{}:deadbeefdeadbeefdeadbeefdeadbeef".format(kernel.uname): btrfs.opts.subvol.new(
                    layer = kernel.derived_targets.image,
                ),
                "/volume/image/rootfs/metalos:deadbeefdeadbeefdeadbeefdeadbeef": btrfs.opts.subvol.new(
                    layer = rootfs,
                ),
            },
        ),
        labels = ["vm-root-disk-with-kernel={}".format(kernel.uname)],
        visibility = visibility,
    )
