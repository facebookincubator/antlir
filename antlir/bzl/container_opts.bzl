load(":rpm_repo_snapshot.bzl", "snapshot_install_dir")
load(":shape.bzl", "shape")

# Forward container runtime configuration to the Python implementation.
# This currently maps to `NspawnPluginArgs`.
#
# Prefer to keep this default-initializable to avoid having to update a
# bunch of tests and other Python callsites.
container_opts_t = shape.shape(
    shadow_proxied_binaries = shape.field(bool, default = False),
    serve_rpm_snapshots = shape.list(str, default = []),
)

def make_container_opts(
        # List of target or /__antlir__ paths, see `snapshot_install_dir` doc.
        serve_rpm_snapshots = (),
        **kwargs):
    return shape.new(
        container_opts_t,
        serve_rpm_snapshots = [
            snapshot_install_dir(s)
            for s in serve_rpm_snapshots
        ],
        **kwargs
    )
