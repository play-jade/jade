// SPDX-License-Identifier: Apache-2.0
//
// Android NDK JNI bridge routing Kotlin `play.jade.app.engine.NativeEngineBridge`
// calls directly through the narrow `jade_engine_*` C-ABI surface.
//
// On Android NDK / JDK hosts (`__has_include(<jni.h>)`), compiles against the
// platform `<jni.h>` with full `JavaVM` worker-thread attachment and global-ref
// callback dispatch. On non-JNI hosts (such as macOS CommandLineTools without a
// JDK installed), provides ABI-compatible opaque JNI typedefs so the symbols
// compile cleanly into `libjade_engine` for link/symbol verification.

#include "jade_engine.h"

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <memory>
#include <mutex>
#include <string>

#if defined(__has_include) && __has_include(<jni.h>)
  #include <jni.h>
  #define JADE_HAS_PLATFORM_JNI_H 1
#else
  #define JADE_HAS_PLATFORM_JNI_H 0
  typedef int32_t jint;
  typedef int64_t jlong;
  typedef uint8_t jboolean;
  typedef struct _jobject* jobject;
  typedef jobject jstring;
  typedef void JNIEnv;
  #if defined(_WIN32)
    #define JNIEXPORT __declspec(dllexport)
    #define JNICALL __stdcall
  #else
    #define JNIEXPORT __attribute__((visibility("default")))
    #define JNICALL
  #endif
#endif

namespace {

#if JADE_HAS_PLATFORM_JNI_H

class ScopedThreadAttachment {
 public:
  explicit ScopedThreadAttachment(JavaVM* vm) : vm_(vm) {
    if (vm_ == nullptr) return;
    void* raw_env = nullptr;
    jint status = vm_->GetEnv(&raw_env, JNI_VERSION_1_6);
    if (status == JNI_OK) {
      env_ = static_cast<JNIEnv*>(raw_env);
    } else if (status == JNI_EDETACHED) {
    #if defined(__ANDROID__)
      if (vm_->AttachCurrentThread(&env_, nullptr) == JNI_OK) {
        attached_here_ = true;
      }
    #else
      if (vm_->AttachCurrentThread(reinterpret_cast<void**>(&raw_env), nullptr) == JNI_OK) {
        env_ = static_cast<JNIEnv*>(raw_env);
        attached_here_ = true;
      }
    #endif
    }
  }

  ~ScopedThreadAttachment() {
    if (attached_here_ && vm_ != nullptr) {
      vm_->DetachCurrentThread();
    }
  }

  JNIEnv* env() const { return env_; }

 private:
  JavaVM* vm_ = nullptr;
  JNIEnv* env_ = nullptr;
  bool attached_here_ = false;
};

struct JniCallbackSlot {
  JavaVM* vm = nullptr;
  jobject listener_global = nullptr;
  jmethodID on_event_mid = nullptr;

  ~JniCallbackSlot() {
    if (vm != nullptr && listener_global != nullptr) {
      ScopedThreadAttachment attach(vm);
      if (JNIEnv* env = attach.env()) {
        env->DeleteGlobalRef(listener_global);
      }
      listener_global = nullptr;
    }
  }
};

struct JniEngineContext {
  JadeEngineHandle* handle = nullptr;
  JavaVM* vm = nullptr;
  std::mutex slot_mutex;
  std::shared_ptr<JniCallbackSlot> active_slot;
  std::shared_ptr<JniCallbackSlot> prev_slot;
};

void jni_search_callback_trampoline(
    const char* payload_json_utf8,
    size_t /*payload_len*/,
    void* user_data) {
  auto* slot = static_cast<JniCallbackSlot*>(user_data);
  if (slot == nullptr || slot->vm == nullptr || slot->listener_global == nullptr ||
      slot->on_event_mid == nullptr || payload_json_utf8 == nullptr) {
    return;
  }

  ScopedThreadAttachment attach(slot->vm);
  JNIEnv* env = attach.env();
  if (env == nullptr) return;

  jstring jpayload = env->NewStringUTF(payload_json_utf8);
  if (jpayload == nullptr) {
    if (env->ExceptionCheck()) env->ExceptionClear();
    return;
  }
  env->CallVoidMethod(slot->listener_global, slot->on_event_mid, jpayload);
  if (env->ExceptionCheck()) {
    env->ExceptionClear();
  }
  env->DeleteLocalRef(jpayload);
}

#else

struct JniEngineContext {
  JadeEngineHandle* handle = nullptr;
};

#endif  // JADE_HAS_PLATFORM_JNI_H

}  // namespace

extern "C" {

JNIEXPORT jlong JNICALL
Java_play_jade_app_engine_NativeEngineBridge_nativeCreate(
    JNIEnv* env,
    jobject /*thiz*/) {
  JadeEngineHandle* raw = nullptr;
  if (jade_engine_create(&raw) != JADE_ENGINE_OK || raw == nullptr) {
    return 0;
  }
  try {
    auto* ctx = new JniEngineContext();
    ctx->handle = raw;
#if JADE_HAS_PLATFORM_JNI_H
    if (env != nullptr) {
      env->GetJavaVM(&ctx->vm);
    }
#else
    (void)env;
#endif
    return reinterpret_cast<jlong>(ctx);
  } catch (...) {
    jade_engine_dispose(raw);
    return 0;
  }
}

JNIEXPORT jint JNICALL
Java_play_jade_app_engine_NativeEngineBridge_nativeConfigure(
    JNIEnv* env,
    jobject /*thiz*/,
    jlong handle_ptr,
    jstring options_json) {
  auto* ctx = reinterpret_cast<JniEngineContext*>(handle_ptr);
  if (ctx == nullptr || ctx->handle == nullptr || options_json == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
#if JADE_HAS_PLATFORM_JNI_H
  if (env == nullptr) return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  const char* utf = env->GetStringUTFChars(options_json, nullptr);
  if (utf == nullptr) return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  size_t len = std::strlen(utf);
  int32_t rc = jade_engine_configure(ctx->handle, utf, len);
  env->ReleaseStringUTFChars(options_json, utf);
  return static_cast<jint>(rc);
#else
  (void)env;
  return JADE_ENGINE_ERR_INVALID_ARGUMENT;
#endif
}

JNIEXPORT jint JNICALL
Java_play_jade_app_engine_NativeEngineBridge_nativeSearch(
    JNIEnv* env,
    jobject /*thiz*/,
    jlong handle_ptr,
    jstring request_json,
    jobject callback_listener) {
  auto* ctx = reinterpret_cast<JniEngineContext*>(handle_ptr);
  if (ctx == nullptr || ctx->handle == nullptr || request_json == nullptr ||
      callback_listener == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
#if JADE_HAS_PLATFORM_JNI_H
  if (env == nullptr) return JADE_ENGINE_ERR_INVALID_ARGUMENT;

  jclass cls = env->GetObjectClass(callback_listener);
  if (cls == nullptr) {
    if (env->ExceptionCheck()) env->ExceptionClear();
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  jmethodID mid = env->GetMethodID(cls, "onSearchEvent", "(Ljava/lang/String;)V");
  env->DeleteLocalRef(cls);
  if (mid == nullptr) {
    if (env->ExceptionCheck()) env->ExceptionClear();
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }

  jobject gref = env->NewGlobalRef(callback_listener);
  if (gref == nullptr) {
    if (env->ExceptionCheck()) env->ExceptionClear();
    return JADE_ENGINE_ERR_INTERNAL;
  }

  auto slot = std::make_shared<JniCallbackSlot>();
  slot->vm = ctx->vm;
  slot->listener_global = gref;
  slot->on_event_mid = mid;

  const char* utf = env->GetStringUTFChars(request_json, nullptr);
  if (utf == nullptr) return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  size_t len = std::strlen(utf);

  JniCallbackSlot* raw_slot = slot.get();
  {
    std::lock_guard<std::mutex> lock(ctx->slot_mutex);
    ctx->prev_slot = std::move(ctx->active_slot);
    ctx->active_slot = std::move(slot);
  }

  int32_t rc = jade_engine_search(
      ctx->handle, utf, len, &jni_search_callback_trampoline, raw_slot);
  env->ReleaseStringUTFChars(request_json, utf);
  return static_cast<jint>(rc);
#else
  (void)env;
  return JADE_ENGINE_ERR_INVALID_ARGUMENT;
#endif
}

JNIEXPORT jint JNICALL
Java_play_jade_app_engine_NativeEngineBridge_nativeCancel(
    JNIEnv* /*env*/,
    jobject /*thiz*/,
    jlong handle_ptr) {
  auto* ctx = reinterpret_cast<JniEngineContext*>(handle_ptr);
  if (ctx == nullptr || ctx->handle == nullptr) {
    return JADE_ENGINE_ERR_INVALID_ARGUMENT;
  }
  return static_cast<jint>(jade_engine_cancel(ctx->handle));
}

JNIEXPORT void JNICALL
Java_play_jade_app_engine_NativeEngineBridge_nativeDispose(
    JNIEnv* /*env*/,
    jobject /*thiz*/,
    jlong handle_ptr) {
  auto* ctx = reinterpret_cast<JniEngineContext*>(handle_ptr);
  if (ctx == nullptr) return;
  if (ctx->handle != nullptr) {
    jade_engine_cancel(ctx->handle);
    jade_engine_dispose(ctx->handle);
    ctx->handle = nullptr;
  }
#if JADE_HAS_PLATFORM_JNI_H
  {
    std::lock_guard<std::mutex> lock(ctx->slot_mutex);
    ctx->prev_slot.reset();
    ctx->active_slot.reset();
  }
#endif
  delete ctx;
}

JNIEXPORT jstring JNICALL
Java_play_jade_app_engine_NativeEngineBridge_nativeLastError(
    JNIEnv* env,
    jobject /*thiz*/,
    jlong handle_ptr) {
#if JADE_HAS_PLATFORM_JNI_H
  if (env == nullptr) return nullptr;
  auto* ctx = reinterpret_cast<JniEngineContext*>(handle_ptr);
  if (ctx == nullptr || ctx->handle == nullptr) {
    return env->NewStringUTF("");
  }
  size_t len = jade_engine_last_error(ctx->handle, nullptr, 0);
  std::string buf(len + 1, '\0');
  jade_engine_last_error(ctx->handle, buf.data(), buf.size());
  return env->NewStringUTF(buf.c_str());
#else
  (void)env;
  (void)handle_ptr;
  return nullptr;
#endif
}

}  // extern "C"
