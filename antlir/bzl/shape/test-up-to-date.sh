#!/bin/bash
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

if git diff --no-index "$1" "$2" ; then
    echo "all good :)"
else
    echo
    echo "If you changed the shape definition, run '$3' to update the generated code"
    exit 1
fi
