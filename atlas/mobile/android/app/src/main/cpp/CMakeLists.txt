cmake_minimum_required(VERSION 3.22)
project(atlas_jni C)
# libatlas.a: the Rust core built for this ABI (mobile/build-android.sh puts it
# here). Not in jniLibs/, which is for finished .so files only.
add_library(atlas STATIC IMPORTED)
set_target_properties(atlas PROPERTIES IMPORTED_LOCATION ${CMAKE_SOURCE_DIR}/../rustlib/${ANDROID_ABI}/libatlas.a)
add_library(atlas_jni SHARED atlas_jni.c)
# --gc-sections keeps only what the three doors reach; the rest of the core
# comes along because they reach it.
target_link_options(atlas_jni PRIVATE -Wl,--gc-sections)
# c++_static and c++abi: the phone's own language model is llama.cpp (C++)
# inside libatlas.a (`phonemodel`, OPEN_GAPS P.7), linked in rather than
# shipping libc++_shared.so.
target_link_libraries(atlas_jni atlas log android dl m c++_static c++abi)
