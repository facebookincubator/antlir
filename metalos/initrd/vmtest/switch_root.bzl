# @noautodeps

load("//antlir/bzl:oss_shim.bzl", "third_party")
load("//antlir/vm/bzl:constants.bzl", "VM_GUEST_MAC_ADDRESS")
load("//antlir/vm/bzl:defs.bzl", "vm")

def switch_root_test(name, kernel, disk = ":metalos-gpt-image", disk_interface = "virtio-blk", images_sidecar = False):
    vm.rust_unittest(
        name = name,
        vm_opts = vm.types.opts.new(
            initrd = ":switch-root-initrd.cpio.gz",
            kernel = kernel,
            append = [
                "rd.systemd.journald.forward_to_console=1",
                "macaddress={}".format(VM_GUEST_MAC_ADDRESS),
            ],
            runtime = vm.types.runtime.new(
                sidecar_services = ["$(exe :images-sidecar) $(location :image_packages)"] if images_sidecar else [],
            ),
            disk = vm.types.disk.new(
                package = disk,
                interface = disk_interface,
                subvol = "volume",
            ),
        ),
        timeout_secs = 600,
        srcs = ["test_switch_root.rs"],
        deps = ["//metalos/lib/systemd:systemd"] + third_party.libraries(
            [
                "anyhow",
                "nix",
                "slog",
                "slog_glog_fmt",
                "tokio",
            ],
            platform = "rust",
        ),
        crate_root = "test_switch_root.rs",
    )
