/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <gtest/gtest.h>
#include <pwd.h>
#include <unistd.h>

TEST(CppTest, TestUser) {
  auto uid = geteuid();
  auto pw = getpwuid(uid);
  EXPECT_TRUE(pw);
  EXPECT_STREQ(pw->pw_name, std::getenv("TEST_USER"));
}

TEST(CppTest, TestEnvPropagated) {
  ASSERT_STREQ(std::getenv("ANTLIR2_TEST"), "1");
}
