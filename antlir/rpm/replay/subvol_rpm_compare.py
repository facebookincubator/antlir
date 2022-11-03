#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

""" See `subvol_rpm_compare` for the entry point. """

import pwd
import re
import shlex
import subprocess
from contextlib import contextmanager
from typing import Iterator, List, NamedTuple, Optional, Set, Tuple

from antlir.common import get_logger
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import new_nspawn_opts, NspawnPluginArgs, PopenArgs
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.nspawn_in_subvol.plugins.repo_plugins import repo_nspawn_plugins

from antlir.rpm.replay.fake_pty_wrapper import fake_pty_cmd, fake_pty_resource
from antlir.rpm.yum_dnf_conf import YumDnf
from antlir.subvol_utils import Subvol, TempSubvolumes

log = get_logger()


class SubvolsToCompare(NamedTuple):
    root: Subvol
    leaf: Subvol
    ba: Subvol
    rpm_installer: YumDnf
    rpm_repo_snapshot: Path  # under `ba`


class NEVRA(NamedTuple):
    name: str
    epoch: str  # it's really an int, but we never convert from string
    version: str
    release: str
    arch: str

    def download_path(self) -> str:
        "Path under `rpm_download_subvol`"
        return f"{self.name}-{self.version}-{self.release}.{self.arch}.rpm"


class RpmDiff(NamedTuple):
    added_in_order: List[NEVRA]
    removed: Set[NEVRA]


def _gen_nevras_in_subvol(ba_subvol: Subvol, subvol: Subvol) -> Iterator[NEVRA]:
    delim = "<:>"
    # This won't tell us the exact install order within a transaction
    # because `rpm` does not record it in the DB (see `dbAdd` in `psm.c`).
    # And installtid + installtime don't have enough granularity.  For this
    # reason, we have to do some extra work elsewhere to get the install
    # order out of `dnf`'s output.
    #
    # This currently doesn't group by `installtid` because TW agent will
    # just lump all RPMs into a single `rpm` command-line. This avoids
    # needing to tell it the transaction boundaries. If we later needed
    # this, we'd just need to grab `installtid` here, and pass it to
    # the agent. Context: https://fburl.com/xqok460n
    opts = new_nspawn_opts(
        # We don't want to nspawn into `subvol` directly since it might have
        # mounts specified in `/.meta`, and it would be a pain to wire those
        # up to be `.bzl` dependencies to make that work.  An alternative
        # would be to add a `skip_meta_mounts` option to `nspawn_in_subvol`,
        # but this ugliness here is more local.
        bindmount_ro=[(subvol.path(), "/i")],
        cmd=[
            "rpm",
            "--root=/i",
            "--query",
            "--all",
            "--queryformat",
            delim.join(
                ("%{" + key + "}")
                for key in ["name", "epochnum", "version", "release", "arch"]
            )
            + "\n",
        ],
        layer=ba_subvol,
    )
    rpm_cp, _ = run_nspawn(opts, PopenArgs(stdout=subprocess.PIPE))
    for nevra_str in rpm_cp.stdout.decode().split():
        n, e, v, r, a = nevra_str.split(delim)
        yield NEVRA(n, e, v, r, a)


def _gen_nevras_from_installer_output(
    rpm_installer: YumDnf,
    stdout: bytes,
    requested_nevras: Set[NEVRA],
) -> Iterator[NEVRA]:
    # `yum` and `dnf` differ in how they format the "progress" part
    installing_re = re.compile(r"^ +(Upgrading|Updating|Installing) +: +([^ ]+) ")
    nvra_re = re.compile(r"^([a-zA-Z0-9._+-]+)-([^-]+)-([^-]+)\.([^.]+)$")
    nevra_re = re.compile(r"^([a-zA-Z0-9._+-]+)-([0-9]+):([^-]+)-([^-]+)\.([^.]+)$")
    envra_re = re.compile(r"^([0-9]+):([a-zA-Z0-9._+-]+)-([^-]+)-([^-]+)\.([^.]+)$")

    just_yielded = None
    installed_nevras = set()
    for line in re.split("[\n\r]", stdout.decode()):
        m = installing_re.match(line)
        if not m:
            continue

        pkg_spec = m.group(2)
        if ":" not in pkg_spec:  # Both `yum` and `dnf` omit epoch if 0
            m = nvra_re.match(pkg_spec)
            assert m, f"Could not parse {rpm_installer} output: {line}"
            nevra = NEVRA(m.group(1), "0", m.group(2), m.group(3), m.group(4))
        elif rpm_installer == YumDnf.dnf:  # NEVRA
            m = nevra_re.match(pkg_spec)
            assert m, f"Could not parse {rpm_installer} output: {line}"
            nevra = NEVRA(*m.groups())
        elif rpm_installer == YumDnf.yum:  # ENVRA
            m = envra_re.match(pkg_spec)
            assert m, f"Could not parse {rpm_installer} output: {line}"
            nevra = NEVRA(m.group(2), m.group(1), m.group(3), m.group(4), m.group(5))
        else:  # pragma: no cover
            raise NotImplementedError(rpm_installer)

        # The installer can print multiple "Installing" lines per NEVRA
        if nevra == just_yielded:
            continue
        just_yielded = nevra

        assert nevra not in installed_nevras, f"{nevra} was installed twice"
        installed_nevras.add(nevra)

        assert nevra in requested_nevras, (
            f"Tried to install {nevra}, which was not added between "
            f"the root subvol, and the final subvol: {requested_nevras}"
        )

        yield nevra

    # An assert above already checked that installed_nevras < requested_nevras.
    assert (
        installed_nevras == requested_nevras
    ), f"{requested_nevras - installed_nevras} were never installed"


def _cmd_to_quoted_bash(cmd) -> str:
    return " ".join(
        c.shell_quote() if isinstance(c, Path) else shlex.quote(c) for c in cmd
    )


def _gen_yum_dnf_install_order(
    *,
    fake_pty: Path,
    subvols: SubvolsToCompare,  # this won't use `leaf` or `root`
    install_subvol: Subvol,
    added_nevras: Set[NEVRA],
    rpm_download_subvol: Subvol,
) -> Iterator[NEVRA]:
    """
    Sort `added_nevras` in the order that `subvols.rpm_installer` from
    `subvols.ba` would install them into `install_subvol`.

    Since there's no "plumbing" API to capture the correct install order
    from `yum` or `dnf`, we determine this order by parsing the installer's
    stdout.  We need `fake_pty` because `dnf` truncates "Installing : "
    lines to 80 characters when the output is not going to a TTY.

    NB: We could optionalize `justdb` and check whether the resulting
    `install_subvol` is "effectively identical" to the original child
    subvolume.  However, this is not a very useful idea since in production
    we use `rpm` to install the downloaded & sorted RPMs.

    TODO: Play with increasing the download parallelism?  On the `dnf` side,
    `max_parallel_downloads`, and can add more repo servers in the BA.
    """
    prog_name = subvols.rpm_installer.value
    # Future(per @malmond): Provide a custom `yum/dnf.conf` to avoid the
    # fact that `--setopt` is known to be buggy.
    common_cmd_prefix = [
        *fake_pty_cmd(subvols.ba.path(), "/fake_pty.py"),
        subvols.rpm_repo_snapshot / prog_name / "bin" / prog_name,
        "install",
        "--installroot=/i",
        "--assumeyes",
        # Do not install weak deps since we want to order **precisely**
        # the packages that actually got installed between the "root"
        # and "destination" subvol -- and that installation could easily
        # have avoided installing some of the weak dependencies.
        "--setopt=install_weak_deps=False",
    ]
    # Unfortunately, `dnf install --setopt=tsflags=justdb` downloads the
    # *.rpm files even if it will not need them. So, we have to pay the
    # RPM download cost, whether or not we want to use a particular file
    # as part of packaging this layer.
    #
    # This explicit download step makes sure that the RPM files are fetched
    # to a location we control, making them available "almost for free".
    # This is slightly more expensive than a single `dnf install` call,
    # since we pay startup & depsolving twice (~1 sec).
    #
    # If we didn't do this two-step dance, and just used `keepcache`,
    # we would be at the mercy of the yum / dnf cache layout, which is
    # both messier, and likely more fragile.
    download_cmd = common_cmd_prefix + [
        "--downloadonly",
        "--downloaddir=/d",
        *(f"{r.name}-{r.epoch}:{r.version}-{r.release}.{r.arch}" for r in added_nevras),
    ]
    install_cmd = common_cmd_prefix + [
        # Avoid the IO of actually unpacking the RPMs
        "--setopt=tsflags=justdb",
        # `dnf` (but not `yum`) has a horrendous bug, wherein doing this
        # here sequence of "install --downloadonly" and "install
        # /downloaddir/*.rpm", with the SAME `--installroot`, will result in
        # all the content of `/downloaddir` being deleted.  This avoids it.
        "--setopt=keepcache=True",
        # NB: The last word should NOT be quoted, and is therefore added below.
    ]
    opts = new_nspawn_opts(
        bindmount_ro=[(fake_pty, "/fake_pty.py")],
        bindmount_rw=[
            (install_subvol.path(), "/i"),
            (rpm_download_subvol.path(), "/d"),
        ],
        user=pwd.getpwnam("root"),
        cmd=[
            "/bin/bash",
            "-uec",
            f"""
set -o pipefail
{_cmd_to_quoted_bash(download_cmd)}
{_cmd_to_quoted_bash(install_cmd)} /d/*.rpm
""",
        ],
        layer=subvols.ba,
    )
    res, _ = run_nspawn(
        opts,
        PopenArgs(stdout=subprocess.PIPE),
        plugins=repo_nspawn_plugins(
            opts=opts,
            plugin_args=NspawnPluginArgs(
                serve_rpm_snapshots=[subvols.rpm_repo_snapshot],
                shadow_proxied_binaries=False,  # Just serve the 1 snapshot
            ),
        ),
    )
    yield from _gen_nevras_from_installer_output(
        subvols.rpm_installer,
        res.stdout,
        added_nevras,
    )


def subvol_rpm_compare(
    *,
    subvols: SubvolsToCompare,
    # If you want the downloaded RPMs, use `subvol_rpm_compare_and_download()`.
    #
    # If this subvol is set, populate it with the downloaded RPMs corresponding
    # to `RpmDiff.added_nevras` -- each file named `NEVRA.download_path`.
    rpm_download_subvol: Optional[Subvol] = None,
) -> RpmDiff:
    """
    Finds what RPMs were added / removed between `.root` and `.leaf`.

    Then, use `.ba` to determine that precise installation order that would
    be used by `.rpm_installer` to install the added NEVRAs from
    `.rpm_repo_snapshot`.

    It **should** true that `RpmDiff.added_in_order` can be `rpm --install`ed
    into `.root` in order to reproduce `.leaf`.

    IMPORTANT: this function exercises `yum` / `dnf` in a way that is
    necessarily somewhat different from how `subvols.leaf` was actually
    constructed.  Therefore, it is important to verify that installing
    `RpmDiff.added_in_order` in `subvols.root` will produce the same output.
    Therefore, typical usage of this function should be followed by using
    the `rpm_diff` module, e.g.  `replay_rpms_and_compiler_items` followed
    by `subvol_diff`.

    Future: Eventually, `RpmActionItem` ought to become self-aware enough to
    record precisely which RPMs installed, in which order -- and perhaps we
    can even switch its actual install method to `rpm -i` for full
    consistency with prod.  At that point, this function should be able to
    use that authoritative changelog instead, only falling back to the
    current "best effort" method when a `genrule_layer` installs RPMs by
    other means.
    """
    root_nevras = set(_gen_nevras_in_subvol(subvols.ba, subvols.root))
    my_nevras = set(_gen_nevras_in_subvol(subvols.ba, subvols.leaf))
    removed_nevras = root_nevras - my_nevras
    added_nevras = my_nevras - root_nevras
    # The "sort & download" step is fairly expensive (~25s) even when
    # there are no RPMs to sort. Instead of debugging why this is,
    # just short-circuit it.
    if not added_nevras:
        return RpmDiff(removed=removed_nevras, added_in_order=[])

    # Shell out to `yum` or `dnf` in the BA to find the correct install
    # order for the new RPMs. Per the comment on P410145489, this matters.
    #
    # `fake_pty` is a separate binary because handling PTY signals in the
    # same process would be insanity, and I don't want to risk `fork()` in a
    # process that's liable to have random FB infra threads.
    with fake_pty_resource() as fake_pty, TempSubvolumes() as tmp_subvols:
        if not rpm_download_subvol:
            rpm_download_subvol = tmp_subvols.create("rpm_compare_download")
        added_in_order = list(
            _gen_yum_dnf_install_order(
                fake_pty=fake_pty,
                subvols=subvols,
                install_subvol=tmp_subvols.snapshot(subvols.root, "subvol_rpm_compare"),
                added_nevras=added_nevras,
                rpm_download_subvol=rpm_download_subvol,
            ),
        )
        # Check that the set of downloaded RPMs is exactly what we requested
        actual_downloaded = {f"{p}" for p in rpm_download_subvol.path().listdir()}
        expected_downloaded = {r.download_path() for r in added_in_order}
        assert expected_downloaded == actual_downloaded, (
            expected_downloaded,
            actual_downloaded,
        )
        return RpmDiff(removed=removed_nevras, added_in_order=added_in_order)


@contextmanager
def subvol_rpm_compare_and_download(
    subvols: SubvolsToCompare,
) -> Iterator[Tuple[RpmDiff, Subvol]]:
    """
    Runs `subvol_rpm_compare` and yields the resulting `RpmDiff` together
    with a temporary subvolume that contains all the added RPM files,
    accessible via `NEVRA.download_path()`.
    """
    with TempSubvolumes() as tmp_subvols:
        rpm_download_subvol = tmp_subvols.create("subvol_rpm_compare_download")
        rd = subvol_rpm_compare(
            subvols=subvols,
            rpm_download_subvol=rpm_download_subvol,
        )
        yield rd, rpm_download_subvol
