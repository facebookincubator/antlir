---
id: disk-booting
title: Disk Booting
---

# Disk boot artifacts

MetalOS ships with an EFI bootloader based on
[systemd-stub](https://www.freedesktop.org/software/systemd/man/systemd-stub.html).
This bootloader is composed of a small initrd (`//metalos/bootloader:initrd`)
that is built using a standard production kernel and the required disk boot
driver modules.

The bootloader binary is part of the host's `BootConfig` and is installed by the
imaging initrd during imaging, and updated by the rootfs as part of the standard
offline update implementation for rootfs+kernel upgrades.

# Disk boot process

Within the EFI bootloader:
1. Mount the root partition (`LABEL=/`) at `/run/fs/control`
2. Load the current MetalOS host config from disk
3. Kexec into the host kernel and disk-boot initrd

Within the disk boot initrd:
4. Create unique per-boot rootfs snapshot
5. Evaluate rootfs generators and apply to the new snapshot
6. Switch-root into the rootfs

## Offline update process

Bootloader is not involved in offline updates, but the same code prepares the
btrfs subvolumes and kexecs into the host kernel (steps 3-5)

# Why?

This single bootloader artifact is completely static and fully-testable at
build-time.
The disk bootloader itself has no moving pieces, as the imaging initrd and/or
rootfs will ensure that the necessary rootfs and kernel images are already on
disk.

Additionally, there is no need to manage bootloader entries in an external
system like sd-boot or grub, and the host is _entirely_ described by the MetalOS
host config.

## Attestation implications

UEFI will measure this single disk bootloader EFI binary into PCR4. Any
necessary TPM history must be preserved by MetalOS across kexec boundaries for
both disk boots and kexec-based upgrades at runtime.
