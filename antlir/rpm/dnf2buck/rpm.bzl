# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

nevra = record(
    name = str.type,
    epoch = int.type,
    version = str.type,
    release = str.type,
    arch = str.type,
)

def nevra_to_string(nevra: nevra.type) -> str.type:
    return "{}-{}:{}-{}.{}".format(
        nevra.name,
        nevra.epoch,
        nevra.version,
        nevra.release,
        nevra.arch,
    )

def package_href(nevra: nevra.type, id: str.type) -> str.type:
    """
    Make the location encode the pkgid. The last path component is the package
    nevra so that dnf logs look nice, but the repo proxy only looks at the
    middle path component that includes the pkgid.
    """
    return "Packages/{id}/{nevra}.rpm".format(id = id, nevra = nevra_to_string(nevra))

RpmInfo = provider(fields = {
    "nevra": "RPM NEVRA",
    "pkgid": "checksum (sha256 or sha1, usually sha256)",
    "rpm": ".rpm file artifact",
    "xml_filelists": "filelists.xml chunk artifact",
    "xml_other": "other.xml chunk artifact",
    "xml_primary": "primary.xml chunk artifact",
})

def _make_chunk(ctx: "context", rpm: "artifact", which: str.type, href: str.type) -> "artifact":
    out = ctx.actions.declare_output(which + ".xml")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.makechunk[RunInfo],
            cmd_args(rpm, format = "--rpm={}"),
            cmd_args(out.as_output(), format = "--out={}"),
            "--chunk={}".format(which),
            "--href={}".format(href),
        ),
        category = "makechunk",
        identifier = which,
    )
    return out

def _impl(ctx: "context") -> ["provider"]:
    if (int(bool(ctx.attrs.sha256)) + int(bool(ctx.attrs.sha1))) != 1:
        fail("exactly one of {sha256,sha1} must be set")

    if ctx.attrs.rpm:
        rpm_file = ctx.attrs.rpm
    else:
        if not ctx.attrs.url:
            fail("'rpm' or 'url' required")
        rpm_file = ctx.actions.declare_output("rpm")
        ctx.actions.download_file(rpm_file, ctx.attrs.url, sha256 = ctx.attrs.sha256, sha1 = ctx.attrs.sha1)

    pkg_nevra = nevra(
        name = ctx.attrs.rpm_name,
        epoch = ctx.attrs.epoch,
        version = ctx.attrs.version,
        release = ctx.attrs.release,
        arch = ctx.attrs.arch,
    )
    href = package_href(pkg_nevra, ctx.attrs.sha256)

    if ctx.attrs.xml_primary:
        xml_primary = ctx.attrs.xml_primary
    else:
        xml_primary = _make_chunk(ctx, rpm_file, "primary", href)
    if ctx.attrs.xml_filelists:
        xml_filelists = ctx.attrs.xml_filelists
    else:
        xml_filelists = _make_chunk(ctx, rpm_file, "filelists", href)
    if ctx.attrs.xml_primary:
        xml_other = ctx.attrs.xml_other
    else:
        xml_other = _make_chunk(ctx, rpm_file, "other", href)

    return [
        DefaultInfo(default_outputs = [], sub_targets = {
            name: [DefaultInfo(default_outputs = [artifact])]
            for name, artifact in {
                "filelists.xml": xml_filelists,
                "other.xml": xml_other,
                "primary.xml": xml_primary,
            }.items()
        }),
        RpmInfo(
            nevra = pkg_nevra,
            rpm = rpm_file,
            pkgid = ctx.attrs.sha256 or ctx.attrs.sha1,
            xml_primary = xml_primary,
            xml_filelists = xml_filelists,
            xml_other = xml_other,
        ),
    ]

rpm = rule(
    impl = _impl,
    attrs = {
        "arch": attrs.string(),
        "epoch": attrs.int(),
        "makechunk": attrs.default_only(attrs.exec_dep(default = "//antlir/rpm/dnf2buck:makechunk")),
        "release": attrs.string(),
        "rpm": attrs.option(attrs.source(), default = None),
        "rpm_name": attrs.string(),
        "sha1": attrs.option(attrs.string(), default = None),
        "sha256": attrs.option(attrs.string(), default = None),
        "url": attrs.option(attrs.string(), default = None),
        "version": attrs.string(),
        "xml_filelists": attrs.option(attrs.source(doc = "filelists.xml chunk"), default = None),
        "xml_other": attrs.option(attrs.source(doc = "other.xml chunk"), default = None),
        "xml_primary": attrs.option(attrs.source(doc = "primary.xml chunk"), default = None),
    },
)
