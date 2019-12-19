#!/usr/bin/env python3
import io
import textwrap
import unittest

from ..yum_conf import YumConfRepo, YumConfParser


class YumConfTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

        self.yum_conf = YumConfParser(io.StringIO(textwrap.dedent('''\
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
        ''')))

    def test_gen_repos(self):
        self.assertEqual([
            YumConfRepo('potato', 'file:///pot.at/to', ()),
            YumConfRepo(
                name='oleander',
                base_url='http://example.com/oleander',
                gpg_key_urls=(
                    'https://example.com/zupa',
                    'https://example.com/super/safe',
                ),
            ),
        ], list(self.yum_conf.gen_repos()))

    def test_isolate_repos(self):
        isolated_repos = [YumConfRepo(
            name='potato',
            base_url='https://example.com/potato',
            gpg_key_urls=('file:///much/secure/so/hack_proof', 'https://cat'),
        )]
        with self.assertRaisesRegex(AssertionError, 'Failed to isolate '):
            self.yum_conf.isolate().isolate_repos(isolated_repos)
        isolated_repos.append(YumConfRepo(
            name='oleander',
            base_url='https://zupa.example.com/sup',
            gpg_key_urls=(),
        ))

        out = io.StringIO()
        self.yum_conf.isolate().isolate_repos(isolated_repos).isolate_main(
            install_root='/install_root',
            config_path='/config_path',
            versionlock_dir='/versionlock_dir',
        ).write(out)

        self.assertEqual(textwrap.dedent('''\
        [main]
        debuglevel = 2
        gpgcheck = 1
        cachedir = /var/cache/yum
        persistdir = /var/lib/yum
        usercache = 0
        reposdir = /dev/null
        logfile = /var/log/yum.log
        installroot = /install_root
        config_file_path = /config_path
        syslog_device =\x20
        plugins = 1
        pluginpath = /versionlock_dir
        pluginconfpath = /versionlock_dir
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

        '''), out.getvalue())
