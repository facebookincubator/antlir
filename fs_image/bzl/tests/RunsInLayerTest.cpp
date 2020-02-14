// Copyright 2004-present Facebook. All Rights Reserved.

#include <gtest/gtest.h>
#include "common/files/FileUtil.h"

using namespace facebook::files;

TEST(RunsInLayerTest, TestUniquePathExists) {
  // Ensure that the containers are running inside the correct layer
  ASSERT_TRUE(FileUtil::isDirectory("/unique/test/path"));
}
