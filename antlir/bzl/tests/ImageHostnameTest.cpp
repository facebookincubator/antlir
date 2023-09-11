/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <gtest/gtest.h>
#include <climits>

TEST(ImageHostname, TestContainer) {
  // Ensure the hostname configuration was propagated inside the container
  std::array<char, HOST_NAME_MAX> hostname;
  gethostname(hostname.data(), sizeof(hostname));
  ASSERT_STREQ("test-hostname.com", hostname.data());
}
