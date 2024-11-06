/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <stdio.h>
#include <stdlib.h>

int add(int a, int b) {
  return a + b;
}

int main(int argc, char** argv) {
  int a = atoi(argv[1]);
  int b = atoi(argv[2]);
  printf("%d + %d = %d\n", a, b, add(a, b));
  return 0;
}
