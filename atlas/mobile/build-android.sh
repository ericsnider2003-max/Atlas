#!/usr/bin/env bash
# Build the Android app: the Rust core for 64-bit ARM phones, then the APK
# around it. Used by .github/workflows/android.yml and by hand on any Linux
# machine with the Android SDK and NDK. The APK comes out UNSIGNED; sign it
# on Eric's machine with sign-android.sh.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
NDK="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/${NDK_VERSION:-27.2.12479018}}"
TC="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
API=29   # Android 10, the app's minSdk
[ -x "$TC/aarch64-linux-android$API-clang" ] || { echo "no NDK at $NDK" >&2; exit 2; }

export CC_aarch64_linux_android="$TC/aarch64-linux-android$API-clang"
export AR_aarch64_linux_android="$TC/llvm-ar"
export RANLIB_aarch64_linux_android="$TC/llvm-ranlib"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TC/aarch64-linux-android$API-clang"

cd "$here/.."
rustup target add aarch64-linux-android >/dev/null
# phone-llm: the phone's own language model, llama.cpp inside the core (P.7).
# llama.cpp's build uses CMake with the NDK's own toolchain file.
export CMAKE_TOOLCHAIN_FILE_aarch64_linux_android="$NDK/build/cmake/android.toolchain.cmake"
export ANDROID_ABI=arm64-v8a ANDROID_PLATFORM="android-$API"
export CXX_aarch64_linux_android="$TC/aarch64-linux-android$API-clang++"
cargo rustc --release --lib --no-default-features --features phone-llm --target aarch64-linux-android --crate-type staticlib
target_dir="${CARGO_TARGET_DIR:-$here/../target}"
mkdir -p "$here/android/app/src/main/rustlib/arm64-v8a"
cp "$target_dir/aarch64-linux-android/release/libatlas.a" "$here/android/app/src/main/rustlib/arm64-v8a/"

cd "$here/android"
# The project's own pinned Gradle (gradle/wrapper), never whichever Gradle the
# machine happens to have (28 Sep 2026).
./gradlew --no-daemon -q assembleRelease
ls -la app/build/outputs/apk/release/
