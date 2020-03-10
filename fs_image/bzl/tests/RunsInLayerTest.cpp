/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <gtest/gtest.h>
#include "common/files/FileUtil.h"

using namespace facebook::files;

TEST(RunsInLayerTest, TestUniquePathExists) {
  // Ensure that the containers are running inside the correct layer
  ASSERT_TRUE(FileUtil::isDirectory("/unique/test/path"));
}
