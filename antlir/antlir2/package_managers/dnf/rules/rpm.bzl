# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl/dnf:reflink.bzl", "REFLINK_FLAVORS", "rpm2extents")

def package_href(nevra: str, id: str) -> str:
    """
    Make the location encode the pkgid. The last path component is the package
    nevra so that dnf logs look nice, but the repo proxy only looks at the
    middle path component that includes the pkgid.
    """
    return "Packages/{id}/{nevra}.rpm".format(id = id, nevra = nevra)

RpmInfo = provider(fields = [
    "extents",  # .rpm transformed by rpm2extents
    "name",  # Name component of NEVRA
    "nevra",  # RPM NEVRA
    "pkgid",  # checksum (sha256 or sha1, usually sha256)
    "raw_rpm",  # .rpm file artifact
    "xml",  # combined xml chunks
])

def _make_xml(ctx: AnalysisContext, rpm: Artifact, href: str) -> Artifact:
    out = ctx.actions.declare_output("xml.json")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.makechunk[RunInfo],
            cmd_args(rpm, format = "--rpm={}"),
            cmd_args(out.as_output(), format = "--out={}"),
            "--href={}".format(href),
        ),
        category = "makexml",
    )
    return out

def _impl(ctx: AnalysisContext) -> list[Provider]:
    if (int(bool(ctx.attrs.sha256)) + int(bool(ctx.attrs.sha1))) != 1:
        fail("exactly one of {sha256,sha1} must be set")

    if ctx.attrs.rpm:
        rpm_file = ctx.attrs.rpm
    else:
        if not ctx.attrs.url:
            fail("'rpm' or 'url' required")
        rpm_file = ctx.actions.declare_output("rpm.rpm")
        ctx.actions.download_file(rpm_file, ctx.attrs.url, sha256 = ctx.attrs.sha256, sha1 = ctx.attrs.sha1)

    # TODO: move nevra directly into attrs.string()
    nevra = "{}-{}:{}-{}.{}".format(
        ctx.attrs.rpm_name,
        ctx.attrs.epoch,
        ctx.attrs.version,
        ctx.attrs.release,
        ctx.attrs.arch,
    )
    pkgid = ctx.attrs.sha256 or ctx.attrs.sha1
    href = package_href(nevra, pkgid)

    xml = ctx.attrs.xml or _make_xml(ctx, rpm_file, href)

    return common_impl(
        ctx = ctx,
        name = ctx.attrs.rpm_name,
        nevra = nevra,
        rpm = rpm_file,
        xml = xml,
        pkgid = ctx.attrs.sha256 or ctx.attrs.sha1,
        reflink_flavors = ctx.attrs.reflink_flavors,
    )

def common_impl(
        ctx: AnalysisContext,
        name: str,
        nevra: str,
        rpm: Artifact,
        xml: Artifact,
        pkgid: str,
        reflink_flavors: dict[str, Dependency]) -> list[Provider]:
    # Produce an rpm2extents artifact for each flavor. This is tied specifically
    # to the version of `rpm` being used in the build appliance, and should be
    # broadly compatible in practice, especially within os versions (eg if we
    # ever end up with centos9-untested in antlir2, it could use the centos9
    # reflink flavor). rpm will reject any version mismatch so the worst that
    # can happen is builds will fail, not be incorrect.
    #
    # Ideally, these reflink artifacts would be constructed on-demand as
    # anonymous targets, but since we don't know the full depgraph of rpms, it's
    # too late at that point, and it's a huge efficiency win to only build them
    # once per reflink flavor that we put it directly onto the provider
    extents = {
        flavor: ctx.actions.declare_output("{}_extents.rpm".format(flavor))
        for flavor in reflink_flavors
    }
    for flavor, appliance in reflink_flavors.items():
        rpm2extents(
            ctx = ctx,
            appliance = appliance,
            rpm = rpm,
            extents = extents[flavor],
            identifier = flavor,
        )
    return [
        DefaultInfo(default_outputs = [rpm], sub_targets = {
            "extents": [DefaultInfo(sub_targets = {
                key: [DefaultInfo(artifact)]
                for key, artifact in extents.items()
            })],
            "xml": [DefaultInfo(xml)],
        }),
        RpmInfo(
            name = name,
            nevra = nevra,
            raw_rpm = rpm,
            pkgid = pkgid,
            xml = xml,
            extents = extents,
        ),
    ]

_rpm = rule(
    impl = _impl,
    attrs = {
        "arch": attrs.string(),
        "epoch": attrs.int(),
        "makechunk": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/package_managers/dnf/rules:makechunk")),
        "reflink_flavors": attrs.dict(attrs.string(), attrs.exec_dep(), default = REFLINK_FLAVORS),
        "release": attrs.string(),
        "rpm": attrs.option(attrs.source(), default = None),
        "rpm_name": attrs.string(),
        "sha1": attrs.option(attrs.string(), default = None),
        "sha256": attrs.option(attrs.string(), default = None),
        "url": attrs.option(attrs.string(), default = None),
        "version": attrs.string(),
        "xml": attrs.option(attrs.source(doc = "all xml chunks"), default = None),
    },
)

rpm = rule_with_default_target_platform(_rpm)
