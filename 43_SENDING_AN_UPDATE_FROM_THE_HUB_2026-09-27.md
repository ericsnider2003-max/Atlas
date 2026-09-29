# Sending an update from the hub (27 Sep 2026)

**Eric's ruling:** "Like downloading something on a computer… they don't have to go into files and click around or play in command prompt." Until now, the only way to send an update was `atlas release sign <version> windows-x86_64=<path>` in a terminal, followed by `atlas release announce`. This replaces both with one button.

## What Eric does

1. On GitHub: **Actions** → **Windows app** → the newest run → download **Atlas-Windows**.
2. In the hub: **Updates** → **Send an update to friends**. The page has already found the download: "Found **Atlas-Windows-57.zip**, downloaded 3 minutes ago: Atlas 0.2.0 for Windows."
3. Type the vault passphrase, then press **Sign and send Atlas 0.2.0**.

Friends' Atlases hear the notice in the release channel and fetch the program from Eric's Atlas. They check it against the signature and try it before keeping it.

## What Atlas does

| step | how | why |
|---|---|---|
| **Finds the build** | The newest file in Downloads or on the Desktop whose name starts with "atlas" and ends in `.exe` or `.zip`. A zip is opened for the Atlas program inside, never `tor/`. | No path is typed and no file is picked. |
| **Knows what it is** | The platform comes from the program's own header (x64 or Arm). The version comes from the version block written into the program, and only from the block whose ProductName is "Atlas". The program also carries Microsoft Edge's block, from WebView2. | Nothing is taken from the download's name. |
| **Signs exactly what was shown** | The button carries the program's fingerprint. If the file changed after the page was drawn, nothing is signed. | The yes is to that build. |
| **Refuses a key friends would refuse** | If the key in the vault isn't the one this Atlas was built with, it says so and signs nothing. | Otherwise the release would go out and do nothing. |
| **Never twice** | The last program sent is remembered, so the page says "went out as release 3" instead of offering it again. | The same program twice would be ignored by every copy anyway. |
| **Numbers** | The next release number is kept before anything goes out. | A number can't be reused. |
| **Vault** | Locked again afterwards if it was locked before. The passphrase is only ever a form field, never in an address. | |
| **Queues** | Puts the program aside for friends to fetch and writes the notice to the outbox. Atlas's usual loop posts it in every release channel Eric owns. With no channel yet, the page says it waits. | |

The section appears only on the releaser's Atlas, where the vault holds the release key, and only once a build carries that key. A friend's Atlas never shows it.

`atlas release sign` still works. It now keeps the release number in the same place the hub does (the releaser's store), so the two can be mixed.

## Measured

- `release` unit tests:
  - the header and version block, including an Edge block placed before Atlas's;
  - the real `windows/atlas-res.o` reads as this version;
  - the newest download is found, other files are passed over, and an unusable newest file is named with the reason;
  - a real zip, with `tor/` inside, is opened for the program;
  - signing is refused with no key and with the wrong key, and otherwise verifies as the next release.
- Through the daemon (`an_update_is_signed_and_sent_from_the_updates_page`):
  - a friend's Atlas has no section;
  - nothing found, then found;
  - a changed file is refused, and a wrong passphrase sends nothing and isn't echoed back;
  - it signs as release 1, relocks the vault, verifies the notice, and puts the program aside;
  - the same build is refused a second time;
  - the next build becomes release 2.
- The full suite: see the session's closing line.

## Not yet

- **Windows only.** Android and iPhone builds reach phones through their own stores and links, not this courier.
- **Until the key card is in a build** (`release=none` today), the section stays hidden: there's nothing a friend's copy would accept.

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
