#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

echo "+======================================+"
echo "| ___  ___     _        _ _____ _____  |"
echo "| |  \/  |    | |      | |  _  /  ___| |"
echo "| | .  . | ___| |_ __ _| | | | \ \`--.  |"
echo "| | |\/| |/ _ \ __/ _\` | | | | |\`--. \\ |"
echo "| | |  | |  __/ || (_| | \ \_/ /\__/ / |"
echo "| \_|  |_/\___|\__\__,_|_|\___/\____/  |"
echo "|                                      | "
echo "+======================================+"
echo "+======================================+"
echo "| Welcome to a MetalOS VM!             |"
echo "| The VM will automatically shutdown   |"
echo "| when this shell is exited, or if     |"
echo "| you invoke \`systemctl poweroff\`      |"
echo "+======================================+"
echo "Uptime: $(cut -d' ' -f1 < /proc/uptime)"
echo "Release: "
