/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include <glog/logging.h>
#include <glog/mutex.h>

// FB-specific gflags not in upstream glog but used throughout fbcode.
DEFINE_bool(logthreadnames, false, "include thread names in log messages");
DEFINE_bool(nologthreadids, false, "include thread ids in log messages");
DEFINE_bool(logthreadcontext, false, "include thread context in log messages");
DEFINE_int32(
    logthreadcontext_max_size,
    128,
    "the max size of the thread log context string");

namespace {

struct ThreadLocalData {
  char* threadName{NULL};
  char* threadLogContext{NULL};
};

// will be called on thread termination
void deleteThreadLocalData(void* ptr) {
  ThreadLocalData* tlsData = (ThreadLocalData*)ptr;
  if (tlsData) {
    delete[] tlsData->threadName;
    delete[] tlsData->threadLogContext;
    delete tlsData;
  }
}

// thread local vars to store thread name
pthread_key_t threadKey;
pthread_once_t threadKeyOnce = PTHREAD_ONCE_INIT;

// initializes the thread specific key that will map to the thread name
void makeThreadKey() {
  pthread_key_create(&threadKey, deleteThreadLocalData);
}

// gets the thread local data, making sure to create, if it does not exist yet
ThreadLocalData* getThreadLocalData() {
  if (pthread_once(&threadKeyOnce, makeThreadKey) != 0) {
    return NULL;
  }

  ThreadLocalData* tlsData = (ThreadLocalData*)pthread_getspecific(threadKey);
  if (tlsData == NULL) {
    tlsData = new (std::nothrow) ThreadLocalData();
    pthread_setspecific(threadKey, tlsData);
  }
  return tlsData;
}

// for assigning thread names, an int protected by a mutex
Mutex threadNumMutex;
int threadNum = 0;

// borrowed from folly/ThreadName.h: this looks a bit weird, but it's
// necessary to avoid having an undefined compiler function called.
#if defined(__GLIBC__) && !defined(__APPLE__)
#if __GLIBC_PREREQ(2, 12)
#define HAVE_PTHREAD_NAME_FUNCS
// The system limit on Linux
const int kMaxThreadName = 16;
#endif
#endif

} // namespace

#define EnvToString(envname, dflt) (!getenv(envname) ? (dflt) : getenv(envname))

DEFINE_string(
    customlogprefix,
    EnvToString("GLOG_customlogprefix", ""),
    "Add a custom prefix to the log messages."
    "Only printed if log_prefix is true");

// sets the logging context of the current thread
const char* setThreadLogContext(const std::string& context) {
  if (!FLAGS_logthreadcontext) {
    return NULL;
  }

  ThreadLocalData* tlsData = getThreadLocalData();
  if (tlsData == NULL) {
    return NULL;
  }

  if (tlsData->threadLogContext == NULL) {
    tlsData->threadLogContext =
        new (std::nothrow) char[FLAGS_logthreadcontext_max_size + 1];
  }

  if (tlsData->threadLogContext != NULL) {
    size_t size = context.size();
    if (size > FLAGS_logthreadcontext_max_size) {
      size = FLAGS_logthreadcontext_max_size;
    }
    memcpy(tlsData->threadLogContext, context.c_str(), size);
    tlsData->threadLogContext[size] = '\0';
  }

  return tlsData->threadLogContext;
}

// gets the logging context of the current thread
const char* google::getThreadLogContext() {
  if (!FLAGS_logthreadcontext) {
    return NULL;
  }

  ThreadLocalData* tlsData = getThreadLocalData();
  if (tlsData == NULL) {
    return NULL;
  }

  return tlsData->threadLogContext;
}
