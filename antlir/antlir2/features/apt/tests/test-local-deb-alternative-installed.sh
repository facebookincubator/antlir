#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -ex

test "$(readlink /usr/bin/antlir2-greeter)" = "/etc/alternatives/antlir2-greeter"
test "$(readlink /etc/alternatives/antlir2-greeter)" = "/usr/bin/antlir2-greeter-hello"
test "$(readlink /usr/bin/antlir2-greet)" = "/etc/alternatives/antlir2-greet"
test "$(readlink /etc/alternatives/antlir2-greet)" = "/usr/bin/antlir2-greeter-hello"

test "$(antlir2-greeter)" = "hello from antlir2"
test "$(antlir2-greet)" = "hello from antlir2"

update-alternatives --query antlir2-greeter > /tmp/query
grep -qxF "Value: /usr/bin/antlir2-greeter-hello" /tmp/query
grep -qxF "Status: auto" /tmp/query
