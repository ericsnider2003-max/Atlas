# Atlas on iPhone and iPad

**State, 27 Sep 2026.**
- Builds and signs on GitHub. Run 5 is signed ad hoc for your iPhone and iPad (two UDIDs), valid until **27 Sep 2027**.
- It has **never been installed or opened on a device.**

How it's put together (the core, the WebView hub, hold-to-talk, Share, the Live Activity) is in `atlas/mobile/README.md`.

## Build

1. **Register every device first:** developer.apple.com → Certificates, IDs & Profiles → Devices → +, with its UDID. To find a UDID on Windows: plug the device in, open the Apple Devices app, and click the serial number until it shows the UDID.
2. **Run the build:** GitHub → Actions → **iPhone and iPad app** → Run workflow. Tick "check only" for a one-minute check of Apple's side.
3. **Check the log:** it lists the devices the build covers and when it expires. The `.ipa` is under Artifacts.

**Secrets (set):** `APPLE_TEAM_ID`, `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_KEY_P8`. The optional fallback is `DIST_CERT_P12` / `DIST_CERT_PASSWORD` (`ios-certificate.sh`).

**Apple-side identifiers:**
- App IDs `com.ericsnider.atlas`, `.share` and `.live`.
- App Group `group.group.com.ericsnider.atlas`. The doubled `group.` is how it was registered, and the app uses it as registered.

**If signing fails,** the failure step prints what each profile grants. Runs 1–4 and their causes are in doc 36.

## Install (no Mac)

1. **Laptop:** Tailscale running and signed in; in the admin console, DNS page, **HTTPS Certificates** on.
2. **The .ipa** on the laptop.
3. **Serve it:** `atlas install-page Atlas.ipa`. It prints a link and a QR code; open it in **Safari** on the iPhone, which also needs Tailscale on.
4. **Install:** tap Install, then Install again. Open the app once with internet: Apple checks an ad hoc app on first launch.
5. **Friends:** `--friends` opens the page through Tailscale Funnel for the minutes given. That's held for your ruling (OPEN_GAPS P.8). Their UDIDs have to be in the build.

## Keeping it working

- **Rebuild yearly,** as the profile runs out with your membership year, and reinstall from the page.
- **A friend's new iPhone** means one more build.
- **The lasting route** is the Unlisted App Store (build plan B5). It needs a demo mode, a privacy policy, an icon and screenshots, and an upload step.

## Not proven yet

- An install and a first launch.
- The Swift has compiled but never run.
- The share extension, the Live Activity and hold-to-talk on a device.
- **Its own language model, built, not yet run on a phone** (P.7, 27 Sep): llama.cpp inside the app on Metal (`--features phone-llm-metal` in `ios.yml`; Metal, MetalKit, Accelerate and `-lc++` in `project.yml`). Say "get your own model" on wifi: Qwen3 1.7B on 8 GB iPhones and iPads, 0.6B on the rest. **The first `ios.yml` run with it is the first compile** -- nothing here can build for iOS. Before this the phone core had no model connection at all, and could not have used the laptop's either (every model call was a `curl` it can't start); both are fixed.
- iOS suspends apps in the background (P.3).
