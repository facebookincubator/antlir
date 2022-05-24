#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import textwrap
import unittest

from ..yum_dnf_conf import YumDnf, YumDnfConfParser, YumDnfConfRepo


# This is the base class for two test classes at the bottom of the file.
class YumDnfConfTestCaseImpl:
    def setUp(self) -> None:
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `maxDiff`.
        self.maxDiff = 12345

        conf_str = io.StringIO(
            textwrap.dedent(
                """\
        # Unfortunately, comments are discarded by ConfigParser, but I don't
        # want to depend on `ConfigObj` or `iniparse` for this.
        [main]
        debuglevel=2
        gpgcheck=1
        metadata_expire = 1  # seconds
        sslcacert =/some/file

        [potato]
        sslclientkey = /another/file
        baseurl=file:///pot.at/to
        enabled=1

        [oleander]
        baseurl=http://example.com/oleander
        gpgkey=https://example.com/zupa, https://example.com/list/comma
        \thttps://example.com/super/safe https://example.com/list/space
        enabled=1
        """
            )
        )
        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `conf`.
        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `_YUM_DNF`.
        self.conf = YumDnfConfParser(self._YUM_DNF, conf_str)

    def test_gen_repos(self) -> None:
        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `assertEqual`.
        self.assertEqual(
            [
                # pyre-fixme[6]: For 3rd param expected `Tuple[str]` but got `Tuple[]`.
                YumDnfConfRepo("potato", "file:///pot.at/to", ()),
                YumDnfConfRepo(
                    name="oleander",
                    base_url="http://example.com/oleander",
                    # pyre-fixme[6]: For 3rd param expected `Tuple[str]` but got
                    #  `Tuple[str, str, str, str]`.
                    gpg_key_urls=(
                        "https://example.com/zupa",
                        "https://example.com/list/comma",
                        "https://example.com/super/safe",
                        "https://example.com/list/space",
                    ),
                ),
            ],
            # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `conf`.
            list(self.conf.gen_repos()),
        )

    def test_isolate_repos(self) -> None:
        isolated_repos = [
            YumDnfConfRepo(
                name="potato",
                base_url="https://example.com/potato",
                # pyre-fixme[6]: For 3rd param expected `Tuple[str]` but got
                #  `Tuple[str, str]`.
                gpg_key_urls=(
                    "file:///much/secure/so/hack_proof",
                    "https://cat",
                ),
            )
        ]
        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `assertRaisesRegex`.
        with self.assertRaisesRegex(AssertionError, "Failed to isolate "):
            # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `conf`.
            self.conf.isolate().isolate_repos(isolated_repos)
        isolated_repos.append(
            YumDnfConfRepo(
                name="oleander",
                base_url="https://zupa.example.com/sup",
                # pyre-fixme[6]: For 3rd param expected `Tuple[str]` but got `Tuple[]`.
                gpg_key_urls=(),
            )
        )

        out = io.StringIO()
        self.conf.isolate().isolate_repos(isolated_repos).isolate_main(
            config_path="/config_path",
            pluginconf_dir="/pluginconf_dir",
            cache_dir="/cache_dir",
        ).write(out)

        extra_directives = ""
        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `_YUM_DNF`.
        if self._YUM_DNF == YumDnf.yum:
            extra_directives = textwrap.dedent(
                """\
                skip_missing_names_on_install = 0
                skip_missing_names_on_update = 0
            """
            )

        # pyre-fixme[16]: `YumDnfConfTestCaseImpl` has no attribute `assertEqual`.
        self.assertEqual(
            textwrap.dedent(
                """\
        [main]
        debuglevel = 2
        gpgcheck = 1
        metadata_expire = never
        cachedir = /cache_dir
        check_config_file_age = 0
        persistdir = /var/lib/{prog_name}
        reposdir = /dev/null
        logfile = /var/log/{prog_name}.log
        config_file_path = /config_path
        timeout = 60
        localpkg_gpgcheck = 0
        plugins = 1
        pluginconfpath = /pluginconf_dir
        varsdir = /dev/null
        usercache = 0
        syslog_device =\x20
        bugtracker_url =\x20
        fssnap_devices = !*
        {extra_directives}\

        [potato]
        baseurl = https://example.com/potato
        enabled = 1
        gpgkey = file:///much/secure/so/hack_proof
        \thttps://cat

        [oleander]
        baseurl = https://zupa.example.com/sup
        gpgkey =\x20
        enabled = 1

        """
            ).format(
                prog_name={
                    # This is deliberately verbose, replacing
                    # `self._YUM_DNF.value` The idea is to assert that the enum
                    # values matter.
                    YumDnf.yum: "yum",
                    YumDnf.dnf: "dnf",
                }[self._YUM_DNF],
                extra_directives=extra_directives,
            ),
            out.getvalue(),
        )


class YumConfTestCase(YumDnfConfTestCaseImpl, unittest.TestCase):
    # pyre-fixme[4]: Attribute must be annotated.
    _YUM_DNF = YumDnf.yum


class DnfConfTestCase(YumDnfConfTestCaseImpl, unittest.TestCase):
    # pyre-fixme[4]: Attribute must be annotated.
    _YUM_DNF = YumDnf.dnf
