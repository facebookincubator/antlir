// Copyright 2004-present Facebook. All Rights Reserved.

#include <gtest/gtest.h>
#include <limits.h>

TEST(ImageHostname, TestContainer) {
  // Ensure the hostname configuration was propagated inside the container
  std::array<char, HOST_NAME_MAX> hostname;
  gethostname(hostname.data(), sizeof(hostname));
  ASSERT_STREQ("test-hostname.com", hostname.data());
}
