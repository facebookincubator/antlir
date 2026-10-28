# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Hermetic RPM installs (opt-in, off by default): every rpm feature's dnf
# transaction runs under Hermit (https://github.com/facebookexperimental/hermit),
# so time, randomness and thread scheduling inside dnf, rpm and their scriptlets
# are deterministic, and the RPM database, dnf history and scriptlet outputs are
# reproducible with package state kept.
#
# Enable it with the `antlir//antlir/antlir2/cfg:hermetic_rpm[enabled]` modifier,
# for a package with set_cfg_modifiers or for one build with `--modifier`.
# `-c antlir2.source_date_epoch=<seconds>` must then give the commit time of the
# revision being built, e.g. `$(sl log -r . -T '{date|hgdate}' | cut -d' ' -f1)`.
# It starts Hermit's virtual clock and so becomes the install time recorded in
# the RPM database. It is the same setting fixed_metadata cpio packages use.

# The flags are fixed so every hermetic build of a revision runs the same way:
# - host-no-rdrand: the guest sees the host's CPU features (CentOS Stream 10's
#   glibc needs x86-64-v3) with RDRAND/RDSEED hidden; requires CPUID faulting.
# - max-timeslice=disabled and no-rcb-time: no PMU-driven preemption and a
#   virtual clock that does not depend on retired-branch counts, so neither the
#   schedule nor the recorded times depend on the host's PMU behaviour.
_HERMIT_RUN_FLAGS = [
    "--cpu-profile=host-no-rdrand",
    "--max-timeslice=disabled",
    "--no-rcb-time",
    "--seed=1",
]

def _pad(n: int, width: int) -> str:
    s = str(n)
    return "0" * (width - len(s)) + s

def _rfc3339(seconds: int) -> str:
    """UTC time `seconds` after the epoch (civil_from_days, H. Hinnant)."""
    days = seconds // 86400
    rem = seconds % 86400
    z = days + 719468
    era = z // 146097
    doe = z - era * 146097
    yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
    doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
    mp = (5 * doy + 2) // 153
    day = doy - (153 * mp + 2) // 5 + 1
    month = mp + 3 if mp < 10 else mp - 9
    year = yoe + era * 400 + (1 if month <= 2 else 0)
    return "{}-{}-{}T{}:{}:{}Z".format(
        _pad(year, 4),
        _pad(month, 2),
        _pad(day, 2),
        _pad(rem // 3600, 2),
        _pad(rem % 3600 // 60, 2),
        _pad(rem % 60, 2),
    )

def _hermetic_driver_impl(ctx: AnalysisContext) -> list[Provider]:
    if ctx.attrs.hermit == None:
        fail("hermetic RPM installs need a Hermit build, which is not available in this build")
    if ctx.attrs.source_date_epoch == None or ctx.attrs.source_date_epoch <= 0:
        fail(
            "hermetic RPM installs need -c antlir2.source_date_epoch=<seconds since the epoch>, "
            + "the commit time of the revision being built; got {}".format(ctx.attrs.source_date_epoch),
        )
    epoch = _rfc3339(ctx.attrs.source_date_epoch)

    # Copied into the Buck output directory: the appliance sees these paths.
    wrapper = ctx.actions.copy_file("hermetic_driver.sh", ctx.attrs.wrapper)
    # RunInfo, not the bare executable: it carries the shared libraries the
    # distro build of Hermit links against.
    hermit = ctx.attrs.hermit[RunInfo]
    return [
        DefaultInfo(),
        RunInfo(
            args = cmd_args(
                "/bin/sh",
                wrapper,
                hermit,
                str(ctx.attrs.source_date_epoch),
                "--epoch=" + epoch,
                _HERMIT_RUN_FLAGS,
                "--",
                ctx.attrs.driver[RunInfo],
            ),
        ),
    ]

hermetic_driver = rule(
    impl = _hermetic_driver_impl,
    attrs = {
        "driver": attrs.dep(providers = [RunInfo]),
        # Hermit runs inside the build appliance, so it is built for the
        # image's distro, like the other binaries Antlir runs there, with the
        # plain driver for the RPMs that build installs.
        "hermit": attrs.option(
            attrs.transition_dep(
                cfg = "antlir//antlir/distro/transition:to-current-distro-platform-without-hermetic-rpm",
                providers = [RunInfo],
            ),
            default = None,
        ),
        # Start of Hermit's virtual clock, i.e. the install time recorded in
        # the RPM database, in seconds since the epoch. Required when this
        # driver is selected; checked at analysis time.
        "source_date_epoch": attrs.option(attrs.int(), default = None),
        "wrapper": attrs.source(),
    },
)
