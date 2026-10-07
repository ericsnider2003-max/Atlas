# Google OAuth verification: what to fill in, and the demo video

Atlas asks Google for `openid email calendar.readonly` (Connect Google) and for `youtube.readonly yt-analytics.readonly` (Connect YouTube).

- **Calendar and YouTube scopes are "sensitive".** Google verifies them for free. There is no paid security assessment, because that only applies to "restricted" scopes such as full Gmail. Atlas doesn't ask for Gmail. It reads Gmail over IMAP with an app password.
- **Until it's verified:** users see "Google hasn't verified this app", and the app is capped at 100 users.

## 1. Before submitting (Eric, once)

1. **Publish the site.** This is `atlas/site/`. Publishing replaces the iPhone-only privacy page at https://ericsnider2003-max.github.io/atlas-privacy/.
   - Home: `…/atlas-privacy/`
   - Privacy policy: `…/atlas-privacy/privacy/`
   - Terms: `…/atlas-privacy/terms/`
2. **Prove you own the site** in Google Search Console (search.google.com/search-console):
   - Add the property `https://ericsnider2003-max.github.io/atlas-privacy/` as a URL prefix.
   - Choose the "HTML file" method and download the `google….html` file.
   - Give the file to Claude. It goes in `site/` and is published with the rest.
3. **Fill in the OAuth consent screen** (Google Cloud console → Google Auth Platform → Branding):

   | Field | Value |
   |---|---|
   | App name | Atlas Personal AI Assistant |
   | User support email | ericsnider2003@gmail.com |
   | App logo | Optional; adding one also needs it reviewed |
   | Application home page | https://ericsnider2003-max.github.io/atlas-privacy/ |
   | Privacy policy link | https://ericsnider2003-max.github.io/atlas-privacy/privacy/ |
   | Terms of service link | https://ericsnider2003-max.github.io/atlas-privacy/terms/ |
   | Authorized domain | ericsnider2003-max.github.io |
   | Developer contact | ericsnider2003@gmail.com |

4. **Data Access:** check that exactly these scopes are listed, and add a one-line reason for each:
   - **`calendar.readonly`:** "Shows the user their own schedule and reminds them of events, on their own computer. Read-only."
   - **`youtube.readonly`:** "Shows the user their own channel's subscribers, views and videos, and keeps a daily history on their computer."
   - **`yt-analytics.readonly`:** "Shows the user their own videos' audience retention (watch time, average view duration)."
5. **Audience:** the app is In production (published), External.
6. **Verification Center:** press "Prepare for verification" and submit it with the video link from part 2.

## 2. The demo video (unlisted YouTube, about 3 minutes)

Google's checklist says the video must show three things:
- the app's name on the consent screen;
- the sign-in flow in English;
- each scope being used.

Record the screen on the laptop in this order.

1. **About 10 s.** Atlas's hub at `localhost:8787`, with the address bar visible. Say: "This is Atlas Personal AI Assistant, which runs on my own computer."
2. **About 40 s.** Accounts page → **Sign in with Google**.
   - The browser opens Google's consent screen. Pause on it so the **app name** and the **calendar permission** are readable.
   - Choose the account and Allow. The page shows "Signed in…"; go back to Atlas.
3. **About 30 s.** Atlas shows "Connected …'s Google Calendar". Open the Calendar page so your Google events appear. Ask Atlas "what's on today?" and let it read an event. This shows `calendar.readonly` in use.
4. **About 40 s.** Social page → **Connect YouTube**.
   - Pause on the consent screen showing both YouTube permissions. Allow.
   - Back in Atlas, press "Refresh my numbers now". Show the YouTube row (subscribers, videos) and a video's retention.
   - This shows `youtube.readonly` and `yt-analytics.readonly` in use.
5. **About 20 s.** Show how to remove access: Accounts page → **Disconnect**, and myaccount.google.com/permissions.
6. **About 10 s.** Open the privacy policy page and scroll to "Google user data".

## 3. What the policy promises, and where the code keeps it

- **"Never sent to an online AI service while a Google account is connected."** Enforced by `brain::google_data_held` (`src/brain.rs`).
  - The flag is set from the Google calendar link, the YouTube sign-in, or any event read from Google (`connecting::note_google_data`).
  - It blocks three paths: the free online models (`freeonline.rs`), Muse's background work (`muse::MuseFirst`), and the fallback to any second model that isn't the user's own (`FallbackLlm::try_secondary`).
  - Tested in `tests/google_data_stays_here.rs`.
- **"Kept in the encrypted vault":** the refresh tokens are vault entries (`signin google …` and `youtube analytics sign-in`).
- **"Never sent to the developer":** Atlas has no server.
