/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <json/json.h>
#include <iostream>
#include "dep.h"

int main(int argc, char** argv) {
  Json::Value root;
  root["clang_version"] = __clang_version__;
  root["rpmlib_version"] = dep_get_rpmlib_version();
  root["platform_preprocessor_flag"] = PLATFORM_PREPROCESSOR_FLAG;
  std::cout << root << std::endl;
  return 0;
}
