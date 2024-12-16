/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "dep.h"
#include <rpm/rpmlib.h>

const char* dep_get_rpmlib_version() {
  return RPMVERSION;
}
