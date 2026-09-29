# Phones without a Mac — building and signing, 26 Sep 2026

Eric: "I dont have a mac to build for Iphone, we have to use another route … I set up an Apple developer account … Have to work the other route for Android as well, then also get all of the signing stuff done and set up."

## The routes

| | iPhone / iPad | Android |
|---|---|---|
| **Built on** | A cloud Mac: GitHub Actions (`.github/workflows/ios.yml`). | Any Linux machine or the cloud (`atlas/mobile/build-android.sh`, `.github/workflows/android.yml`). Already built once in the cloud workspace. |
| **Signed with** | Apple's automatic signing plus an App Store Connect API key. Apple holds the certificate, so no signing key sits anywhere. | A key made **on your laptop**, which never leaves it (`atlas/mobile/sign-android.sh`). |
| **Installs on** | Only the iPhones registered in your Apple account (ad hoc), up to 100 a year. | Any Android phone, after allowing "install unknown apps" once. |
| **App identity** | `com.ericsnider.atlas`. It can never change. | `com.ericsnider.atlas`. |

## Done in this session

**Android:**
- The whole app built in the cloud: the Rust core for 64-bit ARM (with OpenSSL built in, since Android has none to link), the Kotlin, the JNI glue and the APK.
- The Kotlin compiled. The native library exports what the app calls and needs nothing Android 10 lacks.
- **Your Android signing key was made on your laptop:**
  - It's in `C:\Users\erics\Atlas\signing\atlas-android.p12`, with its password in `atlas-android.pass` beside it (never printed anywhere).
  - Certificate SHA-256: `DD:43:F2:F4:E4:DB:AF:BD:DD:0B:E7:87:B5:95:F6:E3:C2:9F:AB:17:89:B4:77:63:D2:D5:F1:41:3E:0D:8D:FD`.
  - It's backed up to `C:\Users\erics\Atlas-Vault\signing`.
  - It is **not** in the OneDrive vault. Copying it there puts it in Microsoft's cloud, so that's your call.
- **Atlas 0.1.0 for Android is signed with it on your laptop:** `4. Awaiting Merge\Atlas-0.1.0-android.apk`. `apksigner verify` passes.
- **Not yet run on a phone.** The cloud workspace has no emulator.

**iPhone:**
- The Xcode project is written as text (`atlas/mobile/ios/project.yml`); the cloud Mac generates the real project from it.
- The app has complete Info.plists, entitlements (the app group the share extension uses) and the widget extension's plist.
- The workflow does the whole build and signing. It stops early, naming any missing secret, and it logs which iPhones the build installs on and when it expires.
- **Fallback:** if Apple refuses cloud signing, `atlas/mobile/ios-certificate.sh` makes your own distribution certificate on Windows with openssl. It was tested with a stand-in certificate.
- **Never compiled.** The first run on the cloud Mac is the first compile of the Swift, so expect a fix or two.

## Your part (about 25 minutes)

### 1. GitHub: a private repository (5 minutes)

Your GitHub folder (`OneDrive\Documents\GitHub\Projects`) has no Atlas in it yet: one empty commit, no remote, and the Atlas folder there isn't tracked. So:

1. On github.com: **New repository → name `atlas` → Private → leave "Add a README" off → Create**.
2. Tell me. I push your laptop's `atlas-current` to it from your machine. A GitHub sign-in window may appear once; that's Git Credential Manager, already installed.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

### 2. Apple (15 minutes, on developer.apple.com and appstoreconnect.apple.com)

1. **Your Team ID.** At developer.apple.com → Account → Membership details. It's 10 characters.
2. **Register your iPhone.** An iPhone can only install the build if it's registered *before* the build.
   - **Find its UDID on Windows:** plug the iPhone in and open the **Apple Devices** app (or iTunes). Click the phone, then click the serial number until it changes to the UDID. Right-click it to copy.
   - **Add it:** developer.apple.com → Certificates, IDs & Profiles → **Devices → +** → iOS, give it a name, paste the UDID.
   - Every friend's iPhone later goes in the same way, and each one needs a new build afterwards.
3. **An API key for the build.**
   - Go to appstoreconnect.apple.com → **Users and Access → Integrations → App Store Connect API**. The first time, press **Request Access** and accept.
   - Then **Team Keys → Generate API Key**. Name it "Atlas build" and set **Access: Admin**. Admin is what lets the build use Apple's signing certificate.
   - **Download the .p8 file. Apple lets you download it only once.**
   - Note the **Key ID** (in the key's row) and the **Issuer ID** (above the list).

### 3. GitHub secrets (5 minutes)

In the repository: **Settings → Secrets and variables → Actions → New repository secret**, four times:

| Name | Value |
|---|---|
| `APPLE_TEAM_ID` | the Team ID |
| `ASC_KEY_ID` | the Key ID |
| `ASC_ISSUER_ID` | the Issuer ID |
| `ASC_KEY_P8` | open the .p8 in Notepad and paste all of it, including the BEGIN/END lines |

Then keep the .p8 file somewhere safe; your vault folder is a good place.

### 4. Build

**Actions → "iPhone and iPad app" → Run workflow.**
- It takes about 20–30 minutes.
- GitHub's free allowance covers about 200 Mac minutes a month, so roughly 7–10 builds. Past that it costs about 6¢ a minute.
- The finished `.ipa` is under the run's **Artifacts**.

## Getting it onto the phones

- **Android:** copy `Atlas-0.1.0-android.apk` to the phone (USB, Google Drive or email), tap it, and allow "install unknown apps" for that app once. Updates must be signed with the same key, which is why the key is backed up.
- **iPhone, first install.** There's no Mac, so the `.ipa` has to arrive another way. Two routes:
  - **In-house (the spec's route, §6):** your laptop's Atlas serves an install link over your own Tailscale with a valid HTTPS certificate. You tap the link on the iPhone and it installs. **Not built yet.** It's the next step once there's an `.ipa` to serve.
  - **A Windows tool (Sideloadly or AltStore):** these install over USB, but they re-sign the app with your Apple ID. Only with your OK, since it's software on your laptop.
- **Apple check-in.** The first launch of an ad hoc build checks in with Apple once, so the iPhone needs internet then. After that it works offline.

## Still open in the signing sitting

- **Atlas's own release key (B1).** Run `atlas release keygen` on the laptop and write down the recovery key on paper. It needs a working `atlas.exe` on the laptop (gap 1.1: no Windows toolchain there, so it's built in the cloud and copied over). You have to be at the keyboard, because the recovery key is shown once and must not pass through me.
- **Windows signing (B2, Azure Artifact Signing).** Check whether individual sign-up is open before paying.
- **iOS yearly re-signing (O2d).** The profile expires with your membership year. The build log shows the date.

## 27 Sep, later: the install page, Android on CI, Windows on CI

**`atlas install-page <Atlas.ipa | Atlas.apk> [--friends] [--minutes 20]`** (`src/ota.rs`, tested) puts a phone build on a page the phone installs it from.
- **What it reads.** It reads the .ipa itself: the version, and the devices and expiry in its profile. So the page can't disagree with the build.
- **What it serves.** The page, Apple's install manifest and the app, on 127.0.0.1 only, under a random path, for the minutes given.
- **How it reaches the phone.** Tailscale carries it over HTTPS on port 8443, leaving 443 to the hub:
  - `serve` for your own devices;
  - `--friends` uses `funnel`, which opens it publicly until the page closes.

  Either way it's switched off at the end.
- **Android.** A signed APK gets the same page with Android's steps. An unsigned one is refused.
- **Tested here:** a real socket, and the manifest and .ipa served correctly under a stand-in Tailscale.
- **Not yet tested:** an actual install. That needs Tailscale running on the laptop and the .ipa downloaded there.

**Android on GitHub.** The "Android app" workflow built successfully on its first run. Each build's versionCode is now 100 + the run number, so a new APK installs over an old one. The hand-built APKs are versionCode 1.

**Windows on GitHub.** The new "Windows app" workflow builds atlas.exe with Microsoft's own toolchain on a cloud Windows machine.
- The first run built it (about 34 minutes cold, because the release profile uses one codegen unit), and `atlas --version` ran on real Windows.
- It's unsigned, and the log says so. Signing switches on once six Azure Artifact Signing secrets are added.

**The phone has no language model of its own yet.** The main chat is right. `models.rs` runs inference through a `llama-server` process, and a phone app can't start one; iOS allows no child processes at all. So the phone thinks only by reaching the laptop's model over the tailnet. An on-device model means linking llama.cpp as a library into each app, or an inference engine inside the Rust core. That is its own piece of work, and it's on the plan.

## 27 Sep: the first iPhone build succeeded (run 5)

- The .ipa is signed for Eric's iPhone and iPad (two UDIDs) and expires 27 Sep 2027.
- Runs 1–4 failed at signing, each for a different reason:
  - App IDs with no App Group attached.
  - A profile that couldn't be read (the API doesn't list Xcode-managed profiles).
  - The group itself: developer.apple.com puts `group.` in front of what's typed, so it was registered as `group.group.com.ericsnider.atlas`. The app now uses that name as registered.
- The workflow now:
  - checks Apple's side first (`asc_profiles.py`);
  - on a failure, prints what each profile actually grants;
  - caches the Rust core;
  - has a `check_only` run that takes about a minute.
- Mac minutes: $2.60 of the free allowance used by 27 Sep. That's roughly 42 Mac minutes, with about 15 full builds left this month. The allowance resets on the 1st.
- Still open: the in-house install page (the .ipa has no way onto a phone yet without a Mac), and a first launch on a real iPhone.

## Decided 27 Sep: ad hoc now, unlisted store later

Eric wants builds that last more than 90 days, for him and his friends.
- **TestFlight is out,** because every TestFlight build expires at 90 days.
- **iPhone: ad hoc.** A build lasts about a year. Each friend's UDID is
  registered, then there's one new build.
- **Android: the sideloaded APK.** It doesn't expire. Google's developer
  verification reaches the US in 2027; register the app before then.
- **Unlisted App Store (iPhone) and a Play closed/internal testing track
  (Android)** are on the build plan as the lasting route. See
  MASTER_BUILD_PLAN.md §B5 for what each one needs.

## Sources

- [Apple: App Store Connect API — Account Holder requests access; team keys need Account Holder or Admin; keys download once](https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api/)
- [Apple forums: cloud signing with xcodebuild needs an Admin key](https://developer.apple.com/forums/thread/698117)
- [Apple: cloud-managed certificates](https://developer.apple.com/help/account/certificates/cloud-managed-certificates/)
- [GitHub Actions billing 2026: 2,000 free minutes; macOS about 10× Linux, $0.062/min](https://trimci.com/learn/github-actions-billing-explained/)
- [Apple: unlisted app distribution](https://developer.apple.com/support/unlisted-app-distribution)
- [Android developer verification timeline](https://android-developers.googleblog.com/2026/06/android-developer-verification.html)
- [Codemagic pricing (the alternative): 500 free Mac minutes a month](https://docs.codemagic.io/billing/pricing/)
