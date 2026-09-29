/* JNI glue from Kotlin to the one door in src/mobile.rs. Hand-written so the
   Rust core needs no JNI crate. */
#include <jni.h>
#include "../../../../../atlas.h"

JNIEXPORT jint JNICALL Java_app_atlas_AtlasCore_start(JNIEnv *env, jclass cls, jstring home, jint port) {
    const char *h = (*env)->GetStringUTFChars(env, home, 0);
    jint rc = atlas_mobile_start(h, (uint16_t)port);
    (*env)->ReleaseStringUTFChars(env, home, h);
    return rc;
}

JNIEXPORT jstring JNICALL Java_app_atlas_AtlasCore_url(JNIEnv *env, jclass cls) {
    char buf[512];
    if (atlas_mobile_url(buf, sizeof buf) <= 0) return NULL;
    return (*env)->NewStringUTF(env, buf);
}

JNIEXPORT void JNICALL Java_app_atlas_AtlasCore_network(JNIEnv *env, jclass cls, jboolean unmetered) {
    atlas_mobile_network(unmetered ? 1 : 0);
}

JNIEXPORT void JNICALL Java_app_atlas_AtlasCore_stopCore(JNIEnv *env, jclass cls) {
    atlas_mobile_stop();
}

JNIEXPORT jint JNICALL Java_app_atlas_AtlasCore_state(JNIEnv *env, jclass cls) {
    return atlas_mobile_state();
}
