"""
This .bzl provides a macro for installing rust into an image.layer via the
'Standalone Installers' provided via Rust Forge: https://forge.rust-lang.org/infra/other-installation-methods.html#standalone-installers

The reason for this approach vs using rustup is because `rustup` requires
network access to download the desired package from the internet.  Antlir
doesn't allow network access when building an image.layer, so we need to
have the artifact being installed downloaded by buck so it can know about
it.

The installation itself is done via the `install.sh` script provided as
part of the standalone package inside of an `image.genrule`.  This is
not ideal because now we can't really track exactly what is installed
into the image.layer, but it is necessary due to way that rust compiles
the various `.so` and `.rlib` files to include hashes in the filenames.
In short: each version of rust has a unique set of filenames, making it
near impossible to be explicit about which files to include. Furthermore
`image.clone` only works when the source is an `image.layer` and 
`image.install` requires explicit file paths.  So we're stuck with
`image.genrule` blindly mutating the layer with the `install.sh` script.
At least it doesn't try and talk to the network, so we'll take that win.
"""
load("//antlir/bzl:oss_shim.bzl", "buck_genrule", "http_file")
load("//antlir/bzl:image.bzl", "image")

# Update this map for matching the sha256s of various channel/version
# combinations of rust builds that are supported.
# Todo: maybe this should be part of the `config//` cell.
CHANNEL_VERSION_SHA256_MAP = {
    "nightly": {
        "2021-04-20": "89effca4bf6420446cd55ce46c384ad4f8496f7ad6e96108255fbad0d37f036b",
    },
}

def _install_rustc(
    name,
    parent_layer,
    version,
    channel,
    arch=None,
    **kwargs,
):
    arch = arch or "x86_64"

    tarball_name = "rust-{}-{}.tar.gz".format(channel, arch)
    download_name = "rust-{}-{}__download".format(channel, arch)
    http_file(
        name = download_name,
        out = tarball_name,
        sha256 = CHANNEL_VERSION_SHA256_MAP[channel][version],
        urls = [
            "https://static.rust-lang.org/dist/{}/rust-{}-{}-unknown-linux-gnu.tar.gz".format(version, channel, arch),
        ] ,
        visibility = []
    )

   
    image.layer(
        name = name + "__install-rustc-setup",
        parent_layer = parent_layer,
        features = [
            image.install(
                ":{}".format(download_name),
                "/{}".format(tarball_name),
            ),
            image.ensure_subdirs_exist(
                "/", "working",
            ),
        ],
        **kwargs,
    )

    image.genrule_layer(
        name = name + "__install-rustc-work",
        parent_layer = ":" + name + "__install-rustc-setup",
        rule_type = "install_rustc",
        user = "root",
        cmd = ["/bin/bash", "-uec", ";".join([
                "tar --extract --verbose --strip=1 --directory /working --file /{}".format(tarball_name),
                "/working/install.sh --components=rustc,rust-std-{}-unknown-linux-gnu --prefix=/usr".format(arch),
            ]),
        ],
        antlir_rule = "user-internal",
        **kwargs,
    )

    image.layer(
        name = name,
        parent_layer = ":" + name +"__install-rustc-work",
        features = [
            image.remove("/{}".format(tarball_name)),
            image.remove("/working"),
        ],
        **kwargs,
    )

rustc = struct(
    install = _install_rustc,
)
