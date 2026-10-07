#!/usr/bin/bash
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

set -euo pipefail

if [ ! -d "antlir2-out" ]; then
    cd "$(hg root)"
fi

declare -a working_volumes=()
declare -A seen_working_volumes=()

run_btrfs_with_sudo_fallback() {
    if btrfs "$@"; then
        return
    fi
    sudo -n btrfs "$@"
}

run_in_subid_userns() (
    local user uid gid subuid subgid subuid_count subgid_count
    user="$(id -un)"
    uid="$(id -u)"
    gid="$(id -g)"
    read -r subuid subuid_count < <(awk -F: -v u="$user" -v i="$uid" '$1==u||$1==i {print $2, $3; exit}' /etc/subuid) || return 1
    read -r subgid subgid_count < <(awk -F: -v u="$user" -v i="$uid" '$1==u||$1==i {print $2, $3; exit}' /etc/subgid) || return 1

    local namespace_dir='' pid='' rc='' ready_fd go_fd
    trap '
        rc=$?
        set +e
        if [[ -n "$pid" ]]; then
            kill "$pid" 2>/dev/null
            wait "$pid" 2>/dev/null
        fi
        if [[ -n "$namespace_dir" ]]; then
            rm -f "$namespace_dir/ready" "$namespace_dir/go"
            rmdir "$namespace_dir"
        fi
        exit "$rc"
    ' EXIT
    for signal in 1 2 3 13 15; do
        eval "trap 'exit $((128 + signal))' $signal"
    done
    namespace_dir="$(mktemp -d)" || return 1
    mkfifo "$namespace_dir/ready" "$namespace_dir/go" || return 1
    exec {ready_fd}<>"$namespace_dir/ready" || return 1
    exec {go_fd}<>"$namespace_dir/go" || return 1

    # Signal readiness after unshare; mapping before it races with namespace creation.
    # shellcheck disable=SC2016
    unshare --user -- bash -c 'printf "ready\n" >&"$1" || exit 1; read -r _ <&"$2" || exit 1; shift 2; exec "$@"' _ "$ready_fd" "$go_fd" "$@" &
    pid=$!
    read -r -t 5 -u "$ready_fd" _ || return 1
    newgidmap "$pid" 0 "$gid" 1 1 "$subgid" "$subgid_count" || return 1
    newuidmap "$pid" 0 "$uid" 1 1 "$subuid" "$subuid_count" || return 1
    printf 'go\n' >&"$go_fd"
    wait "$pid" && rc=0 || rc=$?
    pid=
    return "$rc"
)

remove_subvolume() {
    local subvolume="$1"
    if run_btrfs_with_sudo_fallback subvolume delete "$subvolume"; then
        return
    fi
    # shellcheck disable=SC2016
    local empty_subvolume='
        btrfs property set -f "$1" ro false || exit 1
        rm -rf --one-file-system -- "$1" && exit 0
        # A failed removal may leave nested read-only subvolumes.
        find "$1" -depth -type d -inum 256 -exec btrfs property set -f {} ro false \; || exit 1
        find "$1" -depth -type d -inum 256 -exec rm -rf --one-file-system -- {} +
    '
    if [[ "$(stat -c %u "$subvolume")" != "$(id -u)" ]]; then
        sudo -n bash -c "$empty_subvolume" _ "$subvolume" && return
        echo "cannot remove non-user-owned subvolume without sudo: $subvolume" >&2
        return 1
    else
        run_in_subid_userns bash -c "$empty_subvolume" _ "$subvolume"
    fi
}

if [ -d "antlir2-out" ]; then
    working_volumes+=("antlir2-out")
fi

if [ -e ".eden/root" ] && command -v mkscratch >/dev/null 2>&1; then
    eden_root="$(readlink ".eden/root" || true)"
    if [ -n "${eden_root}" ]; then
        scratch_working_volume="$(
            mkscratch --no-create path "${eden_root}" --subdir antlir2-out 2>/dev/null || true
        )"
        if [ -n "${scratch_working_volume}" ] && [ -d "${scratch_working_volume}" ]; then
            working_volumes+=("${scratch_working_volume}")
        fi
    fi
fi

if [ "${#working_volumes[@]}" -eq 0 ]; then
    echo "no antlir2-out found in repo root or mkscratch path, exiting..."
    exit
fi

failures=0
subvolume_list=
cleanup_rc=
trap 'cleanup_rc=$?; set +e; rm -f "$subvolume_list"; exit "$cleanup_rc"' EXIT
for signal in 1 2 3 13 15; do
    eval "trap 'exit $((128 + signal))' $signal"
done

for working_volume in "${working_volumes[@]}"; do
    resolved_working_volume="$(
        readlink -f "${working_volume}" 2>/dev/null || echo "${working_volume}"
    )"
    if [ -n "${seen_working_volumes[${resolved_working_volume}]+x}" ]; then
        continue
    fi
    seen_working_volumes["${resolved_working_volume}"]=1

    echo "cleaning antlir2 subvolumes in ${working_volume}"
    if ! pushd "${working_volume}" >/dev/null; then
        failures=$((failures + 1))
        continue
    fi

    subvolume_list="$(mktemp)"
    if ! find . -mindepth 1 -maxdepth 2 -type d -inum 256 -prune -print0 > "${subvolume_list}"; then
        echo "could not list subvolumes in ${working_volume}" >&2
        failures=$((failures + 1))
    fi
    while IFS= read -r -d '' subvolume; do
        if ! remove_subvolume "${subvolume}"; then
            echo "could not remove subvolume: ${working_volume}/${subvolume}" >&2
            failures=$((failures + 1))
        fi
    done < "${subvolume_list}"

    rm -f "${subvolume_list}"
    subvolume_list=
    popd >/dev/null
done

if [[ "$failures" -ne 0 ]]; then
    echo "Antlir2 cleanup had $failures failure(s)" >&2
    exit 1
fi
