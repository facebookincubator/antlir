/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <json/json.h>
#include <math.h>
#include <iostream>
#include <span>
#include "dep.h"

int main(int argc, char** argv) {
  Json::Value root;
  root["clang_version"] = __clang_version__;
  root["rpmlib_version"] = dep_get_rpmlib_version();
  root["platform_preprocessor_flag"] = PLATFORM_PREPROCESSOR_FLAG;
  root["std"] = __cplusplus;
  // use a function from libm to prove that sysroot_dep works
  root["cos(0)"] = cos(0);

  std::cout << root << std::endl;

  // prove that we can compile code that uses std::span (which means we have at
  // least -std=c++20)
  int arr[] = {1, 2, 3};
  std::span<int> s(arr);

  return 0;
}
