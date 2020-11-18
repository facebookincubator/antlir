def nevra(*, name, epoch, version, release, arch):
    return struct(_private_envra = [epoch, name, version, release, arch])

image_rpm = struct(
    nevra = nevra,
)
