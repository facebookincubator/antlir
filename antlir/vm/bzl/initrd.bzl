# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def initrd(uname: str):
    """ Get the initrd target based on arch and uname. 

    Initrd data isn't really antlir's business, but some of our images need it.
    MetalOS initrd is our only option, but we don't want to import its code. So
    we hardcode target path here for now. Ideally, either we include some OSS
    initrd, or we set this as a REPO_CFG so it's easier to specify where initrd
    targets live.
    """
    return "//metalos/vm/initrd:vm-{}-initrd".format(uname)
