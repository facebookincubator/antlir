/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This is meant to be `LD_PRELOAD`ed into `yum` or `dnf`.  We intercept the
// `rename` glibc call, and check whether the destination path exists under
// `ANTLIR_SHADOWED_PATHS_ROOT`.  If the shadowed path does exist, change
// the destination path of the `rename` to overwrite it.  If the shadowed
// path does not exist, or the environment variable is not set, perform the
// unmodified `rename`.
//
// Caveats:
//
//  - This is implemented in a way that is asynchronous signal-unsafe,
//    whereas `rename (3)` is supposed to be AS-safe according to POSIX.
//
//    We don't bother with an AS-safe implementation because of its cost,
//    and because the risk seems low. Specifically:
//      * Both `yum` and `dnf` call out to `rpm` to do package installation.
//      * The `dnf` codebase has no mentions of `rename` at all.
//      * `yum` has some `os.rename` calls, but it is in Python, and as such
//        it's almost impossible to run anything in a context that requires
//        async-signal safety.
//      * `rpm` calls `rename (3)` through `fsmRename`, which in its current
//        incarnation has several AS-unsafe calls.
//
//    It is technically possible that some dependency of either package
//    manager uses `rename` in a signal handler for some kind of last-ditch
//    cleanup thing.  However, it is not very plausible because libraries
//    should not install signal handlers.  Moreover, we're not too concerned
//    about breaking error handling, since we expect image builds to
//    generally be on the "gold path" where programs exit cleanly.
//
//    While the risk is low, the cost is considerable:
//      * One can replace uses of `malloc` and `free` by stack buffers of
//        `PATH_MAX` in size, and lose the `strndup`.  This has some
//        downsides since it may artificially limit path length in some
//        settings, but it's probably good enough.
//      * `realpath` and `canonicalize_file_name` are AS-unsafe because of
//        heap accesses.  I suspect that `realpath` with a pre-allocated
//        buffer might be fine (same caveat: artificially limiting the path
//        length), but the docs don't explicitly promise it.
//      * Reimplementing `realpath` with static buffers is a big pain.
//      * Losing the dependency on `snprintf` is also a pain.
//      * Removing the `fprintf` reduces debuggability.
//    On net, an AS-safe implementation would be far longer and would require
//    a much higher test burden.
//
//  - This lacks support for directories because we don't currently shadow
//    directories, and `yum` / `dnf` do not (and cannot) use `rename` for
//    overwriting directories.
//
//   - About logging & error handling: we log to stderr only when we alter
//     the `rename`.  Many "error" cases in the code is actually just an
//     indication that we shouldn't be interposing.  There are also a few
//     "this should never happen" conditions, where we would still get an
//     error message from `yum` when it fails to overwrite the read-only
//     bind mount.

#ifndef _GNU_SOURCE
#define _GNU_SOURCE 1
#endif
#include <dlfcn.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h> // With _GNU_SOURCE gives us the GNU `basename`
#include <sys/stat.h>

#ifdef __cplusplus
extern "C" {
#endif

static char* g_shadowed_paths_root = NULL;
static size_t g_len_shadowed_paths_root = 0;

__attribute__((__constructor__)) static void __init__() {
  // Grabbing the root for shadowed paths from the environment is less
  // robust than hardcoding it (something can unset the env var), but in
  // our current usage, there is nothing between `yum-dnf-from-snapshot`
  // and `yum` or `dnf` that would do that.  And we have tests.
  //
  // The upside is that it makes our tests cleaner, and eliminates the
  // need to rebuild the `.so` (and with it, the BA) to change the root.
  //
  // As far as security, we're an `LD_PRELOAD` library, so we already
  // trust the environment roughly 100%.
  g_shadowed_paths_root = getenv("ANTLIR_SHADOWED_PATHS_ROOT");
  if (g_shadowed_paths_root) {
    g_len_shadowed_paths_root = strlen(g_shadowed_paths_root);
  }
}

// If the parent directory of `path` exists, and the environment variable
// `ANTLIR_SHADOWED_PATHS_ROOT` is set, allocates and returns a
// NUL-terminated canonical "shadowed original" for `path`, under that root.
//
// Returns NULL on error.
//
// This is not `static` so our tests can see it.  In production builds, it
// gets hidden via `-fvisibility=hidden`.
char* get_shadowed_original(const char* path) {
  // No shadow paths root? Don't alter any `rename` calls.
  if (!g_shadowed_paths_root) {
    return NULL;
  }

  const char* base = basename(path);
  const int len_base = strlen(base);
  const int len_path = strlen(path);

  char* dirname = (len_base == len_path)
      ? strdup(".") // Otherwise path == "a" would make bad dirname == ""
      : strndup(path, len_path - len_base); // Keep trailing / for path == /a
  if (!dirname) {
    return NULL;
  }
  // `rename` does not follow symlinks in the last component
  char* realdir = canonicalize_file_name(dirname);

  char* orig = NULL;
  if (realdir && realdir[0] == '/') {
    const size_t len_orig =
        g_len_shadowed_paths_root + strlen(realdir) + 1 + len_base;
    orig = malloc(len_orig + 1);
    if (orig) {
      snprintf(
          orig,
          len_orig + 1,
          "%s%s%s%s",
          g_shadowed_paths_root,
          realdir,
          // Don't emit an extra / for `realdir == "/"`
          (realdir[1] == '\0' ? "" : "/"),
          base);
    }
  }

  if (realdir) {
    free(realdir);
  }
  free(dirname);
  return orig;
}

// For us to decide to redirect a `rename`'s `new` to its shadow location,
// a few conditions have to be met:
//   - `new` has to exist and not be a directory (see top doc)
//   - `new` must not be the same inode as `old` (inline comment)
//   - the shadow of `new` must exist and not be a directory
//
// If all conditions are met, return an allocated path to the shadow of
// `new`, to be `free`d by the caller.  Otherwise, return NULL.
//
// This is not `static` so our tests can see it.  In production builds, it
// gets hidden via `-fvisibility=hidden`.
char* get_shadowed_rename_dest(const char* old, const char* new) {
  // We don't support shadowing directories.
  struct stat st;
  if (0 != lstat(new, &st)) {
    return NULL;
  }
  if (S_ISDIR(st.st_mode)) {
    return NULL;
  }

  struct stat st_old;
  if (0 != lstat(old, &st_old)) {
    return NULL;
  }
  // `rename` should be a no-op if `old` and `new` are the same.  However,
  // if we were to rewrite the destination path, then `rename` would fail
  // because `old`, a shadowed path, would be a read-only bind mount.
  if (st.st_ino == st_old.st_ino && st.st_dev == st_old.st_dev) {
    return NULL;
  }

  char* replaced_new = get_shadowed_original(new);
  if (!replaced_new) {
    return NULL;
  }

  if (0 != lstat(replaced_new, &st) || S_ISDIR(st.st_mode)) {
    free(replaced_new);
    return NULL;
  }

  return replaced_new;
}

__attribute__((visibility("default"))) int rename(
    const char* old,
    const char* new) {
  static int (*memoized_real_rename)(const char*, const char*) = NULL;
  // In a multi-threaded environment this is subject to a race, so use
  // GCC/CLANG an atomic load/stores to avoid pointer shear.
  int (*real_rename)(const char*, const char*) = NULL;
  __atomic_load(&memoized_real_rename, &real_rename, __ATOMIC_ACQUIRE);
  if (!real_rename) {
    real_rename = dlsym(RTLD_NEXT, "rename");
    // We don't mind if several threads race to store a value here, it
    // would presumably be the same anyway.
    __atomic_store(&memoized_real_rename, &real_rename, __ATOMIC_RELEASE);
  }

  char* original = get_shadowed_rename_dest(old, new);
  int ret;
  if (original) {
    fprintf(
        stderr,
        "`rename(%s, %s)` will replace shadowed original `%s`\n",
        old,
        new,
        original);
    ret = real_rename(old, original);
    free(original);
  } else {
    ret = real_rename(old, new);
  }

  return ret;
}

#ifdef __cplusplus
}
#endif
