# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

class SignedSourceError(Exception): ...

def signed_source_sigil() -> str: ...
def sign_source(src: str) -> str: ...
