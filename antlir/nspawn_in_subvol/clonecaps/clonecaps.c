/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/**

Usage: clonecaps /proc/PID/status -- cmd argv1 ...

First, set current capabilities to match those in the specified
`procfs`-formatted process status file, and exit with a non-zero code if
that is not possible.

Note that we attempt to clone all 5 classes of capabilties: inheritable,
permitted, effective, bounding_set, ambient. Ambient caps will not be
clonable if this is compiled with `libcap-ng`.

We will fail unless `/proc/MY_PID/status` exaactly matches the specified
capability settings, so older `libcap-ng` is only usable in situations where
the current process's ambient caps already match the target's.

If capabilities match the target, this will `execv` a new process, using
arguments 3 onwards.

Compiling with `libcap-ng` older than 0.8:
    gcc -std=c99 nspawn_in_subvol/clonecaps/clonecaps.c -o clonecaps -lcap-ng

Compiling with 0.8 and newer:
    gcc -std=c99 -DCAPNG_SUPPORTS_AMBIENT=1 \
      nspawn_in_subvol/clonecaps/clonecaps.c -o clonecaps -lcap-ng

**/
#include <cap-ng.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// This is in service of OSS compilation, where getting a modern `libcap-ng`
// may be a hassle.
#ifndef CAPNG_SUPPORTS_AMBIENT
// Our `libcap-ng` cannot yet set ambient caps, so ask for the best it can.
#define CAPNG_SELECT_ALL CAPNG_SELECT_BOTH
// We use this as a key in our parsing, but never pass it to `capng_` code.
#define CAPNG_AMBIENT 16
#endif

// Returns the last valid capability for the current kernel, or -1 on error.
int find_last_cap() {
  int last_cap = -1; // Negative so we fail if `sscanf` fails
  FILE* last_cap_file = fopen("/proc/sys/kernel/cap_last_cap", "re");
  if (last_cap_file == NULL) {
    perror("open /proc/sys/kernel/cap_last_cap");
    return -1;
  }
  // No error-checking since `last_cap` will stay at -1 on failure to match.
  fscanf(last_cap_file, "%d", &last_cap);
  fclose(last_cap_file);
  if (last_cap < 0 || last_cap >= 64) {
    fprintf(stderr, "Got %d in /proc/sys/kernel/cap_last_cap\n", last_cap);
    return -1;
  }
  return last_cap;
}

// Returns `true` iff all `bits` were successfully added to `libcap-ng` state.
bool add_all_caps(int last_cap, capng_type_t cap_type, __u64 bits) {
  for (int cap = 0; cap <= last_cap; ++cap) {
    if (bits & ((__u64)1 << cap)) {
      if (capng_update(CAPNG_ADD, cap_type, cap) != 0) {
        fprintf(
            stderr,
            "Failed to add capability %d of capability type %d\n",
            cap,
            cap_type);
        return false;
      }
    }
  }
  return true;
}

// Returns `true` iff `expected_bits` matches `libcap-ng` state.
//
// NB: This is kind of useless as of Oct 2020, because of
//   https://github.com/stevegrubb/libcap-ng/issues/19
// Hopefully, once the bug is fixed, it won't even be necessary?
bool check_all_caps(int last_cap, capng_type_t cap_type, __u64 expected_bits) {
  __u64 actual_bits = 0;
  for (int cap = 0; cap <= last_cap; ++cap) {
    actual_bits |= (__u64)capng_have_capability(cap_type, cap) << cap;
  }
  if (actual_bits != expected_bits) {
    fprintf(
        stderr,
        "Unexpected caps of type %d: actual %llx != expected %llx\n",
        cap_type,
        actual_bits,
        expected_bits);
    return false;
  }
  return true;
}

typedef struct {
  __u64 inheritable;
  __u64 permitted;
  __u64 effective;
  __u64 bounding_set;
  __u64 ambient;
} cap_bits_t;

// Detail for `read_procfs_cap_bits`.
bool _match(
    char* buf,
    const char* pref,
    int* cap_type,
    int match_type,
    int* pref_len) {
  *pref_len = strlen(pref);
  if (strncmp(buf, pref, *pref_len) == 0) {
    *cap_type = match_type;
    return true;
  }
  return false;
}

// Parses the `Cap...:` lines from `/proc/PID/status` and populates `cap_bits`.
// Returns `true` on success. On error, logs to stderr and returns `false`.
bool read_procfs_cap_bits(const char* status_filename, cap_bits_t* cap_bits) {
  memset(cap_bits, 0, sizeof(*cap_bits));

  // We'll compare these to make sure we saw all the expected procfs lines.
  int expected_cap_types = CAPNG_INHERITABLE | CAPNG_PERMITTED |
      CAPNG_EFFECTIVE
      // NB: Antlir doesn't really like kernels older than 4.3, so
      // I did not bother to conditionalize the availability of CapAmb.
      | CAPNG_AMBIENT | CAPNG_BOUNDING_SET;

  int actual_cap_types = 0;

  FILE* status_file = fopen(status_filename, "re");
  if (status_file == NULL) {
    perror(status_filename);
    return false;
  }
  // Not all lines are under 64 bytes (the max length is ~unbounded thanks
  // to groups), but `Cap*:` lines will be, for the foreseeable future.
  // As of capability API v3, they are at 25 bytes including newline.
  char buf[64];
  bool continuing_line = false; // Previous `buf` lacked `\n`.
  while (fgets(buf, sizeof(buf), status_file)) {
    bool skip_buf = continuing_line;
    continuing_line = (buf[strlen(buf) - 1] != '\n');
    if (skip_buf) {
      continue; // Nothing to see here, this is not a `Cap*:` line.
    }
    // Both values are populated by `match`
    int cap_type = 0;
    int pref_len = 0;
    if (!(_match(buf, "CapInh:\t", &cap_type, CAPNG_INHERITABLE, &pref_len) ||
          _match(buf, "CapPrm:\t", &cap_type, CAPNG_PERMITTED, &pref_len) ||
          _match(buf, "CapEff:\t", &cap_type, CAPNG_EFFECTIVE, &pref_len) ||
          _match(buf, "CapBnd:\t", &cap_type, CAPNG_BOUNDING_SET, &pref_len) ||
          _match(buf, "CapAmb:\t", &cap_type, CAPNG_AMBIENT, &pref_len))) {
      continue;
    }

    // Fail on duplicate cap types in the input
    if (actual_cap_types & cap_type) {
      fprintf(
          stderr,
          "%s: Capability type %d occurred more than once\n",
          status_filename,
          cap_type);
      return false;
    }
    actual_cap_types |= cap_type;

    // Read out the bits for this capability, we'll apply them later
    char* end_of_bits = NULL;
    __u64 bits = strtoull(buf + pref_len, &end_of_bits, 16);
    // We should have read 16 hex bytes, terminated by a newline.
    if ((end_of_bits - (buf + pref_len)) != 16 || end_of_bits[0] != '\n') {
      fprintf(
          stderr,
          "%s: Failed to parse value %s for capability type %d\n",
          status_filename,
          buf + pref_len,
          cap_type);
      return false;
    }
    if (cap_type == CAPNG_INHERITABLE) {
      cap_bits->inheritable = bits;
    } else if (cap_type == CAPNG_PERMITTED) {
      cap_bits->permitted = bits;
    } else if (cap_type == CAPNG_EFFECTIVE) {
      cap_bits->effective = bits;
    } else if (cap_type == CAPNG_BOUNDING_SET) {
      cap_bits->bounding_set = bits;
    } else if (cap_type == CAPNG_AMBIENT) {
      cap_bits->ambient = bits;
    }
  }
  fclose(status_file);
  if (actual_cap_types != expected_cap_types) {
    fprintf(
        stderr,
        "%s: Missing capability types: %d vs %d\n",
        status_filename,
        actual_cap_types,
        expected_cap_types);
    return false;
  }
  return true;
}

bool is_debug() {
  static int debug = -1;
  if (debug == -1) {
    const char* debug_env = getenv("ANTLIR_DEBUG");
    debug = debug_env && debug_env[0];
  }
  return debug;
}

void fprint_cap_bits(FILE* file, const char* msg, cap_bits_t cap_bits) {
  fprintf(
      file,
      "%s: i %llx, p %llx, e %llx, bs %llx, a %llx\n",
      msg,
      cap_bits.inheritable,
      cap_bits.permitted,
      cap_bits.effective,
      cap_bits.bounding_set,
      cap_bits.ambient);
}

int main(int argc, char** argv) {
  if (argc < 3 || strcmp("--", argv[2]) != 0) {
    fprintf(stderr, "Usage: clonecaps /proc/PID/status -- cmd argv1 ...\n");
    return 1;
  }
  char* target_procfs_path = argv[1];
  argv += 3; // Skip "our" args, this is now ready to `execv`.
  argc -= 3;

  // The running kernel may not match our compile-time headers.
  int last_cap = find_last_cap();
  if (last_cap == -1) { // The function already printed the error
    return 1;
  }

  // We read this to check that `libcap-ng` worked correctly, since
  // `check_all_caps` cannot.
  char my_procfs_path[64]; // `/proc/PID/status` fits even with 64-bit PIDs
  if (snprintf(
          my_procfs_path,
          sizeof(my_procfs_path),
          "/proc/%d/status",
          getpid()) >= sizeof(my_procfs_path)) {
    fprintf(stderr, "PID too long??? %d\n", getpid());
    return 1;
  }

  if (is_debug()) {
    cap_bits_t cur_bits;
    if (!read_procfs_cap_bits(my_procfs_path, &cur_bits)) {
      return 1; // An error was already printed
    }
    fprint_cap_bits(stderr, "Initial procfs for getpid()", cur_bits);
  }

  cap_bits_t target_bits;
  if (!read_procfs_cap_bits(target_procfs_path, &target_bits)) {
    return 1; // An error was already printed
  }
  if (is_debug()) {
    fprint_cap_bits(stderr, "Procfs for target PID", target_bits);
  }

  capng_clear(CAPNG_SELECT_ALL); // Clear traditional, bounding, ambient

  // Clone the target's values
  if (!(add_all_caps(last_cap, CAPNG_INHERITABLE, target_bits.inheritable) &&
        add_all_caps(last_cap, CAPNG_PERMITTED, target_bits.permitted) &&
        add_all_caps(last_cap, CAPNG_EFFECTIVE, target_bits.effective) &&
        add_all_caps(last_cap, CAPNG_BOUNDING_SET, target_bits.bounding_set)
#ifdef CAPNG_SUPPORTS_AMBIENT
        && add_all_caps(last_cap, CAPNG_AMBIENT, target_bits.ambient)
#endif
            )) {
    return 1; // `add_all_caps` already printed an error
  }

  // Apply traditional & bounding
  if (capng_apply(CAPNG_SELECT_ALL) != 0) {
    fprint_cap_bits(stderr, "Failed to apply capabilities", target_bits);
    return 1;
  }

#ifdef CAPNG_SUPPORTS_AMBIENT
  // Due to the following bug, ambient capabilities only get applied the
  // second time around: https://github.com/stevegrubb/libcap-ng/issues/18
  //
  // This can be removed once both the OSS and FB versions of `libcap-ng`
  // are guaranteed to include b6ff250a71a1f0a11b2917186155d2426080293d
  // from https://github.com/stevegrubb/libcap-ng
  if (capng_apply(CAPNG_SELECT_ALL) != 0) {
    fprint_cap_bits(stderr, "Failed to re-apply capabilities", target_bits);
    return 1;
  }
#endif

  if (!(check_all_caps(last_cap, CAPNG_INHERITABLE, target_bits.inheritable) &&
        check_all_caps(last_cap, CAPNG_PERMITTED, target_bits.permitted) &&
        check_all_caps(last_cap, CAPNG_EFFECTIVE, target_bits.effective) &&
        check_all_caps(last_cap, CAPNG_BOUNDING_SET, target_bits.bounding_set)
#ifdef CAPNG_SUPPORTS_AMBIENT
        && check_all_caps(last_cap, CAPNG_AMBIENT, target_bits.ambient)
#endif
            )) {
    return 1; // `check_all_caps` already printed an error
  }

  cap_bits_t final_bits;
  if (!read_procfs_cap_bits(my_procfs_path, &final_bits)) {
    return 1; // An error was already printed
  }

  // Note that this will fail if the target proc has ambient caps that do
  // not match ours, and our `libcap-ng` is old.
  //
  // This also detects an `libcap-ng` bug:
  //   https://github.com/stevegrubb/libcap-ng/issues/19
  if (final_bits.inheritable != target_bits.inheritable ||
      final_bits.permitted != target_bits.permitted ||
      final_bits.effective != target_bits.effective ||
      final_bits.bounding_set != target_bits.bounding_set ||
      final_bits.ambient != target_bits.ambient) {
    fprint_cap_bits(stderr, "After applying new capabilities", target_bits);
    fprint_cap_bits(stderr, "Aborting, procfs does not match", final_bits);
    return 1;
  } else if (is_debug()) {
    fprint_cap_bits(stderr, "Final procfs for getpid()", final_bits);
  }

  execv(argv[0], argv);
  perror("execv");
  return 1;
}
