#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import tempfile
import unittest

from fs_image.nspawn_in_subvol.run_test import rewrite_test_cmd


class NspawnTestInSubvolTestCase(unittest.TestCase):

    def test_rewrite_cmd(self):
        bin = '/layer-test-binary'

        # Test no-op rewriting
        cmd = [bin, 'foo', '--bar', 'beep', '--baz', '-xack', '7', '9']
        with rewrite_test_cmd(cmd, next_fd=1337) as cmd_and_fd:
            self.assertEqual((cmd, None), cmd_and_fd)

        for rewritten_opt in ('--output', '--list-tests'):
            with tempfile.NamedTemporaryFile(suffix='.json') as t:
                prefix = ['--zap=3', '--ou', 'boo', '--ou=3']
                suffix = ['garr', '-abc', '-gh', '-d', '--e"f']
                with rewrite_test_cmd(
                        [bin, *prefix, f'{rewritten_opt}={t.name}', *suffix],
                        next_fd=37,
                ) as (new_cmd, fd_to_forward):
                    self.assertIsInstance(fd_to_forward, int)
                    # The last argument deliberately requires shell quoting.
                    self.assertEqual([
                        '/bin/bash', '-c', ' '.join([
                            'exec',
                            bin, rewritten_opt, '>(cat >&37)', *prefix,
                            *suffix[:-1],
                            """'--e"f'""",
                        ])
                    ], new_cmd)
