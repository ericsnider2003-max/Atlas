# Atlas on the phone, standing alone

Eric ruled on 26 Sep 2026 that Atlas is a standalone app on phones as well. The phone does not open a window onto the laptop. It runs Atlas itself.

## How it fits together

**The core.** The phone runs the same Rust core as the laptop, built `--no-default-features`. That build leaves out the desktop windows and ONNX and uses `platform::mobile`.

**The screens.** The core serves the hub on `127.0.0.1` only, and the app shows it full screen in a WebView. The phone design (the tab bar, one column, safe areas and the Menu) is the hub's own pages at phone width. So every page the laptop hub has, the phone has, and every accessibility fix lands on both at once.

**The door.** The apps talk to the core through three C functions, declared in `atlas.h` and implemented in `src/mobile.rs`:

- `atlas_mobile_start(home, port)`
- `atlas_mobile_url(buf, len)`
- `atlas_mobile_stop()`

**What the native shell adds:**

- **Hold to talk** on the Talk page. The shell injects `window.AtlasShell.listen/stop/speak`, and the page shows the button only when those exist.
  - Speech is recognised on the phone: iOS uses `requiresOnDeviceRecognition`, and Android uses its on-device recogniser where the phone has one and prefers offline otherwise.
  - The words go to `/hub/talk` marked as spoken.
  - The reply is read out by the phone's own voice, only when Sound & voice allows it (mute, quiet hours and the reply rule are decided in the core).
- **Share → Atlas.** Links and text land on Give.
  - iOS uses a share extension that leaves the item in the app group's folder; the app hands it to `/hub/give` when it comes forward.
  - Android uses an `ACTION_SEND` intent.
  - The web-app manifest also declares a `share_target` for the installed web app.
- **The live activity.** It reads `/hub/live.json` from Atlas on the phone, with no push server.
  - iOS: a Live Activity card and a Dynamic Island pill. The card is shown only while something is working or ready.
  - Android: an ongoing notification plus an actionable "Ready for you" notification with Open and Later.
  - Status is always an icon and a word.

## Building — no Mac needed

Eric has no Mac (26 Sep 2026), so neither phone app is built on his machine.

**iPhone and iPad: a cloud Mac** (`.github/workflows/ios.yml`). GitHub's hosted Mac runs it on demand: *Actions → iPhone and iPad app → Run workflow*, or push a tag `v…`. In order, it:

1. builds the Rust core for `aarch64-apple-ios`;
2. turns `ios/project.yml` into the Xcode project with XcodeGen (there is no hand-made `.xcodeproj` to drift);
3. archives the app and exports an **ad hoc** `.ipa` for the iPhones registered in Eric's Apple Developer account;
4. lists the devices and the expiry date in its log.

**Signing uses Apple's automatic signing with an App Store Connect API key.** The distribution certificate is Apple-managed, so no signing key is exported or kept anywhere. The build needs four repository secrets, listed at the top of the workflow:
- `APPLE_TEAM_ID`
- `ASC_KEY_ID`
- `ASC_ISSUER_ID`
- `ASC_KEY_P8`

If Apple refuses its cloud-managed certificate (that needs the key's Admin role), `ios-certificate.sh` makes a distribution certificate on Windows with openssl, and two more secrets hand it to the build.

Every iPhone has to be registered (its UDID) in the Apple account *before* the build it's meant for. Adding a friend's iPhone means one more build.

**Android: any Linux machine, or the cloud** (`build-android.sh`, `.github/workflows/android.yml`).
- The core is built for 64-bit ARM with the NDK, including OpenSSL from source (`native-tls`'s `vendored` feature, Android only). The APK is built around it and comes out **unsigned**.
- **Signing happens on Eric's machine** with `sign-android.sh`, so the key that proves every update is his never leaves it:
  - `./sign-android.sh make-key` runs once, ever. It makes a PKCS12 key in `~/Atlas/signing` with its password in a file beside it; the password is never printed.
  - `./sign-android.sh sign app-release-unsigned.apk` runs each release. It needs only Java and `apksigner.jar` (one file, kept in `signing/tools`).
- **The same key for every release, forever.** Android refuses an update signed with a different key.

## What was checked here, and what wasn't

**Checked in the cloud workspace on 26 Sep:**

- The core type-checks for `aarch64-apple-ios` (`cargo check --no-default-features --lib --target aarch64-apple-ios`).
- `src/mobile.rs`'s test starts the phone core from a fresh app folder, fetches the hub over loopback with the token, and stops it.
- Every hub page passes axe-core (WCAG 2.2 AA rules) at 390px phone width, and reflows at 320px.

**Android, checked 26 Sep (evening):**
- The whole app builds: the Rust core (release, `aarch64-linux-android`, NDK r27c), the Kotlin, the JNI glue and the APK.
- The finished `libatlas_jni.so` exports the three doors and the JNI entry points, and needs nothing Android 10 doesn't have (the one newer libc call, `copy_file_range`, is weakly linked, which Rust's std expects).
- A throwaway key signed it, and `apksigner verify` and `zipalign -c` passed. The key was deleted afterwards.
- **Not yet run on a phone.** There's no emulator here (no KVM).

**iPhone: not checked here.** The Swift has never been compiled, and there is no Mac or Xcode in the cloud workspace. The first run of the workflow is the first compile. Expect a fix or two, and none of it has been used with VoiceOver yet.

**Known limits:**

- **iOS suspends apps in the background, and Atlas with them.** Reminders due while it is suspended fire when you next open it. Local notifications scheduled ahead would close most of that gap, and aren't built.
- **Android keeps Atlas running** with a foreground service. The service type is `specialUse`, which Play review may question.
- **Voice out** uses the phone's own system voice, not Atlas's Piper voice.

## The phone's own language model (27 Sep 2026)

- **Inside the app.** llama.cpp is compiled into the core (`--features phone-llm`; `phone-llm-metal` on iPhone and iPad, which runs it on the GPU) and loaded in-process (`src/phonemodel.rs`). No server, no other program: iOS allows neither.
- **Fetched when asked.** Say "get your own model" (best on wifi): Qwen3 1.7B (1.8 GB) on phones with 8 GB or more, Qwen3 0.6B (640 MB) on the rest, both pinned by SHA-256, resuming after a break. "how's the model download" says where it's got to. It's used as soon as it has loaded.
- **Your own connection stays as the fallback.** A `tools.llm` you set (for example your laptop's model over Tailscale) answers when the phone's own can't. Before 27 Sep the phone core had no model connection at all, and every model call went through a `curl` the phone can't start; both are fixed.
- **Linking.** Android: `c++_static c++abi` in `app/src/main/cpp/CMakeLists.txt`. iPhone: `-lc++` and the Metal, MetalKit and Accelerate frameworks in `ios/project.yml`.
- **Measured on two laptop CPU cores** (not a phone): 0.6B about 16 tokens a second, 1.7B about 6.5. Not yet run on a phone.
