#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import sys

multi_user_wants = sys.argv[1] + "/multi-user.target.wants"
os.makedirs(multi_user_wants, exist_ok=True)

fake_service = "fake-generated.service"
fake_service_path = sys.argv[1] + "/" + fake_service

with open(fake_service_path, "w") as f:
    f.write(
        """\
[Unit]
Description=Generated fake service from bootable-systemd-os-with-buck-runnables

[Service]
ExecStart=/fake-service generated
"""
    )

os.symlink(fake_service_path, multi_user_wants + "/" + fake_service)

open(sys.argv[1] + "/fake-systemd-generator-ran", "w").close()
