#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Import FB-specific implementations if available.
try:
    from .facebook.logger import log_sample, init_sample_logging  # noqa: F401
except ImportError:  # pragma: no cover
    def log_sample(*args, **kwargs):
        pass

    def init_sample_logging(*args, **kwargs):
        pass
