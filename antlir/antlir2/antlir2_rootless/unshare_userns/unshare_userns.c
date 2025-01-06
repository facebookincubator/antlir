/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

// This is a helper library for unsharing the current process into a new,
// unprivileged user namespace.
// This is a little bit of a tricky dance that requires a few unsafe `fork()`s
// and pipe based communication to accomplish the following flow:
//
// ┌────────────┐    ┌───────┐       ┌───────┐
// │Main Process│    │Child 1│       │Child 2│
// └─────┬──────┘    └───┬───┘       └───┬───┘
//       │               │               │
//       │    fork()     │               │
//       │──────────────>│               │
//       │               │               │
//       │"I've unshared"│               │
//       │──────────────>│               │
//       │               │               │
//       │               │    fork()     │
//       │               │──────────────>│
//       │               │               │
//       │               │exec(newgidmap)│
//       │               │<──────────────│
//       │               │               │
//       │        exec(newuidmap)        │
//       │<──────────────────────────────│
// ┌─────┴──────┐    ┌───┴───┐       ┌───┴───┐
// │Main Process│    │Child 1│       │Child 2│
// └────────────┘    └───────┘       └───────┘
//
// 1. Main Process starts in the initial user namespace. It forks Child 1 (also
// in the initial user namespace).
//
// 2. Main Process unshares itself into a new user namespace. At this point,
// the new user namespace has no IDs mapped into it.
//
// 3. Main Process closes the write end of the pipe it gave to Child 1 to
// indicate that Main Process has created the new user namespace.
//
// 4. Child 1 forks Child 2 (also in the initial user namespace).
//
// 5. Child 2 execs /usr/bin/newgidmap to map GIDs into Main Process's new user
// namespace.
//
// 6. Child 1 execs /usr/bin/newuidmap to map UIDs into Main Process's new user
// namespace.
//
// 7. Main Process gets a 0 return code from Child 1 and continues its
// execution. Main Process's user namespace now has a full range of UIDs and
// GIDs mapped into it.

#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

// WARNING!!!!!
// This does a few `fork()`s with logic afterwards so we have to be careful not
// to accidentally do any dynamic memory allocation, which is not allowed
// between `fork()` and `exec()`.
int unshare_userns(
    char* pid_str,
    char* uid_map_outside_root,
    char* uid_map_outside_sub_start,
    char* uid_map_len,
    char* gid_map_outside_root,
    char* gid_map_outside_sub_start,
    char* gid_map_len) {
  int pipefd[2];
  if (pipe(pipefd) == -1) {
    return -1;
  }

  int child1 = fork();
  switch (child1) {
    case -1:
      close(pipefd[0]);
      close(pipefd[1]);
      return -1;
    case 0:
      // In the child process, wait for the parent process to indicate that it
      // has unshared into a new user namespace, then setup the id mappings
      // using the new{ug}idmap binaries

      // close our end of the write pipe, we won't be using it
      if (close(pipefd[1]) == -1) {
        exit(EXIT_FAILURE);
      }
      // this read() will complete as soon as the parent process closes its end
      // of the pipe
      char buf;
      read(pipefd[0], &buf, 1);
      close(pipefd[0]);

      int child2 = fork();
      switch (child2) {
        case -1:
          exit(EXIT_FAILURE);
        case 0: {
          // do newgidmap first
          char* args[] = {
              "newgidmap",
              pid_str,
              "0",
              gid_map_outside_root,
              "1",
              "1",
              gid_map_outside_sub_start,
              gid_map_len,
              NULL};
          if (execv("/usr/bin/newgidmap", args) == -1) {
            perror("exec newgidmap");
            exit(EXIT_FAILURE);
          }
          exit(EXIT_SUCCESS);
        }
        default: {
          // wait for the newgidmap to finish
          int status = 0;
          if (waitpid(child2, &status, 0) == -1) {
            exit(EXIT_FAILURE);
          }
          if (!WIFEXITED(status) || (WEXITSTATUS(status) != 0)) {
            exit(EXIT_FAILURE);
          }
        }
      }

      // now the newgidmap is done, do newuidmap
      char* args[] = {
          "newuidmap",
          pid_str,
          "0",
          uid_map_outside_root,
          "1",
          "1",
          uid_map_outside_sub_start,
          uid_map_len,
          NULL};
      if (execv("/usr/bin/newuidmap", args) == -1) {
        perror("exec newuidmap");
        exit(EXIT_FAILURE);
      };
      exit(EXIT_SUCCESS);

    default:
      close(pipefd[1]);
      // In the parent process, we must unshare the usernamespace, signal the
      // child process by closing our ends of the pipe and then wait for it to
      // exit, which signals that the namespace mapping is complete
      if (unshare(CLONE_NEWUSER) == -1) {
        close(pipefd[0]);
        return -1;
      }
      if (close(pipefd[0]) == -1) {
        return -1;
      }

      int status = 0;
      if (waitpid(child1, &status, 0) == -1) {
        exit(EXIT_FAILURE);
      }
      if (!WIFEXITED(status) || (WEXITSTATUS(status) != 0)) {
        return status;
      }
      return 0;
  }
}
