#!/usr/libexec/platform-python
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# NOTE: this must be run with system python, so cannot be a PAR file
# /usr/bin/dnf itself uses /usr/libexec/platform-python, so by using that we can
# ensure that we're using the same python that dnf itself is using

import importlib.util
import sys

import dnf
import libdnf

spec = importlib.util.spec_from_file_location(
    "antlir2_dnf_base", "/__antlir2__/dnf/base.py"
)
antlir2_dnf_base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(antlir2_dnf_base)


class AntlirPlugin(dnf.Plugin):
    name = "antlir"

    def __init__(self, base, cli):
        super().__init__(base, cli)

    def pre_config(self):
        antlir2_dnf_base.add_repos(base=self.base, repos_dir="/__antlir2__/dnf/repos")

    def config(self):
        antlir2_dnf_base.configure_base(
            base=self.base, set_persistdir_under_installroot=False
        )

        # Patch the base download_packages function so that it does not do
        # anything. For local repositories all it does is check checksums, but
        # we've already verified the checksum with buck2, and possibly changed
        # the file on disk in the case of rpm2extents. This is perfectly safe.
        def _noop_download(*args, **kwargs):
            assert (
                not self.base.conf.destdir
            ), "destdir requires copying packages which is not implemented (yet)"

        self.base.download_packages = _noop_download

    def resolved(self):
        try:
            explicitly_removed_package_names = set()
            for item in self.base.transaction:
                if (
                    item.action == libdnf.transaction.TransactionItemAction_REMOVE
                    and item.reason == libdnf.transaction.TransactionItemReason_USER
                ):
                    explicitly_removed_package_names.add(item.pkg.name)
            antlir2_dnf_base.ensure_no_implicit_removes(
                base=self.base,
                explicitly_removed_package_names=explicitly_removed_package_names,
            )
        except antlir2_dnf_base.AntlirError as e:
            print(str(e), file=sys.stderr)
            # reverse the default assumeyes config to cause dnf to abort the
            # transaction (because it won't do it if we just raise an exception)
            self.base.conf.assumeno = True
            self.base.conf.assumeyes = False
            sys.exit(1)
        except Exception as e:
            print(str(e), file=sys.stderr)
            sys.exit(1)

    def pre_transaction(self):
        pass
