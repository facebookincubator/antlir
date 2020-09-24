/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <unistd.h>
#include <fstream>
#include <gtest/gtest.h>

TEST(CppTest, TestRunningAsRoot) {
  EXPECT_EQ(getuid(), 0);
}

TEST(CppTest, TestEnv) {
  ASSERT_STREQ(std::getenv("kitteh"), "meow");
  ASSERT_STREQ(std::getenv("dogsgo"), "woof");
}

TEST(CppTest, TestRootfsIsWritable) {
  std::ofstream ofile("/some_path");
  ofile << "content";
  ofile.close();

  std::ifstream ifile("/some_path");
  std::string line;
  while (std::getline(ifile, line)) {
    ASSERT_EQ(line, "content");
  }
}
