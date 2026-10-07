#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -euo pipefail

CLEANUP_SCRIPT="$(realpath "$1")"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/clean-antlir2-out-test.XXXXXX")"
trap 'rm -rf "${TEST_ROOT}"' EXIT

MOCK_BIN="${TEST_ROOT}/bin"
export TEST_LOG="${TEST_ROOT}/commands.log"
WORKTREE="${TEST_ROOT}/worktree"
mkdir -p "${MOCK_BIN}" "${WORKTREE}/antlir2-out"

cat > "${MOCK_BIN}/find" <<'EOF'
#!/usr/bin/bash
printf 'find %s\n' "$*" >> "${TEST_LOG}"
printf './subvols/first\0./subvols/second\0'
exit "${FAIL_FIND:-0}"
EOF

cat > "${MOCK_BIN}/btrfs" <<'EOF'
#!/usr/bin/bash
printf 'btrfs %s\n' "$*" >> "${TEST_LOG}"
if [[ "$1" == "subvolume" && "${FAIL_BTRFS_DIRECT:-0}" == "1" && -z "${VIA_SUDO:-}" ]]; then
    exit 1
fi
if [[ "$1" == "property" && "${FAIL_FIRST:-0}" == "1" && "$4" == "./subvols/first" ]]; then
    exit 1
fi
EOF

cat > "${MOCK_BIN}/sudo" <<'EOF'
#!/usr/bin/bash
printf 'sudo %s\n' "$*" >> "${TEST_LOG}"
[[ "$1" == "-n" ]] || exit 1
[[ "${FAIL_SUDO:-0}" == "0" ]] || exit 1
shift
VIA_SUDO=1 "$@"
EOF

cat > "${MOCK_BIN}/unshare" <<'EOF'
#!/usr/bin/bash
printf 'userns\n' >> "${TEST_LOG}"
[[ "$1 $2" == "--user --" ]] || exit 1
shift 2
exec "$@"
EOF

cat > "${MOCK_BIN}/newgidmap" <<'EOF'
#!/usr/bin/bash
printf 'map\n' >> "${TEST_LOG}"
exit "${FAIL_MAP:-0}"
EOF
cp "${MOCK_BIN}/newgidmap" "${MOCK_BIN}/newuidmap"
cat > "${MOCK_BIN}/awk" <<'EOF'
#!/usr/bin/bash
printf '100000 65536\n'
EOF
REAL_STAT="$(command -v stat)"
export REAL_STAT
cat > "${MOCK_BIN}/stat" <<'EOF'
#!/usr/bin/bash
if [[ "${ROOT_OWNED_FIRST:-0}" == "1" && "$3" == "./subvols/first" ]]; then
    printf '0\n'
else
    exec "${REAL_STAT}" "$@"
fi
EOF
chmod +x "${MOCK_BIN}"/*

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

assert_contains() {
    [[ "$(<"${TEST_LOG}")" == *"$1"* ]] || fail "missing log entry: $1"
}

run_cleanup() {
    : > "${TEST_LOG}"
    mkdir -p "${WORKTREE}/antlir2-out/subvols/first" "${WORKTREE}/antlir2-out/subvols/second"
    (
        cd "${WORKTREE}"
        PATH="${MOCK_BIN}:${PATH}" "${CLEANUP_SCRIPT}" > "${TEST_ROOT}/output" 2>&1
    )
}

run_cleanup
assert_contains 'find . -mindepth 1 -maxdepth 2 -type d -inum 256 -prune -print0'
assert_contains 'btrfs subvolume delete ./subvols/second'
[[ "$(<"${TEST_LOG}")" != *sudo* ]] || fail 'unexpected sudo'

FAIL_BTRFS_DIRECT=1 run_cleanup
assert_contains 'sudo -n btrfs subvolume delete ./subvols/second'

FAIL_BTRFS_DIRECT=1 FAIL_SUDO=1 run_cleanup
[[ ! -e "${WORKTREE}/antlir2-out/subvols/first" ]] || fail 'first volume remains'
[[ ! -e "${WORKTREE}/antlir2-out/subvols/second" ]] || fail 'second volume remains'
[[ "$(<"${TEST_LOG}")" == *$'userns\nmap\nmap'* ]] || fail 'namespace mapping was skipped'

if FAIL_BTRFS_DIRECT=1 FAIL_SUDO=1 FAIL_FIRST=1 run_cleanup; then
    fail 'expected partial cleanup failure'
fi
[[ -e "${WORKTREE}/antlir2-out/subvols/first" ]] || fail 'failed volume was removed'
[[ ! -e "${WORKTREE}/antlir2-out/subvols/second" ]] || fail 'later volume was skipped'

if FAIL_BTRFS_DIRECT=1 FAIL_SUDO=1 ROOT_OWNED_FIRST=1 run_cleanup; then
    fail 'expected root-owned volume cleanup failure'
fi
[[ ! -e "${WORKTREE}/antlir2-out/subvols/second" ]] || fail 'later volume was skipped'

if FAIL_BTRFS_DIRECT=1 FAIL_SUDO=1 FAIL_MAP=1 run_cleanup; then
    fail 'expected namespace mapping failure'
fi
assert_contains 'btrfs subvolume delete ./subvols/second'

if FAIL_FIND=1 run_cleanup; then
    fail 'expected enumeration failure'
fi
assert_contains 'btrfs subvolume delete ./subvols/second'
