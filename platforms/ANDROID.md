# Atlas on Android

**State, 27 Sep 2026.**
- Builds on GitHub (Android run 1, 27 Sep, unsigned) and by hand.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- It has **never run on a phone.**

## Build

- **GitHub:** Actions → **Android app** → Run workflow. The artifact is `Atlas-Android-unsigned-<n>`, with versionCode 100 + the run number.
- **By hand, on Linux with the SDK and NDK r27c:** `ANDROID_HOME=… atlas/mobile/build-android.sh`. It uses versionCode `ATLAS_VERSION_CODE`, default 1.
- **What's in it:** 64-bit ARM only (`arm64-v8a`), minSdk 29 (Android 10), with OpenSSL built in (`native-tls` vendored, Android only).

## Sign (on the laptop only)

- **Command:** `sign-android.sh sign <unsigned.apk>`, using `C:\Users\erics\Atlas\signing\atlas-android.p12` and its `.pass` file. The key is backed up in `Atlas-Vault\signing`.
- **Always the same key.** Android refuses an update signed with a different one. Keep that key and its backup.

## Install

- **Copy it:** put the signed APK on the phone, open it, allow "install unknown apps" for that app once, then Install. If Play Protect says "unknown developer", tap More details → Install anyway.
- **Or serve it:** `atlas install-page Atlas.apk` serves a download page over Tailscale. It refuses an unsigned APK.
- **Updates** install over the old version as long as they have the same key and a higher versionCode.

## Coming

- **Developer verification:** Google's rule for installs outside Play reaches the US in 2027. Register `com.ericsnider.atlas` with this key first; the free limited-distribution account covers up to 20 devices (build plan B5).
- **The lasting route:** a Play internal or closed testing track (B5).

## Not proven yet

- A run on a real phone.
- Share, the notifications and hold-to-talk on a device.
- **Its own language model, built, not yet run on a phone** (P.7, 27 Sep): llama.cpp inside the core (`--features phone-llm` in `build-android.sh`; `c++_static c++abi` in the JNI `CMakeLists.txt`). Cross-built here for arm64-v8a with NDK r27c (doc 39). Say "get your own model" on wifi: Qwen3 1.7B on 8 GB+ phones, 0.6B on the rest.
