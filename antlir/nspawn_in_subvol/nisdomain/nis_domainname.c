/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#define _GNU_SOURCE

#include <errno.h>
#include <stdio.h>
#include <string.h>
#include <sys/utsname.h>
#include <unistd.h>

int set_domain_name() {
  const char kNonBuildStepDomain[] = "AntlirNotABuildStep";
  if (setdomainname(kNonBuildStepDomain, sizeof(kNonBuildStepDomain)) == -1) {
    return errno;
  }
  return 0;
}

int print_domain_name() {
  struct utsname u;
  if (uname(&u) == -1) {
    return errno;
  }
  puts(u.domainname);
  return 0;
}

int main(int argc, char** argv) {
  if (argc <= 1) {
    return print_domain_name();
  }
  if (argc == 2) {
    if (strcmp("get", argv[1]) == 0) {
      return print_domain_name();
    }
    if (strcmp("set", argv[1]) == 0) {
      return set_domain_name();
    }
    // fall through to fail
  }
  fprintf(stderr, "Usage: %s [set|get]\n", argv[0]);
  return 1;
}
