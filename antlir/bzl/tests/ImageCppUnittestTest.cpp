/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <gtest/gtest.h>
#include <unistd.h>
#include <cstdlib>
#include <set>
#include <string>

TEST(ImageCppUnittest, TestContainer) {
  ASSERT_STREQ("nobody", ::getenv("USER"));
  // Future: add more assertions about the container setup as needed
}

TEST(ImageCppUnittest, TestEnv) {
  // Ensure that per-test `env` settings do reach the container.
  ASSERT_STREQ("meow", std::getenv("kitteh"));
  // Ensure that the container's environment is sanitized.
  //
  // Unlike the Python test, we don't check the environment against an
  // allowlist, but only because it's considerably more hassle to figure out
  // how to do this in GTest.
  ASSERT_EQ(nullptr, ::getenv("BUCK_BUILD_ID"));
}
