#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import io
import textwrap
import unittest

from ..yum_dnf_conf import YumDnf, YumDnfConfRepo, YumDnfConfParser


# This is the base class for two test classes at the bottom of the file.
class YumDnfConfTestCaseImpl:

    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        conf_str = io.StringIO(textwrap.dedent('''\
        # Unfortunately, comments are discarded by ConfigParser, but I don't
        # want to depend on `ConfigObj` or `iniparse` for this.
        [main]
        debuglevel=2
        gpgcheck=1

        [potato]
        baseurl=file:///pot.at/to
        enabled=1

        [oleander]
        baseurl=http://example.com/oleander
        gpgkey=https://example.com/zupa
        \thttps://example.com/super/safe
        enabled=1
        '''))
        self.conf = YumDnfConfParser(self._YUM_DNF, conf_str)

    def test_gen_repos(self):
        self.assertEqual([
            YumDnfConfRepo('potato', 'file:///pot.at/to', ()),
            YumDnfConfRepo(
                name='oleander',
                base_url='http://example.com/oleander',
                gpg_key_urls=(
                    'https://example.com/zupa',
                    'https://example.com/super/safe',
                ),
            ),
        ], list(self.conf.gen_repos()))

    def test_isolate_repos(self):
        isolated_repos = [YumDnfConfRepo(
            name='potato',
            base_url='https://example.com/potato',
            gpg_key_urls=('file:///much/secure/so/hack_proof', 'https://cat'),
        )]
        with self.assertRaisesRegex(AssertionError, 'Failed to isolate '):
            self.conf.isolate().isolate_repos(isolated_repos)
        isolated_repos.append(YumDnfConfRepo(
            name='oleander',
            base_url='https://zupa.example.com/sup',
            gpg_key_urls=(),
        ))

        out = io.StringIO()
        self.conf.isolate().isolate_repos(isolated_repos).isolate_main(
            install_root='/install_root',
            config_path='/config_path',
            versionlock_dir='/versionlock_dir',
        ).write(out)

        self.assertEqual(textwrap.dedent('''\
        [main]
        debuglevel = 2
        gpgcheck = 1
        cachedir = /var/cache/{prog_name}
        persistdir = /var/lib/{prog_name}
        reposdir = /dev/null
        logfile = /var/log/{prog_name}.log
        installroot = /install_root
        config_file_path = /config_path
        timeout = 60
        plugins = 1
        pluginpath = /versionlock_dir
        pluginconfpath = /versionlock_dir
        varsdir = /dev/null
        usercache = 0
        syslog_device =\x20
        bugtracker_url =\x20
        fssnap_devices = !*

        [potato]
        baseurl = https://example.com/potato
        enabled = 1
        gpgkey = file:///much/secure/so/hack_proof
        \thttps://cat

        [oleander]
        baseurl = https://zupa.example.com/sup
        gpgkey =\x20
        enabled = 1

        '''.format(prog_name={
            # This is deliberately verbose, replacing `self._YUM_DNF.value`
            # The idea is to assert that the enum values matter.
            YumDnf.yum: 'yum',
            YumDnf.dnf: 'dnf',
        }[self._YUM_DNF])), out.getvalue())


class YumConfTestCase(YumDnfConfTestCaseImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.yum


class DnfConfTestCase(YumDnfConfTestCaseImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.dnf
