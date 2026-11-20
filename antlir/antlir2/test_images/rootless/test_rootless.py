# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import unittest

BUILD_ENV = "/.build_environment.json"

# Build-time scratch paths serialized into the image by the build_environment
# feature. The build creates its working tempdir under BUCK_SCRATCH_PATH, so
# any root-owned leftovers land in buck-out/, which is visible in here via the
# repo mount. TMPDIR is checked too, but note the container's /tmp is a private
# tmpfs - host /tmp leaks are NOT visible from in here, which is exactly why
# the build must place its tempdir under scratch for the leak to be detectable.
SCRATCH_KEYS = ("BUCK_SCRATCH_PATH", "TMPDIR")

# Inside a rootless test container our host uid is mapped to container root,
# so a file owned by real host root shows up as the overflow uid instead of 0.
OVERFLOW_UIDS = {65534, 4294967294}


def _sudo_skip_reason(data: dict, *, rootless: bool) -> str | None:
    # The test process itself was invoked via sudo: there is no
    # unprivileged user to compare ownership against.
    if os.environ.get("SUDO_UID") is not None or os.environ.get("SUDO_USER"):
        return "test is running under sudo (SUDO_UID/SUDO_USER set)"
    if data.get("inside_re_worker"):
        # RE workers run as root regardless of sudo: the scratch paths
        # are not visible in the test container, so there is nothing
        # for this check to skip - it already passes vacuously there.
        return None
    # `unprivileged_ids` is the `(uid, gid)` the build's sudo dropped
    # to (see build_environment). Rooted builds always go through
    # sudo, so None/0 there means no unprivileged build user; rootless
    # builds never sudo (None is normal), so only 0 means sudo-by-root.
    ids = data.get("unprivileged_ids")
    uid = ids[0] if ids else None
    if uid == 0 or (not rootless and "unprivileged_ids" in data and uid is None):
        return (
            "image was built under sudo with no unprivileged build "
            "user (e.g. a -sudo Sandcastle job)"
        )
    return None


def _is_bad(st: os.stat_result, *, rootless: bool) -> bool:
    if rootless:
        return st.st_uid in OVERFLOW_UIDS or st.st_gid in OVERFLOW_UIDS
    return st.st_uid == 0 or st.st_gid == 0


def _bad_files(path: str, *, rootless: bool) -> list[str]:
    bad: list[str] = []

    def _check(p: str) -> None:
        try:
            st = os.lstat(p)
        except OSError:
            return
        if _is_bad(st, rootless=rootless):
            bad.append(p)

    # The top-level dir itself may be the root-owned leftover: a leaked
    # tempdir is often mode 700, so we cannot descend into it, but lstat on it
    # still works.
    _check(path)

    def _onerror(err: OSError) -> None:
        # Unreadable dir: still ownership-check the dir itself.
        _check(err.filename)

    seen = 0
    for dirpath, dirnames, filenames in os.walk(path, onerror=_onerror):
        for name in dirnames + filenames:
            _check(os.path.join(dirpath, name))
            seen += 1
            if len(bad) >= 20 or seen >= 20000:
                return bad
    return bad


class TestRootless(unittest.TestCase):
    def setUp(self) -> None:
        with open(BUILD_ENV) as f:
            self.data = json.load(f)
        self.rootless = os.environ.get("EXPECT_ROOTLESS") == "1"

    def test_build_environment_valid(self) -> None:
        self.assertIsInstance(self.data, dict)
        self.assertIn("env", self.data)
        self.assertIsInstance(self.data["env"], dict)

    def test_image_marker_files(self) -> None:
        # Files baked into the image itself must survive the build with the
        # expected contents and ownership (container root).
        path = "/rootless-check/hello"
        with open(path) as f:
            self.assertEqual(f.read(), "hello\n")
        st = os.stat(path)
        self.assertEqual(st.st_uid, 0, f"{path} has wrong owner")
        self.assertEqual(st.st_gid, 0, f"{path} has wrong group")

    def test_no_root_owned_scratch_leftovers(self) -> None:
        # Build-time scratch dirs (under buck-out/) must not contain files
        # owned by root. A rooted cad-stack build that fails to clean up its
        # working tempdir leaves a root-owned dir behind - this test codifies
        # the healthy state to track that fix and prevent regressions.
        # Under sudo (e.g. a `-sudo` Sandcastle job) there is no
        # unprivileged build user, so root-owned scratch files are
        # expected and this check has nothing meaningful to compare.
        if reason := _sudo_skip_reason(self.data, rootless=self.rootless):
            self.skipTest(reason)
        env = self.data["env"]
        checked = []
        for key in SCRATCH_KEYS:
            raw = env.get(key)
            if not raw:
                continue
            path = raw if os.path.isabs(raw) else os.path.join(os.getcwd(), raw)
            if not os.path.exists(path):
                # Built on RE (path is from a remote worker, e.g. /re_cwd/...
                # with inside_re_worker=True) or scratch already cleaned -
                # nothing to check.
                continue
            checked.append(f"{key}={path}")
            bad = _bad_files(path, rootless=self.rootless)
            self.assertEqual([], bad, f"root-owned files left under {key}={path}")
        if not self.data.get("inside_re_worker"):
            # Local builds must serialize at least one visible scratch path,
            # otherwise this test is vacuous (e.g. sudo scrubbed the action
            # env and the feature recorded nothing).
            self.assertNotEqual(
                [],
                checked,
                f"no local scratch paths to check (env={env}) - test is vacuous",
            )
