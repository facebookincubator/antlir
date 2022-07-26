# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Trigger initialization of Rust modules so that they will be writen into
# sys.modules at the right import locations to match where the
# antlir_rust_extension is defined
import antlir.rust  # noqa
