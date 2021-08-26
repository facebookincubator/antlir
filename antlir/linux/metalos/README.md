MetalOS
=======


systemd targets
---------------
The MetalOS initrd handles mounting local filesystems and preparing a
per-boot snapshot of an image. Once the MetalOS rootfs is switch-rooted into,
there are some provided synchronization points for systemd services:

- `core-services.target` should be used for any widely-used services that
workloads may depend on, but are not directly related to the workload themselves

- `workload-pre.target` should be used for any workload-specific setup. It
is ordered `After=core-services.target`

- `workload.target` should be used for the service(s) that constitute the host's
actual workload. It is ordered `After=workload-pre.target`

WARNING: The well known systemd synchronization points described in
`man 7 bootup` should be used only with extreme caution in a MetalOS rootfs
image, as most with have already been activated by the initrd, and will not be
automatically restarted upon switch-rooting into the root fs.
