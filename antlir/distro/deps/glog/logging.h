/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//
// fbcode glog has some non-upstreamed patches to enable a few key features:
// - CRITICAL and VERBOSE logging levels. Below we map these to existing GLOG
// levels but internally these are separate levels.
// - get/set thread local context: Helper functions to introspect thread-local
// storage that are used all over fbcode. These are exposed below
// - A few other gflag symbols not exported or defined in glog but are used in
// fbcode; these are defined and declared in this wrapper library.
//

#pragma once

// This is the _real_ glog/logging from the glog rpm.
#include <glog/_logging.h>

// Copied from fbcode/common/logging/logging.h
//
// This is not _strictly_ correct because VERBOSE will not actually log (since
// it's mapped to google::NullStream()).
#ifndef COMPACT_GOOGLE_LOG_VERBOSE
// This is the open source glog, which does not support VERBOSE.
// First, pretend VERBOSE is INFO.
namespace google {
constexpr auto GLOG_VERBOSE = GLOG_INFO;
constexpr auto VERBOSE = GLOG_INFO;
} // namespace google
#define COMPACT_GOOGLE_LOG_VERBOSE google::NullStream()
#define FACEBOOK_DETAIL_VLOG_IF_IMPL(cond) LOG_IF(INFO, cond)
#endif

#ifndef COMPACT_GOOGLE_LOG_CRITICAL
// This is the open source glog, which does not support CRITICAL.
// Map it to ERROR.
namespace google {
constexpr auto GLOG_CRITICAL = GLOG_ERROR;
constexpr auto CRITICAL = GLOG_ERROR;
} // namespace google
#define COMPACT_GOOGLE_LOG_CRITICAL COMPACT_GOOGLE_LOG_ERROR
#endif

// From glog headers; these symbols are not exposed from the library but are
// used internally in fbcode.
//
// The namespace is slightly different (fLS vs some other name). Unclear why
// this is the namespace that gets picked.
#ifndef DECLARE_VARIABLE
#define MUST_UNDEF_GFLAGS_DECLARE_MACROS
#define DECLARE_VARIABLE(type, name, tn)           \
  namespace fLS##tn {                              \
    extern GOOGLE_GLOG_DLL_DECL type FLAGS_##name; \
  }                                                \
  using fLS##tn::FLAGS_##name

// bool specialization
#define DECLARE_bool(name) DECLARE_VARIABLE(bool, name, bool)

// int32 specialization
#define DECLARE_int32(name) DECLARE_VARIABLE(google::int32, name, int32)

// Special case for string, because we have to specify the namespace
// std::string, which doesn't play nicely with our FLAG__namespace hackery.
#define DECLARE_string(name)                            \
  namespace fLS {                                       \
  extern GOOGLE_GLOG_DLL_DECL std::string FLAGS_##name; \
  }                                                     \
  using fLS::FLAGS_##name
#endif

// FB-specific gflags used to control various aspects of glog not in upstream.
// Include thread names in log messages.
DECLARE_bool(logthreadnames);
// Include thread ids in log messages.
DECLARE_bool(nologthreadids);
// Include thread context in log messages.
DECLARE_bool(logthreadcontext);
// The max size of the thread log context string.
DECLARE_int32(logthreadcontext_max_size);

#ifdef MUST_UNDEF_GFLAGS_DECLARE_MACROS
#undef MUST_UNDEF_GFLAGS_DECLARE_MACROS
#undef DECLARE_VARIABLE
#undef DECLARE_bool
#undef DECLARE_int32
#undef DECLARE_string
#endif

// This is private in glog but used quite a bit in fbcode.
namespace google {
const char* getThreadLogContext();
}
