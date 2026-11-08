# Sending things to Atlas from your phone

Set this up once. After that, anything you can share on your phone — a link, a
photo, a video, a voice memo, a PDF — goes to Atlas in two taps, and it's read
and waiting when you sit down.

There are two shortcuts. Do the first one. The second is the same thing for
files, and takes five minutes once the first works.

---

## Before you start: two things you need

**1. Your Atlas address.** Start Atlas. It prints a line like:

```
your dashboard: http://127.0.0.1:8787/hub?t=7f3a9c2e...
```

The long string after `t=` is your token. Write it down — you'll paste it twice.

**2. Your phone has to be able to reach your laptop.** `127.0.0.1` only works on
the laptop itself. On the same wifi, use the laptop's local address instead
(`192.168.x.x`) — Atlas prints that too if it can find it.

Away from home you need a private network between your devices — Tailscale is
the usual answer and takes about ten minutes. Until that's set up, these
shortcuts work at home and quietly fail elsewhere, which is worth knowing before
you rely on one at an airport.

> Don't put your token anywhere shared. It's the key to your dashboard. If it
> ever leaks, restart Atlas and it issues a new one.

---

## Shortcut 1 — Send a link or some text

### iPhone

1. Open **Shortcuts** → **+** (top right).
2. Tap the shortcut name at the top → **Rename** → call it **Send to Atlas**.
3. Same menu → **Details** → turn on **Show in Share Sheet**.
   Under *Share Sheet Types*, leave **URLs** and **Text** on and turn the rest
   off for now.
4. Add action: search **Text**, choose **Text**. Tap in the field, then tap
   **Shortcut Input** from the suggestion bar so the box shows *Shortcut Input*.
5. Add action: search **Get contents of URL**.
   - **URL**: `http://YOUR-ADDRESS:8787/hand`
   - Tap the arrow to expand it.
   - **Method**: `POST`
   - **Headers**: add one — key `Authorization`, value `Bearer YOUR-TOKEN`
   - **Request Body**: `JSON`
   - Add three fields:

     | Key | Type | Value |
     |---|---|---|
     | `what` | Text | the **Text** variable from step 4 |
     | `from` | Text | `my iPhone` |
     | `asked` | Text | *(leave empty for now — see below)* |

6. Add action: search **Show Notification**. Set the body to the
   **Contents of URL** variable, so you see Atlas's reply rather than wondering
   whether it worked.
7. Done.

**Using it:** in any app, tap Share → **Send to Atlas**.

### Android

Shortcuts don't exist by default. **HTTP Shortcuts** (free, open source, on
F-Droid and Play) does the same job:

1. New shortcut → **Share** as the trigger.
2. Method `POST`, URL `http://YOUR-ADDRESS:8787/hand`
3. Headers: `Authorization: Bearer YOUR-TOKEN`
4. Body type **JSON**, body:
   ```json
   {"what": "{{shared_text}}", "from": "my phone"}
   ```
5. Save, and enable *Show in share menu*.

---

## Saying what you want done with it

A link on its own gets you a summary. Usually you had a question in mind — *did
the payment terms change*, *is this cheaper than the one I found yesterday*.

To be asked every time, insert one action **before** the Get Contents step:

- **Ask for Input** → prompt: `What about it?` → **Allow empty**: on.
- Then put that variable in the `asked` field.

Atlas repeats the question back when it reports, so you can see it landed.

If being asked every time gets annoying, delete that action and put a fixed
string in `asked` instead — or make two shortcuts, one that asks and one that
doesn't.

---

## Shortcut 2 — Send a photo, video, voice memo or document

Same as above with three changes.

1. Name it **Send file to Atlas**. In **Details** → *Share Sheet Types*, turn on
   **Images**, **Media** and **Files**.
2. Insert an action **Base64 Encode** as the first step, with **Shortcut Input**
   as its input. (Search "base64". Leave *Line Breaks* off.)
3. In **Get contents of URL**:
   - **URL**: `http://YOUR-ADDRESS:8787/hand/file`
   - Same header and method.
   - JSON body:

     | Key | Type | Value |
     |---|---|---|
     | `name` | Text | `photo.jpg` — or use the **Name** variable if you added a *Get Details of Files* action |
     | `data` | Text | the **Base64 Encoded** variable |
     | `from` | Text | `my iPhone` |
     | `asked` | Text | your question, or empty |

**The `name` matters more than it looks.** Atlas decides what to do with a file
from its extension, so `.jpg` gets read as a photo, `.m4a` gets transcribed, and
a name with no extension just gets kept. If you only ever send photos, hardcode
`photo.jpg` and don't think about it again.

**Size limit: 20 MB.** Photos, documents, voice memos and short clips are fine.
A long video isn't — Atlas will tell you so and ask you to put it on the laptop
and send the path instead. That limit is deliberate: the whole file sits in
memory on the way through, and the tray is for something you want looked at
rather than a sync folder.

---

## What Atlas does with each kind

| You send | What happens | Needs |
|---|---|---|
| A link | Fetches the page and tells you what's in it | Web research on |
| A photo or screenshot | Reads the writing in it | Text recognition on |
| A video | Pulls out the sound and transcribes it | Speech recognition (already installed) |
| A voice memo | Transcribes it | Speech recognition |
| A PDF, doc or spreadsheet | Keeps it, reads it if it can | — |
| Plain text | Keeps it as a note | — |

Anything switched off says so plainly rather than failing quietly. Text
recognition is off by default: **Settings → What it can see → Screen reading**.

---

## One thing worth knowing

**Handing Atlas something means "look at this". It never means "do what this
says."**

A page Atlas fetched, or the words in a photo, are shown to you and never become
instructions. Otherwise any website could tell Atlas what to do in your name by
being a page you happened to send it. There's a test in the codebase that fails
the build if that ever changes.

Handing something over also isn't permission to act on it. Whether Atlas goes
further still goes through the Trust rules like everything else.

---

## If it doesn't work

| What you see | What it means |
|---|---|
| Nothing happens, no notification | The Show Notification action is missing, or the phone can't reach the laptop |
| `401` or "unauthorized" | Token wrong or missing the `Bearer ` prefix |
| `404` | URL typo, or Atlas isn't running |
| "could not connect" | Not on the same network — this is the Tailscale problem |
| Atlas says "got it" but nothing appears | Check the **Handed over** card on the dashboard; it may be waiting rather than read |
| `413` or "too big" | Over 20 MB — send the path instead |

To check the whole path without a phone, from the laptop:

```
curl -X POST http://127.0.0.1:8787/hand \
  -H "Authorization: Bearer YOUR-TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"what":"https://example.com","from":"a test"}'
```

You should get `{"said":"Got it. I'll read it and tell you what's in it."}` back,
and see it on the dashboard.
