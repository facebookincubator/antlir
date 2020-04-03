#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import textwrap
import unittest

from fs_image.fs_utils import create_ro, temp_dir

from ..yum_dnf_conf import YumDnf
from .. import write_yum_dnf_conf as wydc

_CONF_IN = '''\
# Unfortunately, comments are discarded by ConfigParser, but I don't want to
# depend on `ConfigObj` or `iniparse` for this.
[main]
debuglevel=2
gpgcheck=1

[potato]
baseurl=https://one.example.com/something-or-other
enabled=1

[oleander]
baseurl=http://example.com/this-is-a-flowering-bush
gpgkey=https://example.com/zupa
\thttps://example.com/super/safe
enabled=1
'''

# Below, we use \x20 (hex-quoted space) to silence the linter that bans
# trailing whitespace.
_CONF_OUT = '''\
[main]
debuglevel = 2
gpgcheck = 1
cachedir = /var/cache/{prog_name}
persistdir = /var/lib/{prog_name}
reposdir = /dev/null
logfile = /var/log/{prog_name}.log
config_file_path = /INSTALL/DIR/{prog_name}/{prog_name}.conf
timeout = 60
plugins = 1
pluginpath = /INSTALL/DIR/{prog_name}/plugins
pluginconfpath = /INSTALL/DIR/{prog_name}/plugins
varsdir = /dev/null
usercache = 0
syslog_device =\x20
bugtracker_url =\x20
fssnap_devices = !*
{extra_directives}\

[potato]
baseurl = http://localhost:1234/potato
\thttp://localhost:5678/potato
enabled = 1
gpgkey =\x20

[oleander]
baseurl = http://localhost:1234/oleander
\thttp://localhost:5678/oleander
gpgkey = http://localhost:1234/oleander/zupa
\thttp://localhost:1234/oleander/safe
enabled = 1

'''


# This is the base class for two test classes at the bottom of the file.
class WriteYumDnfConfTestImpl:

    def test_conf(self):
        install_dir = '/INSTALL/DIR'
        prog_name = self._YUM_DNF.value
        expected_out = _CONF_OUT.format(
            prog_name=prog_name,
            extra_directives=textwrap.dedent('''\
                skip_missing_names_on_install = 0
                skip_missing_names_on_update = 0
            ''') if self._YUM_DNF == YumDnf.yum else '',
        )
        with temp_dir() as td:
            with create_ro(td / 'in', 'w') as outf:
                outf.write(_CONF_IN)
            wydc.main([
                f'--output-dir={td / "out"}',
                f'--install-dir={install_dir}',
                *('--write-conf', self._YUM_DNF.value, td / 'in', '1234 5678'),
            ])
            with open(td / f'out/{prog_name}/{prog_name}.conf') as infile:
                self.assertEqual(expected_out, infile.read())


class WriteDnfConfTest(WriteYumDnfConfTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.dnf


class WriteYumConfTest(WriteYumDnfConfTestImpl, unittest.TestCase):
    _YUM_DNF = YumDnf.yum
