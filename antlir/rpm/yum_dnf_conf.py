#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""
Since `yum` and `dnf` configs are extremely similar in core syntax and
function, we use the same code to handle both.

Support for `dnf` config isolation is less complete, since we made no
systematic effort to find ways in which `dnf` affects the host system.  The
reason for this was that from now on, both package managers are always
expected to run in read-only or ephemeral build appliance images, never on
an actual host.
"""

import re
from configparser import ConfigParser
from enum import Enum
from typing import Iterable, Iterator, NamedTuple, TextIO, Tuple


MAX_PARALLEL_DOWNLOADS = 16

# NB: The 'main' section in `{yum,dnf}.conf` acts similarly to
# ConfigParser's magic 'DEFAULT', in that it provides default values for
# some of the repo options.  I did not investigate this in enough detail to
# say that setting `default_section='main'` would be appropriate.  Since
# this code currently only cares about `baseurl`, this is good enough.
_NON_REPO_SECTIONS = ["DEFAULT", "main"]


class YumDnf(Enum):
    yum = "yum"
    dnf = "dnf"


class YumDnfConfRepo(NamedTuple):
    name: str
    base_url: str
    gpg_key_urls: Tuple[str]

    @classmethod
    def from_config_section(cls, name, cfg_sec) -> "YumDnfConfRepo":
        assert "/" not in name and "\0" not in name, f"Bad repo name {name}"
        return YumDnfConfRepo(
            name=name,
            base_url=cfg_sec["baseurl"],
            # pyre-fixme[6]: For 3rd param expected `Tuple[str]` but got
            #  `Union[Tuple[], typing.Tuple[typing.Any, ...]]`.
            gpg_key_urls=tuple(re.findall(r"[^\n\s,]+", cfg_sec["gpgkey"]))
            if "gpgkey" in cfg_sec
            else (),
        )


def _isolate_ssl_options(cfg) -> None:
    # We don't actually need the SSL options, because we serve everything
    # over HTTP from the local `repo-server`, so they shouldn't affect
    # anything.  However, `yum` prints this annoying logspam when the
    # snapshot's original `yum.conf` contains `sslcacert`, and when that
    # cert is no longer included in the image with the snapshot:
    #     Repo EACH_REPO forced skip_if_unavailable=True due to BAD_CA_FILE
    # So, let's eliminate references to non-default files that may not exist
    # (this leaves `sslverify` alone since it's not a file):
    for ssl_opt in ["sslcacert", "sslclientcert", "sslclientkey"]:
        cfg.pop(ssl_opt, None)


class YumDnfConfIsolator:
    """
    The functions in this class ATTEMPT to edit `{yum,dnf}.conf` in such a
    way that the package manager will:
      - never interact with state, caches, or configuration from the host
        filesystem,
      - never interact with servers outside of the ones we specify.

    As per the file-docblock note, `dnf` isolation is likely incomplete.

    IMPORTANT: With `yum`, it is actually impossible to configure it such
    that it does not touch the host filesystem.  A couple of specific
    examples:

    (1) Regardless of the configuration, `yum` will look in
        `$host_root/$cachedir` BEFORE `$installroot/$cachedir`, which is
        breaks isolation of RPM content and repodata.

    (2) Regardless of the configuration, `yum` will attempt to read
        `/etc/yum/vars` from the host, breaking isolation of configuration.

    There are other examples. To see the bind-mount protections we use to
    avoid leakage from the host, read `_isolate_yum_dnf_and_wait_until_ready` --
    and of course, the larger purpose of `yum-dnf-from-snapshot` is to run
    its `yum` or `dnf` inside a private network namespace to guarantee no
    off-host repo accesses.
    """

    def __init__(self, yum_dnf: YumDnf, cp: ConfigParser) -> None:
        self._yum_dnf = yum_dnf
        self._cp = ConfigParser()
        self._cp.read_dict(cp)  # Make a copy
        self._isolated_main = False
        self._isolated_repos = False

    def isolate_repos(
        self, repos: Iterable[YumDnfConfRepo]
    ) -> "YumDnfConfIsolator":
        """
        Asserts that the passed repos are exactly those defined in the
        config file. This ensures that we leave no repo unisolated.

        For each specified repo, sets the config values specified in its
        `YumDnfConfRepo`, and clears `proxy`.  Other config keys are left
        unchanged -- but seeing some "known bad" configs in the config file
        will cause an assertion error.

        IMPORTANT: See the class docblock, this is not **ENOUGH**.
        """
        unchanged_repos = {r for r in self._cp if r not in _NON_REPO_SECTIONS}
        for repo in repos:
            unchanged_repos.remove(repo.name)
            assert repo.name not in _NON_REPO_SECTIONS
            repo_sec = self._cp[repo.name]
            repo_sec["baseurl"] = (
                "\n".join(repo.base_url)
                if isinstance(repo.base_url, list)
                else repo.base_url
            )
            repo_sec["gpgkey"] = "\n".join(repo.gpg_key_urls)
            repo_sec.pop("proxy", None)  # We talk only to a local reposerver.
            # These are not handled for now, but could be supported. The
            # goal of asserting their absence is to avoid accidentally
            # having non-isolated URLs in the config.
            for unsupported_key in [
                "include",
                "metalink",
                "mirrorlist",
                "gpgcakey",
            ]:
                assert unsupported_key not in repo_sec, (unsupported_key, repo)
            _isolate_ssl_options(repo_sec)
        assert not unchanged_repos, f"Failed to isolate {unchanged_repos}"
        self._isolated_repos = True
        return self

    def isolate_main(
        self, *, config_path: str, pluginconf_dir: str, cache_dir: str
    ) -> "YumDnfConfIsolator":
        """
        Set keys that could cause `yum` or `dnf` to interact with the host
        filesystem.  IMPORTANT: See the class docblock, this is not ENOUGH.
        """
        prog_name = self._yum_dnf.value
        main_sec = self._cp["main"]
        assert (
            "include" not in main_sec and "include" not in self._cp["DEFAULT"]
        ), "Includes are not supported"

        # Since we have an immutable snapshot of the repos, we can pre-build
        # the cache as part of the snapshot, and it never expires.
        main_sec["cachedir"] = cache_dir
        main_sec["metadata_expire"] = "never"
        main_sec["check_config_file_age"] = "0"

        # This list was obtained by scrolling through `man yum.conf`.  To be
        # really thorough, we'd also remove glob filesystem dependencies
        # from options like `exclude`, `includepkgs`, `protected_packages`,
        # `exactarchlist`, etc -- but this is a moot point now that all RPM
        # installs go trough a build appliance.
        #
        # `persistdir` is under `--installroot`, so no isolation needed.
        # However, ensuring defaults makes later container customization
        # (e.g.  cleanup) easier.  These can be optionalized later if a good
        # reason arises.
        main_sec["persistdir"] = f"/var/lib/{prog_name}"  # default
        # Specify repos only via this `.conf` -- that eases isolating them.
        main_sec["reposdir"] = "/dev/null"
        # See the note about `persistdir` -- the same logic applies.
        main_sec["logfile"] = f"/var/log/{prog_name}.log"  # default
        main_sec["config_file_path"] = config_path
        # Having `dnf` download with high concurrency from the FB-internal
        # repo snapshot storage results in a speedup of over 2x.  This is
        # because concurrency masks significant per-blob setup overheads.
        main_sec["max_parallel_downloads"] = str(MAX_PARALLEL_DOWNLOADS)
        # CI hosts can experience resource starvation and slowdowns.  We
        # never expect to see timeouts in interactive use, so err on the
        # side of making them higher to improve CI reliability.
        main_sec["timeout"] = "90"
        main_sec.pop("proxy", None)  # We talk only to a local reposerver.

        # This forces that any incoming repo or RPM installation MUST be signed
        # with a key that is trusted (imported) in the destination RPM DB.
        main_sec["gpgcheck"] = "1"
        # FIXME: Temporarily block this out to try to troubleshoot some CI
        # code signing issues.  This isn't introducing a true security
        # problem, in the sense that any local RPM must have made it into
        # the source repo somehow -- but disabling the check does makes it
        # easier to pull in code without double-checking its trust.
        main_sec["localpkg_gpgcheck"] = "0"

        _isolate_ssl_options(main_sec)

        # `yum-dnf-from-snapshot` and friends need certain plugins, and they
        # are off by default in `yum`, so just enable them.  Don't worry,
        # this won't turn on all plugins installed in the BA --
        # `yum-dnf-from-snapshot` hardcodes a list of known-good ones on the
        # command-line.
        main_sec["plugins"] = "1"
        # Provide custom configuration, e.g. to set up version locking for
        # the RPM snapshot that'll use this yum/dnf config.
        main_sec["pluginconfpath"] = pluginconf_dir

        # This option seems to only exist for `dnf`.
        main_sec["varsdir"] = "/dev/null"

        # This final block of options seems only to exist for `yum`.
        #
        # Shouldn't make a difference for as-root runs, but it's good hygiene
        main_sec["usercache"] = "0"
        main_sec["syslog_device"] = ""  # We'll just use `logfile`.
        main_sec["bugtracker_url"] = ""
        main_sec["fssnap_devices"] = "!*"  # Snapshots don't make sense.
        assert not main_sec.get("commands")  # This option seems dodgy.

        # Make yum fail if one package in a list can't be installed.
        if self._yum_dnf == YumDnf.yum:
            main_sec["skip_missing_names_on_install"] = "0"
            main_sec["skip_missing_names_on_update"] = "0"

        self._isolated_main = True
        return self

    def write(self, out: TextIO) -> None:
        "Outputs a `{yum,dnf}.conf` file with the changed configuration."
        assert self._isolated_main and self._isolated_repos
        self._cp.write(out)


class YumDnfConfParser:
    def __init__(self, yum_dnf: YumDnf, conf: TextIO) -> None:
        self._yum_dnf = yum_dnf
        self._cp = ConfigParser()
        self._cp.read_file(conf)

    def gen_repos(self) -> Iterator[YumDnfConfRepo]:
        "Raises if repo names cannot be used as directory names."
        for repo, cfg in self._cp.items():
            if repo not in _NON_REPO_SECTIONS:
                yield YumDnfConfRepo.from_config_section(repo, cfg)

    def isolate(self) -> YumDnfConfIsolator:
        return YumDnfConfIsolator(self._yum_dnf, self._cp)
