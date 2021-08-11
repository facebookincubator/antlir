# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest


class AntlirTestCase(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        # `unittest`'s output shortening makes tests hard to debug, e.g.
        #   i[Mixin(requiresHelper=False, fbpkgs=i[Mi[108 chars]x'])] !=
        #   [Mixin(requiresHelper=False, fbpkgs=i[Mix[100 chars]i[])]
        unittest.util._MAX_LENGTH = 20000  # 250 lines of 80 chars
        self.maxDiff = 20000
