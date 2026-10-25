# Session — 26 Sep 2026: the whole hub, Atlas on phones, and accessibility

## What Eric said

"Before I go for a merge … I need you to finish building the hub for everything you said isn't built and the phone versions of all of it as we have determined Atlas will be a stand alone on the phones as well. Those are the designs i asked for so i am happy with it. Now we also need to look at accessibility laws for apps and implement all of those as well."

Then, on the first phone screenshots, two notes:

- "A phone screen does not have a Control K function, I need this to work on a Ipad in horizontal and vertical as well. and the little tabs at the bottom look weird with an even number… We need to rethink the phone/ipad UI."
- "…new types of devices being launched, so have to think about things like the IPhone Duo as well."

## What was built

### The rest of the hub, to the locked design

These are new renderers in `src/hubpages.rs`, fed by `hub_page_q`, `business_views` and `hub_post` in `src/hublive.rs`.

- **Messages.** People and groups in one list, the conversation beside it, a reply box, and read state.
- **Business.**
  - **Overview:** open work, people and clients.
  - **Shared tasks:** as a Table, a Board or a Calendar (`?view=`), each with owner and status as an icon and a word.
  - **Clients:** add one, and open one to see its facts.
  - **Partners:** the roster, marked "paired" or "not paired".
- **Documents.** The tray, with what Atlas made of each item.
- **Give.** Hand Atlas a link, words or a file. It also takes a share from the phone's share sheet (`share_target` in the manifest).
- **Sound & voice** (`src/sound.rs`). Settings that now act:
  - When replies are spoken: always, hands-free only, or never.
  - Volume, applied to the WAV itself.
  - Mute and quiet hours.
  - The locked pop-up rule: only when asked, when urgent, or anything.
  - The wake word and phrase.
  - `Daemon::say` and `reach_you` obey these, and a typed turn is marked, so "hands-free only" keeps its reply on screen.
- **Trusted recipients.**
- **Talk.** The conversation, a compose box, and hold-to-talk inside the phone app.
- **Offline.** What works with no internet, and what's waiting to go out.
- **Help & accessibility.** Using Atlas your way, the keyboard, the accessibility statement, and Report a barrier.
- **The calendar's week grid.**
- **Now** updates in place with a pause button. The meta refresh is gone because it fails WCAG 2.2.1.

### Atlas on the phone, standing alone

- **`src/mobile.rs`**, the phone core. The same Rust core is built `--no-default-features`. It serves the hub on 127.0.0.1 to the app's own WebView and is reached through three C functions (`mobile/atlas.h`).
- **Tested:** it starts from an empty app folder, serves the hub over loopback, answers `/hub/live.json` with a bearer token, refuses it without one, and stops.
- **Type-checks** for `aarch64-apple-ios` and `aarch64-linux-android`.
- **`mobile/ios`.** A SwiftUI WKWebView host, and hold-to-talk that only listens on the device (`requiresOnDeviceRecognition`) and speaks with the system voice. Also:
  - a share extension, through the app group folder;
  - a Live Activity and Dynamic Island that read `live.json`;
  - an Info.plist with every orientation, and plain http to loopback only.
- **`mobile/android`.** A Kotlin WebView and on-device / prefer-offline speech. Also:
  - an `ACTION_SEND` share into Give;
  - a foreground service whose notifications are the live activity (Working… / Ready for you: Open, Later);
  - C JNI glue, so there is no JNI crate;
  - a network config allowing plain http to 127.0.0.1 only.
- **Build steps** are in `mobile/README.md`. **Neither shell has been compiled.** There is no Xcode, Android SDK or device here.

### The phone and tablet UI, rethought

Eric was right on all three points, and two were deviations from his own design.

- **Tabs.** The 24 Sep phone artboards have **five** tabs, and a sixth raised "Talk" had been added. Back to the design's five, evenly spaced: Home, Projects, Messages, Business, Settings. Talk is the microphone in the top bar, and what's waiting on you is a badge on Home.
- **Keyboard hints.** No "Ctrl K" on any touch screen. On a Mac or iPad keyboard it reads ⌘K.
- **Layout by room, not device name,** so new shapes fit:
  - **600px or less (phone):** one top row (mark, Search, Talk, Help, Aa, Menu) and the five tabs. This covers a folded iPhone Duo and Split View halves.
  - **601–1019px (icon rail with names):** an iPhone Duo open, an iPad upright, a phone sideways, an Android foldable open.
  - **1020px and up:** the full sidebar, for an iPad sideways and the laptop.
- **Hinged screens.** Viewport Segments put the page list on one screen and the page on the other.
- **Touch.** 44px targets.
- **The iPhone Duo** (announced September 2026: 5.4" outer and 7.6" inner, same aspect ratio, ships 23 Oct). Its point widths were worked out from Apple's pixel counts, not measured.

### Accessibility, to law

- **Target: WCAG 2.2 AA plus EN 301 549** (chapters 5, 11 and 12). The laws checked:
  - ADA Title II: WCAG 2.1 AA, with its dates moved in April 2026 to April 2027 and 2028.
  - ADA Title III.
  - Section 508.
  - The European Accessibility Act (in force since June 2025). EN 301 549 v4.1.1 (September 2026) adopts WCAG 2.2 but isn't yet the cited version.
  - The UK and Canada.
- **`docs/ACCESSIBILITY.md`** holds the laws, what was built, how it's checked, and a VPAT-style conformance report.
- **Built:**
  - a skip link, one `main`, a named nav, and `aria-current` on the current page;
  - headings in order, and every control labelled;
  - status messages in `role=status`;
  - Help in the same place on every page;
  - no timed refresh;
  - forced colours and reduced motion followed;
  - contrast of at least 4.5:1 in every colourway, including colour-blind modes on dark;
  - reflow down to 320px;
  - no orientation lock anywhere.
- **Contrast fixes.** The design's text greys and accent-as-text were darkened in hue. `SPEC.md` records the exact values, and the design test now allows only those recorded fixes.
- **Native windows:**
  - `accesskit` is compiled into eframe on Windows only, so screen readers reach the windows through UI Automation. This is the one third-party addition, 11 crates in the Windows build. Other platforms would have pulled in a D-Bus stack, so it's kept off them.
  - Painted marks, step states and the QR code have names.
  - **`src/oslook.rs`** has the windows follow Windows' dark mode (when "Follow this computer" is chosen), high-contrast colours, text size and animation setting. It uses Windows' own documented calls with no crate, and type-checks for `x86_64-pc-windows-gnu`.
- **Real bugs found and fixed on the way:**
  - The palette's search box and the Friends "paste their link" box had no label.
  - Friends' placeholder had an unescaped apostrophe that broke the input.
  - The Groups, Accounts, Add-ons and pairing inputs relied on placeholders for their names.
- **Checked:**
  - axe-core 4.13, WCAG 2.2 AA rules, on all 35 pages at ten screen sizes (320px to laptop, iPhone Duo folded and open both ways, two iPads both ways): **0 violations, no sideways scroll**.
  - `tests/the_phone_stands_alone.rs` holds every page's structure, the forms, `live.json`, the manifest and the native-window settings.

## Not done — named

These are in `OPEN_GAPS.md` P.1 to P.6 and 4.3:

- **The phone shells** have not been compiled.
- **Android needs OpenSSL built for Android** before it will build.
- **iOS suspends Atlas in the background.** Reminders that fall due then wait until it's opened. Local notifications scheduled ahead would close this, and aren't built.
- **No hands-on screen-reader pass yet** (NVDA, JAWS, Narrator, VoiceOver, TalkBack).
- **The iPhone Duo's sizes** have not been measured on a real device.
- **`autocomplete` tokens** are not on every personal-data field yet.
- **Business Overview** has no invoices, because there is no invoice module.
- **Partners** shows paired or not, rather than online or offline.
- **Documents** shows every item as Private, because there is no share log.
- **Nothing here is legal advice.**
